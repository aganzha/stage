use crate::gio;
use crate::glib::Sender;
use ffi::OsString;
use git2::{
    ApplyLocation, ApplyOptions, Commit, Diff as GitDiff, DiffFile, DiffFormat, DiffHunk, DiffLine,
    DiffLineType, DiffOptions, ObjectType, Oid, Reference, Repository,
};
use log::{debug, error, info, log_enabled, trace};
use regex::Regex;
use std::{env, ffi, iter::zip, path, str};

fn get_current_repo(mut path_buff: path::PathBuf) -> Result<Repository, String> {
    let path = path_buff.as_path();
    Repository::open(path).or_else(|error| {
        println!("err while open repo {:?}", error);
        if !path_buff.pop() {
            return Err("no repoitory found".to_string());
        }
        get_current_repo(path_buff)
    })
}

#[derive(Debug, Clone)]
pub struct View {
    pub line_no: i32,
    pub expanded: bool,
    pub squashed: bool,
    pub rendered: bool,
    pub dirty: bool,
    pub child_dirty: bool,
    pub active: bool,
    pub current: bool,
    pub transfered: bool,
    pub tags: Vec<String>,
    pub markup: bool
}

#[derive(Debug, Clone)]
pub enum LineKind {
    File,
    Hunk,
    Regular,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub view: View,
    pub origin: DiffLineType,
    pub content: String,
}

impl Line {
    pub fn from_diff_line(l: &DiffLine) -> Self {
        return Self {
            view: View::new(),
            origin: l.origin_value(),
            content: String::from(str::from_utf8(l.content()).unwrap())
                .replace("\r\n", "")
                .replace('\n', ""),
        };
    }

    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        self.view.clone()
    }
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub view: View,
    pub header: String,
    pub old_start: u32,
    pub new_start: u32,
    pub old_lines: u32,
    pub new_lines: u32,
    pub lines: Vec<Line>,
}

impl Hunk {
    pub fn new() -> Self {
        Self {
            view: View::new(),
            header: String::new(),
            lines: Vec::new(),
            old_start: 0,
            new_start: 0,
            old_lines: 0,
            new_lines: 0,
        }
    }

    pub fn get_header_from(dh: &DiffHunk) -> String {
        String::from(str::from_utf8(dh.header()).unwrap())
            .replace("\r\n", "")
            .replace('\n', "")
    }

    pub fn reverse_header(header: String) -> String {
        // "@@ -1,3 +1,7 @@" -> "@@ -1,7 +1,3 @@"
        // "@@ -20,10 +24,11 @@ STAGING LINE..." -> "@@ -24,11 +20,10 @@ STAGING LINE..."
        // "@@ -54,7 +59,6 @@ do not call..." -> "@@ -59,6 +54,7 @@ do not call..."
        let re = Regex::new(r"@@ [+-]([0-9].*,[0-9]*) [+-]([0-9].*,[0-9].*) @@").unwrap();
        if let Some((whole, [nums1, nums2])) = re.captures_iter(&header).map(|c| c.extract()).next()
        {
            // for (whole, [nums1, nums2]) in re.captures_iter(&header).map(|c| c.extract()) {
            let result = whole
                .replace(nums1, "mock")
                .replace(nums2, nums1)
                .replace("mock", nums2);
            return header.replace(whole, &result);
        }
        panic!("cant reverse header {}", header);
        String::from("fail")
    }

    pub fn title(&self) -> String {
        self.header.to_string()
    }

    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        // hunk headers are changing always
        // during partial staging
        clone.dirty = true;
        clone.transfered = true;
        clone
    }
    pub fn push_line(&mut self, line: Line) {
        match line.origin {
            DiffLineType::FileHeader | DiffLineType::HunkHeader | DiffLineType::Binary => {}
            _ => self.lines.push(line),
        }
    }
    pub fn enrich_views(&mut self, other: Hunk) {
        if self.lines.len() != other.lines.len() {
            // so :) what todo?
            panic!(
                "lines length are not the same {:?} {:?}",
                self.lines.len(),
                other.lines.len()
            );
        }
        for pair in zip(&mut self.lines, &other.lines) {
            pair.0.view = pair.1.transfer_view();
        }
    }
}

impl Default for Hunk {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub view: View,
    pub path: OsString,
    pub id: Oid,
    pub hunks: Vec<Hunk>,
}

impl File {
    pub fn new() -> Self {
        Self {
            view: View::new(),
            path: OsString::new(),
            id: Oid::zero(),
            hunks: Vec::new(),
        }
    }
    pub fn from_diff_file(f: &DiffFile) -> Self {
        return File {
            view: View::new(),
            path: f.path().unwrap().into(),
            id: f.id(),
            hunks: Vec::new(),
        };
    }

    pub fn push_hunk(&mut self, h: Hunk) {
        self.hunks.push(h);
    }

    pub fn title(&self) -> String {
        self.path.to_str().unwrap().to_string()
    }

    pub fn enrich_views(&mut self, other: File) {
        // used to maintain view state in existent hunks
        // there are 2 cases
        // 1. side from which hunks are moved out (eg unstaged during staging)
        // this one is simple, cause self.hunks and other.hunks are the same length.
        // just transfer views between them in order.
        // 2. side on which hunks are receiving new hunk (eg staged hunks during staging)
        // this one is hard, cause new hunk (there will be only 1), could break the order
        // of existent hunks and their headers will mutate (line numbers will differ)
        if self.hunks.len() == other.hunks.len() {
            for pair in zip(&mut self.hunks, &other.hunks) {
                pair.0.view = pair.1.transfer_view();
                pair.0.enrich_views(pair.1.clone());
            }
            return;
        }
        // So. It need to map new hunks (self) to existent one: other.
        // Example of new hunks
        // "@@ -1,3 +1,7 @@"
        // "@@ -20,10 +24,11 @@ STAGING LINE INSIDE HUNK (STAGING HUNK MUST BE THE SAME)"
        // "@@ -54,7 +59,6 @@ do not call render on everything"
        // Example of exising hunks
        // "@@ -1,3 +1,7 @@",
        // "@@ -55,7 +59,6 @@ do not call render on everything",
        assert!(self.hunks.len() == other.hunks.len() + 1);
        let mut mine_cursor = 0;
        let mut other_cursor = 0;
        let mut hunk_to_exclude = String::new();
        for pair in zip(&self.hunks, &other.hunks) {
            mine_cursor += pair.0.new_start + pair.0.new_lines - pair.0.old_lines;
            other_cursor += pair.1.new_start + pair.1.new_lines - pair.1.old_lines;
            if mine_cursor != other_cursor {
                hunk_to_exclude = pair.0.header.clone();
                break;
            }
        }
        let filtered: Vec<&mut Hunk> = self
            .hunks
            .iter_mut()
            .filter(|h| h.header != hunk_to_exclude)
            .collect();

        for pair in zip(filtered, &other.hunks) {
            pair.0.view = pair.1.transfer_view();
            pair.0.enrich_views(pair.1.clone());
        }
    }

    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone
    }
}

impl Default for File {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Diff {
    pub files: Vec<File>,
    pub view: View,
    pub dirty: bool,
}

impl Diff {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            view: View::new(),
            dirty: false,
        }
    }

    pub fn add(&mut self, other: Diff) {
        for file in other.files {
            self.files.push(file);
        }
    }

    pub fn enrich_views(&mut self, other: Diff) {
        for file in &mut self.files {
            for of in &other.files {
                if file.path == of.path {
                    file.view = of.transfer_view();
                    file.enrich_views(of.clone());
                }
            }
        }
    }
}

pub fn get_cwd_repo(sender: Sender<crate::Event>) -> Repository {
    let path_buff = env::current_exe().expect("cant't get exe path");
    let repo = get_current_repo(path_buff).expect("cant't get repo for current exe");
    let path = OsString::from(repo.path());
    sender
        .send(crate::Event::CurrentRepo(path.clone()))
        .expect("Could not send through channel");
    repo
}

#[derive(Debug, Clone, Default)]
pub struct Head {
    pub commit: String,
    pub branch: String,
    pub view: View
}

impl Head {
    pub fn new(commit: Commit, head_ref: Reference) -> Self {
        Self {
            commit: commit_string(&commit),
            branch: local_branch_name(String::from(head_ref.name().or(Some("")).unwrap())),
            view: View::new_markup()
        }
    }
}

pub fn commit_string(c: &Commit) -> String {
    return format!(
        "{} {}",
        &c.id().to_string()[..7],
        c.message().or(Some("")).unwrap().replace("\n", "")
    );
}

pub fn local_branch_name(name: String) -> String {
    name.replace("refs/heads/", "")
}

pub fn get_current_repo_status(current_path: Option<OsString>, sender: Sender<crate::Event>) {
    let (repo, path) = {
        if let Some(path) = current_path {
            let repo = Repository::open(path.clone()).expect("can't open repo");
            (repo, path)
        } else {
            let repo = get_cwd_repo(sender.clone());
            let path = OsString::from(repo.path());
            (repo, path)
        }
    };

    // get HEAD
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            let repo = Repository::open(path).expect("can't open repo");
            let head_ref = repo.head().expect("can't get head");
            assert!(head_ref.is_branch());
            debug!("branch name {:?}", head_ref.name());
            let ob = head_ref
                .peel(ObjectType::Commit)
                .expect("can't get commit from ref!");
            let commit = ob.peel_to_commit().expect("can't get commit from ob!");
            sender
                .send(crate::Event::Head(Head::new(commit, head_ref)))
                .expect("Could not send through channel");
        }
    });

    // get staged
    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            let repo = Repository::open(path).expect("can't open repo");
            let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree = repo.find_tree(ob.id()).expect("no working tree");
            let git_diff = repo
                .diff_tree_to_index(Some(&current_tree), None, None)
                .expect("can't get diff tree to index");
            let diff = make_diff(git_diff);
            sender
                .send(crate::Event::Staged(diff))
                .expect("Could not send through channel");
        }
    });
    // get unstaged
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant' get diff index to workdir");
    let diff = make_diff(git_diff);
    sender
        .send(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}

#[derive(Debug, Clone, Default)]
pub struct ApplyFilter {
    pub file_path: String,
    pub hunk_header: String,
}

pub fn make_diff(git_diff: GitDiff) -> Diff {
    let mut diff = Diff::new();
    let mut current_file = File::new();
    let mut current_hunk = Hunk::new();
    let _res = git_diff.print(DiffFormat::Patch, |diff_delta, o_diff_hunk, diff_line| {
        let new_file = diff_delta.new_file();
        let oid = new_file.id();
        if oid.is_zero() {
            todo!();
        }
        if new_file.path().is_none() {
            todo!();
        }
        if current_file.id.is_zero() {
            // init new file
            current_file = File::from_diff_file(&new_file);
        }
        if current_file.id != oid {
            // go to next file
            // push current_hunk to file and init new empty hunk
            current_file.push_hunk(current_hunk.clone());
            current_hunk = Hunk::new();
            // push current_file to diff and change to new file
            diff.files.push(current_file.clone());
            current_file = File::from_diff_file(&new_file);
        }
        if let Some(diff_hunk) = o_diff_hunk {
            let hh = Hunk::get_header_from(&diff_hunk);
            let old_start = diff_hunk.old_start();
            let old_lines = diff_hunk.old_lines();
            let new_start = diff_hunk.new_start();
            let new_lines = diff_hunk.new_lines();
            if current_hunk.header.is_empty() {
                // init hunk
                current_hunk.header = hh.clone();
                current_hunk.old_start = old_start;
                current_hunk.old_lines = old_lines;
                current_hunk.new_start = new_start;
                current_hunk.new_lines = new_lines;
            }
            if current_hunk.header != hh {
                // go to next hunk
                current_file.push_hunk(current_hunk.clone());
                current_hunk = Hunk::new();
                current_hunk.header = hh.clone();
                current_hunk.old_start = old_start;
                current_hunk.old_lines = old_lines;
                current_hunk.new_start = new_start;
                current_hunk.new_lines = new_lines;
            }
            current_hunk.push_line(Line::from_diff_line(&diff_line));
        } else {
            // this is file header line.
            current_hunk.push_line(Line::from_diff_line(&diff_line))
        }

        true
    });
    if !current_hunk.header.is_empty() {
        current_file.push_hunk(current_hunk);
    }
    if !current_file.path.is_empty() {
        diff.files.push(current_file);
    }
    diff
}

pub fn stage_via_apply(
    unstaged: Option<Diff>,
    staged: Option<Diff>,
    is_staging: bool,
    path: OsString,
    filter: ApplyFilter,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let git_diff = {
        if is_staging {
            repo.diff_index_to_workdir(None, None)
                .expect("can't get diff")
        } else {
            let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree = repo.find_tree(ob.id()).expect("no working tree");
            repo.diff_tree_to_index(
                Some(&current_tree),
                None,
                Some(DiffOptions::new().reverse(true)),
            )
            .expect("can't get diff")
        }
    };
    let mut options = ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if filter.hunk_header.is_empty() {
            return true;
        }
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            return {
                if is_staging {
                    filter.hunk_header == header
                } else {
                    filter.hunk_header == Hunk::reverse_header(header)
                }
            };
        }
        false
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let new_file = dd.new_file();
            let file = File::from_diff_file(&new_file);
            return filter.file_path == file.path.into_string().unwrap();
        }
        true
    });
    repo.apply(&git_diff, ApplyLocation::Index, Some(&mut options))
        .expect("can't apply patch");

    // staged changes
    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            let repo = Repository::open(path).expect("can't open repo");
            let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree = repo.find_tree(ob.id()).expect("no working tree");
            let git_diff = repo
                .diff_tree_to_index(Some(&current_tree), None, None)
                .expect("can't get diff tree to index");
            let mut diff = make_diff(git_diff);
            if let Some(s) = staged {
                diff.enrich_views(s);
            }
            sender
                .send(crate::Event::Staged(diff))
                .expect("Could not send through channel");
        }
    });
    // unstaged changes
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant get diff_index_to_workdir");
    let mut diff = make_diff(git_diff);
    if let Some(u) = unstaged {
        diff.enrich_views(u);
    }
    sender
        .send(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}

pub fn commit_staged(path: OsString, message: String, sender: Sender<crate::Event>) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");
    // update_ref: Option<&str>,
    // author: &Signature<'_>,
    // committer: &Signature<'_>,
    // message: &str,
    // tree: &Tree<'_>,
    // parents: &[&Commit<'_>]
    let tree_oid = repo
        .index()
        .expect("can't get index")
        .write_tree()
        .expect("can't write tree");
    let tree = repo.find_tree(tree_oid).expect("can't find tree");
    let ob = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse");
    let parent = repo.find_commit(ob.id()).expect("can't find commit");
    let result = repo
        .commit(Some("HEAD"), &me, &me, &message, &tree, &vec![&parent])
        .expect("can't commit");
    println!("cooooooooooooooommmit {:?}", result);

    // update staged changes
    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_index(Some(&current_tree), None, None)
        .expect("can't get diff tree to index");
    sender
        .send(crate::Event::Staged(make_diff(git_diff)))
        .expect("Could not send through channel");
    // TODO! signal above with unstaged!
    // NO! it does not require update STAGED. just unstaged are gone!
    // so. get status for only head on top and cleanup unstaged changes.
    // perhaps here just return top head and thats it!
    // yes. exactly that. thats how stage_via_apply works.
    // it calls manually staged and unstaged and enrich
    // views! so, for now i will put it to main.
    // get_current_repo_status(Some(path.clone()), sender)
}
