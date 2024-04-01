use crate::gio;
// use crate::glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;

use chrono::{DateTime, FixedOffset, LocalResult, TimeZone};
use ffi::OsString;
use git2::build::CheckoutBuilder;
use git2::{
    ApplyLocation, ApplyOptions, Branch, BranchType, CertificateCheckStatus,
    CherrypickOptions, Commit, Cred, CredentialType, Delta, Diff as GitDiff,
    DiffFile, DiffFormat, DiffHunk, DiffLine, DiffLineType, DiffOptions,
    Error, ObjectType, Oid, PushOptions, RemoteCallbacks, Repository,
    RepositoryState,
};
use log::{debug, info, trace};
use regex::Regex;
use std::cmp::Ordering;
use std::{env, ffi, path, str};

fn get_current_repo(
    mut path_buff: path::PathBuf,
) -> Result<Repository, String> {
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
    pub markup: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub view: View,
    pub origin: DiffLineType,
    pub content: String,
    pub new_line_no: Option<u32>,
    pub old_line_no: Option<u32>,
}

impl Line {
    pub fn from_diff_line(l: &DiffLine) -> Self {
        return Self {
            view: View::new(),
            origin: l.origin_value(),
            new_line_no: l.new_lineno(),
            old_line_no: l.old_lineno(),
            content: String::from(str::from_utf8(l.content()).unwrap())
                .replace("\r\n", "")
                .replace('\n', ""),
        };
    }
    pub fn hash(&self) -> String {
        // IT IS NOT ENOUGH! will be "Context" for
        // empty grey line!
        format!("{}{:?}", self.content, self.origin)
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
    pub max_line_len: i32,
}

#[derive(Debug, Clone)]
pub enum Related {
    Before,
    OverlapBefore,
    Matched,
    OverlapAfter,
    After,
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
            max_line_len: 0,
        }
    }

    pub fn get_header_from(dh: &DiffHunk) -> String {
        String::from(str::from_utf8(dh.header()).unwrap())
            .replace("\r\n", "")
            .replace('\n', "")
    }

    pub fn handle_max(&mut self, line: &String) {
        let le = line.len() as i32;
        if le > self.max_line_len {
            self.max_line_len = le;
        }
    }

    pub fn fill_from(&mut self, dh: &DiffHunk) {
        let header = Self::get_header_from(dh);
        self.handle_max(&header);
        self.header = header;
        self.old_start = dh.old_start();
        self.old_lines = dh.old_lines();
        self.new_start = dh.new_start();
        self.new_lines = dh.new_lines();
    }

    pub fn reverse_header(header: String) -> String {
        // "@@ -1,3 +1,7 @@" -> "@@ -1,7 +1,3 @@"
        // "@@ -20,10 +24,11 @@ STAGING LINE..." -> "@@ -24,11 +20,10 @@ STAGING LINE..."
        // "@@ -54,7 +59,6 @@ do not call..." -> "@@ -59,6 +54,7 @@ do not call..."
        let re =
            Regex::new(r"@@ [+-]([0-9].*,[0-9]*) [+-]([0-9].*,[0-9].*) @@")
                .unwrap();
        if let Some((whole, [nums1, nums2])) =
            re.captures_iter(&header).map(|c| c.extract()).next()
        {
            // for (whole, [nums1, nums2]) in re.captures_iter(&header).map(|c| c.extract()) {
            let result = whole
                .replace(nums1, "mock")
                .replace(nums2, nums1)
                .replace("mock", nums2);
            return header.replace(whole, &result);
        }
        panic!("cant reverse header {}", header);
    }
    pub fn delta_in_lines(&self) -> i32 {
        // returns how much lines this hunk
        // will add to file (could be negative when lines are deleted)
        self.lines
            .iter()
            .map(|l| match l.origin {
                DiffLineType::Addition => 1,
                DiffLineType::Deletion => -1,
                _ => 0,
            })
            .sum()
    }

    pub fn related_to(
        &self,
        other: &Hunk,
        kind: Option<&DiffKind>,
    ) -> Related {
        let (start, lines, other_start, other_lines) = {
            match kind {
                Some(DiffKind::Staged) => (
                    self.new_start,
                    self.new_lines,
                    other.new_start,
                    other.new_lines,
                ),
                Some(DiffKind::Unstaged) => (
                    self.old_start,
                    self.old_lines,
                    other.old_start,
                    other.old_lines,
                ),
                _ => panic!("no kind in related to"),
            }
        };
        debug!(
            ">>> related_to_other NEW HUNK start {:?} lines {:?}
                  OLD HUNK start {:?} {:?} kind {:?}",
            start, lines, other_start, other_lines, kind
        );

        if start < other_start && start + lines < other_start {
            debug!("before");
            return Related::Before;
        }

        if start < other_start && start + lines >= other_start {
            debug!("overlap");
            return Related::OverlapBefore;
        }

        if start == other_start && lines == other_lines {
            debug!("matched");
            return Related::Matched;
        }
        if start > other_start && start <= other_start + other_lines {
            debug!("overlap");
            return Related::OverlapAfter;
        }
        if start > other_start && start > other_start + other_lines {
            debug!("after");
            return Related::After;
        }
        // GOT PANIC HERE
        // unknown case self.new_start 489 self.new_lines 7
        //          other.new_start 488 other.new_lines 7
        // some files were staged. and 1 of them was unstaged also
        // staged it and got panic
        panic!(
            "unknown case start {:?} lines {:?}
                  other_start {:?} other_lines {:?} kind {:?}",
            start, lines, other_start, other_lines, kind
        );
    }

    pub fn title(&self) -> String {
        self.header.to_string()
    }

    pub fn push_line(&mut self, line: Line) {
        match line.origin {
            DiffLineType::FileHeader
            | DiffLineType::HunkHeader
            | DiffLineType::Binary => {}
            _ => {
                self.handle_max(&line.content);
                self.lines.push(line)
            }
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
    pub max_line_len: i32,
}

impl File {
    pub fn new() -> Self {
        Self {
            view: View::new(),
            path: OsString::new(),
            id: Oid::zero(),
            hunks: Vec::new(),
            max_line_len: 0,
        }
    }
    pub fn from_diff_file(f: &DiffFile) -> Self {
        let path: OsString = f.path().unwrap().into();
        let len = path.len();
        return File {
            view: View::new(),
            path: path,
            id: f.id(),
            hunks: Vec::new(),
            max_line_len: len as i32,
        };
    }

    pub fn push_hunk(&mut self, h: Hunk) {
        if h.max_line_len > self.max_line_len {
            self.max_line_len = h.max_line_len;
        }
        self.hunks.push(h);
    }

    pub fn title(&self) -> String {
        self.path.to_str().unwrap().to_string()
    }
}

impl Default for File {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffKind {
    Staged,
    Unstaged,
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub files: Vec<File>,
    pub view: View,
    pub kind: DiffKind,
    pub max_line_len: i32,
}

impl Diff {
    pub fn new(kind: DiffKind) -> Self {
        Self {
            files: Vec::new(),
            view: View::new(),
            kind,
            max_line_len: 0,
        }
    }

    pub fn push_file(&mut self, f: File) {
        if f.max_line_len > self.max_line_len {
            self.max_line_len = f.max_line_len;
        }
        self.files.push(f);
    }

    // is it used???
    pub fn add(&mut self, other: Diff) {
        for file in other.files {
            self.files.push(file);
        }
    }
}

pub fn get_cwd_repo(sender: Sender<crate::Event>) -> Repository {
    let path_buff = env::current_exe().expect("cant't get exe path");
    let repo =
        get_current_repo(path_buff).expect("cant't get repo for current exe");
    let path = OsString::from(repo.path());
    sender
        .send_blocking(crate::Event::CurrentRepo(path.clone()))
        .expect("Could not send through channel");
    repo
}

#[derive(Debug, Clone)]
pub struct State {
    pub state: RepositoryState,
    pub view: View,
}

impl State {
    pub fn new(state: RepositoryState) -> Self {
        let mut view = View::new_markup();
        if state == RepositoryState::Clean {
            view.hidden = true;
        }
        Self { state, view }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Head {
    pub commit: String,
    pub branch: String,
    pub view: View,
    pub remote: bool,
}

impl Head {
    pub fn new(branch: &Branch, commit: &Commit) -> Self {
        Self {
            branch: String::from(branch.name().unwrap().unwrap()),
            commit: commit_string(commit),
            view: View::new_markup(),
            remote: false,
        }
    }
}

pub fn commit_string(c: &Commit) -> String {
    let message = c.message().unwrap_or("").replace('\n', "");
    let mut encoded = String::new();
    html_escape::encode_safe_to_string(&message, &mut encoded);
    format!("{} {}", &c.id().to_string()[..7], encoded)
}

pub fn commit_dt(c: &Commit) -> DateTime<FixedOffset> {
    let tz = FixedOffset::east_opt(c.time().offset_minutes() * 60).unwrap();
    match tz.timestamp_opt(c.time().seconds(), 0) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        _ => todo!("not implemented"),
    }
}

pub fn get_head(path: OsString, sender: Sender<crate::Event>) {
    let repo = Repository::open(path).expect("can't open repo");
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(ObjectType::Commit)
        .expect("can't get commit from ref!");
    let commit = ob.peel_to_commit().expect("can't get commit from ob!");
    let branch = Branch::wrap(head_ref);
    let new_head = Head::new(&branch, &commit);
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");
}

pub fn get_upstream(path: OsString, sender: Sender<crate::Event>) {
    trace!("get upstream");
    let repo = Repository::open(path).expect("can't open repo");
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let branch = Branch::wrap(head_ref);
    if let Ok(upstream) = branch.upstream() {
        let upstream_ref = upstream.get();
        let ob = upstream_ref
            .peel(ObjectType::Commit)
            .expect("can't get commit from ref!");
        let commit = ob.peel_to_commit().expect("can't get commit from ob!");
        let mut new_upstream = Head::new(&upstream, &commit);
        new_upstream.remote = true;
        sender
            .send_blocking(crate::Event::Upstream(Some(new_upstream)))
            .expect("Could not send through channel");
    } else {
        sender
            .send_blocking(crate::Event::Upstream(None))
            .expect("Could not send through channel");
        // todo!("some branches could contain only pushRemote, but no
        //       origin. There will be no upstream then. It need to lookup
        //       pushRemote in config and check refs/remotes/<origin>/")
    };
}

pub fn get_current_repo_status(
    current_path: Option<OsString>,
    sender: Sender<crate::Event>,
) {
    trace!("get_current_repo_status {:?}", current_path);
    let (repo, path) = {
        if let Some(path) = current_path {
            let repo =
                Repository::open(path.clone()).expect("can't open repo");
            (repo, path)
        } else {
            let repo = get_cwd_repo(sender.clone());
            let path = OsString::from(repo.path());
            (repo, path)
        }
    };

    sender
        .send_blocking(crate::Event::State(State::new(repo.state())))
        .expect("Could not send through channel");

    // get HEAD
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            get_head(path.clone(), sender.clone());
            get_upstream(path, sender);
        }
    });

    // get staged
    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            let repo = Repository::open(path).expect("can't open repo");
            let ob =
                repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree =
                repo.find_tree(ob.id()).expect("no working tree");
            let git_diff = repo
                .diff_tree_to_index(Some(&current_tree), None, None)
                .expect("can't get diff tree to index");
            let diff = make_diff(git_diff, DiffKind::Staged);
            sender
                .send_blocking(crate::Event::Staged(diff))
                .expect("Could not send through channel");
        }
    });
    // get unstaged
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant' get diff index to workdir");
    let diff = make_diff(git_diff, DiffKind::Unstaged);
    sender
        .send_blocking(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}

#[derive(Debug, Clone)]
pub enum ApplySubject {
    Stage,
    Unstage,
    Kill,
}

#[derive(Debug, Clone)]
pub struct ApplyFilter {
    pub file_path: String,
    pub hunk_header: Option<String>,
    pub subject: ApplySubject,
}

impl ApplyFilter {
    pub fn new(subject: ApplySubject) -> Self {
        Self {
            file_path: String::from(""),
            hunk_header: None,
            subject,
        }
    }
}

pub fn make_diff(git_diff: GitDiff, kind: DiffKind) -> Diff {
    let mut diff = Diff::new(kind);
    let mut current_file = File::new();
    let mut current_hunk = Hunk::new();
    let _res = git_diff.print(
        DiffFormat::Patch,
        |diff_delta, o_diff_hunk, diff_line| {
            // new_file - is workdir side
            // old_file - index side
            // oid of the file is used as uniq id
            // when file is Delta.Modified, there will be old
            // and new file in diff. we are interesetd in new
            // new_file, of course.
            // but when file is Delta.Deleted - there will be now new_file
            // and we will use old_file instead
            let status = diff_delta.status();
            let file: DiffFile = match status {
                Delta::Modified => diff_delta.new_file(),
                Delta::Deleted => diff_delta.old_file(),
                Delta::Added => match diff.kind {
                    DiffKind::Staged => diff_delta.new_file(),
                    DiffKind::Unstaged => {
                        todo!("delta added in unstaged {:?}", diff_delta)
                    }
                },
                _ => {
                    todo!(
                        "unhandled status ---> {:?} === {:?}, kind === {:?}",
                        status,
                        diff_delta,
                        diff.kind
                    )
                }
            };
            let oid = file.id();
            if oid.is_zero() {
                // this is case of deleted file
                todo!();
            }
            if file.path().is_none() {
                todo!();
            }
            // build up diff structure
            if current_file.id.is_zero() {
                // init new file
                current_file = File::from_diff_file(&file);
            }
            if current_file.id != oid {
                // go to next file
                // push current_hunk to file and init new empty hunk
                current_file.push_hunk(current_hunk.clone());
                current_hunk = Hunk::new();
                // push current_file to diff and change to new file
                diff.push_file(current_file.clone());
                current_file = File::from_diff_file(&file);
            }
            if let Some(diff_hunk) = o_diff_hunk {
                let hh = Hunk::get_header_from(&diff_hunk);
                // let old_start = diff_hunk.old_start();
                // let old_lines = diff_hunk.old_lines();
                // let new_start = diff_hunk.new_start();
                // let new_lines = diff_hunk.new_lines();
                if current_hunk.header.is_empty() {
                    // init hunk
                    current_hunk.fill_from(&diff_hunk)
                    // current_hunk.header = hh.clone();
                    // current_hunk.old_start = old_start;
                    // current_hunk.old_lines = old_lines;
                    // current_hunk.new_start = new_start;
                    // current_hunk.new_lines = new_lines;
                }
                if current_hunk.header != hh {
                    // go to next hunk
                    current_file.push_hunk(current_hunk.clone());
                    current_hunk = Hunk::new();
                    current_hunk.fill_from(&diff_hunk)
                    // current_hunk.header = hh.clone();
                    // current_hunk.old_start = old_start;
                    // current_hunk.old_lines = old_lines;
                    // current_hunk.new_start = new_start;
                    // current_hunk.new_lines = new_lines;
                }
                current_hunk.push_line(Line::from_diff_line(&diff_line));
            } else {
                // this is file header line.
                current_hunk.push_line(Line::from_diff_line(&diff_line))
            }

            true
        },
    );
    if !current_hunk.header.is_empty() {
        current_file.push_hunk(current_hunk);
    }
    if !current_file.path.is_empty() {
        diff.push_file(current_file);
    }
    diff
}

pub fn stage_via_apply(
    path: OsString,
    filter: ApplyFilter,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    // get actual diff for repo
    let git_diff = match filter.subject {
        // The index will be used for the “old_file” side of the delta,
        // and the working directory will be used
        // for the “new_file” side of the delta.
        ApplySubject::Stage => repo
            .diff_index_to_workdir(None, None)
            .expect("can't get diff"),
        // The tree you pass will be used for the “old_file”
        // side of the delta, and the index
        // will be used for the “new_file” side of the delta.
        // !!!!! SEE reverse below. Means tree will be new side
        // and index will be old side. Means changes from tree come to index!
        ApplySubject::Unstage => {
            let ob =
                repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree =
                repo.find_tree(ob.id()).expect("no working tree");
            repo.diff_tree_to_index(
                Some(&current_tree),
                None,
                Some(DiffOptions::new().reverse(true)), // reverse!!!
            )
            .expect("can't get diff")
        }
        // The tree you provide will be used for the “old_file”
        // side of the delta, and the working directory
        // will be used for the “new_file” side.
        // !!!!! SEE reverse below. Means tree will be new side
        // and workdir will be old side. Means changes from tree come to index!
        ApplySubject::Kill => {
            let ob =
                repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree =
                repo.find_tree(ob.id()).expect("no working tree");
            repo.diff_tree_to_workdir(
                Some(&current_tree),
                Some(DiffOptions::new().reverse(true)), // reverse!!!
            )
            .expect("can't get diff in kill")
        }
    };

    let mut options = ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if let Some(hunk_header) = &filter.hunk_header {
            if let Some(dh) = odh {
                let header = Hunk::get_header_from(&dh);
                return match filter.subject {
                    ApplySubject::Stage => hunk_header == &header,
                    ApplySubject::Unstage => {
                        hunk_header == &Hunk::reverse_header(header) // reverse!!!
                    }
                    ApplySubject::Kill => {
                        hunk_header == &Hunk::reverse_header(header) // reverse!!!
                    }
                };
            }
        }
        true
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let status = dd.status();
            trace!("delta_callback in stage_via_apply status {:?}", status);
            let new_file = dd.new_file();
            let file = File::from_diff_file(&new_file);
            let path = file.path.into_string().unwrap();
            return filter.file_path == path;
        }
        todo!("diff without delta");
    });
    let apply_location = match filter.subject {
        ApplySubject::Stage | ApplySubject::Unstage => ApplyLocation::Index,
        ApplySubject::Kill => ApplyLocation::WorkDir,
    };

    repo.apply(&git_diff, apply_location, Some(&mut options))
        .expect("can't apply patch");

    // staged changes. not needed in kill, btw.
    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            let repo = Repository::open(path).expect("can't open repo");
            let ob =
                repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree =
                repo.find_tree(ob.id()).expect("no working tree");
            let git_diff = repo
                .diff_tree_to_index(Some(&current_tree), None, None)
                .expect("can't get diff tree to index");
            let diff = make_diff(git_diff, DiffKind::Staged);
            sender
                .send_blocking(crate::Event::Staged(diff))
                .expect("Could not send through channel");
        }
    });
    // unstaged changes
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant get diff_index_to_workdir");
    let diff = make_diff(git_diff, DiffKind::Unstaged);
    sender
        .send_blocking(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}

pub fn commit(path: OsString, message: String, sender: Sender<crate::Event>) {
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
    repo.commit(Some("HEAD"), &me, &me, &message, &tree, &[&parent])
        .expect("can't commit");

    // update staged changes
    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_index(Some(&current_tree), None, None)
        .expect("can't get diff tree to index");
    sender
        .send_blocking(crate::Event::Staged(make_diff(
            git_diff,
            DiffKind::Staged,
        )))
        .expect("Could not send through channel");

    // get unstaged
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            let repo = Repository::open(path).expect("can't open repo");
            let git_diff = repo
                .diff_index_to_workdir(None, None)
                .expect("cant' get diff index to workdir");
            let diff = make_diff(git_diff, DiffKind::Unstaged);
            sender
                .send_blocking(crate::Event::Unstaged(diff))
                .expect("Could not send through channel");
        }
    });
    get_head(path, sender)
}

pub fn push(
    path: OsString,
    remote_branch: String,
    tracking_remote: bool,
    sender: Sender<crate::Event>,
) {
    trace!("remote branch {:?}", remote_branch);
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let head_ref = repo.head().expect("can't get head");
    trace!("push.head ref name {:?}", head_ref.name());
    assert!(head_ref.is_branch());
    let refspec = format!(
        "{}:refs/heads/{}",
        head_ref.name().unwrap(),
        remote_branch.replace("origin/", "")
    );
    trace!("push. refspec {}", refspec);
    let mut branch = Branch::wrap(head_ref);
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");

    let mut opts = PushOptions::new();
    let mut callbacks = RemoteCallbacks::new();
    callbacks.update_tips({
        move |updated_ref, oid1, oid2| {
            trace!(
                "updated local references {:?} {:?} {:?}",
                updated_ref,
                oid1,
                oid2
            );
            if tracking_remote {
                let res = branch.set_upstream(Some(&remote_branch));
                trace!("result on set upstream {:?}", res);
            }
            get_upstream(path.clone(), sender.clone());
            // todo what is this?
            true
        }
    });
    callbacks.push_update_reference({
        move |ref_name, opt_status| {
            trace!("push update ref {:?}", ref_name);
            trace!("push status {:?}", opt_status);
            // TODO - if status is not None
            // it will need to interact with user
            assert!(opt_status.is_none());
            Ok(())
        }
    });
    callbacks.credentials(|url, username_from_url, allowed_types| {
        trace!("auth credentials url {:?}", url);
        // "git@github.com:aganzha/stage.git"
        trace!("auth credentials username_from_url {:?}", username_from_url);
        trace!("auth credentials allowed_types url {:?}", allowed_types);
        if allowed_types.contains(CredentialType::SSH_KEY) {
            let result = Cred::ssh_key_from_agent(username_from_url.unwrap());
            trace!("got auth memory result. is it ok? {:?}", result.is_ok());
            return result;
        }
        todo!("implement other types");
    });
    callbacks.transfer_progress(|progress| {
        trace!("transfer progress {:?}", progress.received_bytes());
        true
    });
    callbacks.sideband_progress(|response| {
        trace!(
            "push.sideband progress {:?}",
            String::from_utf8_lossy(response)
        );
        true
    });
    callbacks.certificate_check(|_cert, error| {
        trace!("cert error? {:?}", error);
        Ok(CertificateCheckStatus::CertificateOk)
    });
    callbacks.push_update_reference(|re, op| {
        trace!("push_update_reference {:?} {:?}", re, op);
        Ok(())
    });
    callbacks.push_transfer_progress(|s1, s2, s3| {
        trace!("push_transfer_progress {:?} {:?} {:?}", s1, s2, s3);
    });
    callbacks.push_negotiation(|update| {
        trace!("push_negotiation {:?}", update.len());
        Ok(())
    });
    opts.remote_callbacks(callbacks);
    remote
        .push(&[refspec], Some(&mut opts))
        .expect("cant push to remote");
}

#[derive(Debug, Clone)]
pub struct BranchData {
    pub name: String,
    pub refname: String,
    pub branch_type: BranchType,
    pub oid: Oid,
    pub commit_string: String,
    pub is_head: bool,
    pub upstream_name: Option<String>,
    pub commit_dt: DateTime<FixedOffset>,
}

impl Default for BranchData {
    fn default() -> Self {
        BranchData {
            name: String::from(""),
            refname: String::from(""),
            branch_type: BranchType::Local,
            oid: Oid::zero(),
            commit_string: String::from(""),
            is_head: false,
            upstream_name: None,
            commit_dt: DateTime::<FixedOffset>::MIN_UTC.into(),
        }
    }
}

impl BranchData {
    pub fn new(branch: Branch, branch_type: BranchType) -> Self {
        let name = branch.name().unwrap().unwrap().to_string();
        let mut upstream_name: Option<String> = None;
        if let Ok(upstream) = branch.upstream() {
            upstream_name =
                Some(upstream.name().unwrap().unwrap().to_string());
        }
        let is_head = branch.is_head();
        let bref = branch.get();
        let refname = bref.name().unwrap().to_string();
        let ob = bref
            .peel(ObjectType::Commit)
            .expect("can't get commit from ref!");
        let commit = ob.peel_to_commit().expect("can't get commit from ob!");
        let commit_string = commit_string(&commit);
        let target = branch.get().target();
        let mut oid = Oid::zero();
        if let Some(t) = target {
            // this could be
            // name: "origin/HEAD" refname: "refs/remotes/origin/HEAD"
            oid = t;
        } else {
            trace!(
                "ZERO OID -----------------------------> {:?} {:?} {:?} {:?}",
                target,
                name,
                refname,
                ob.id()
            );
        }

        let commit_dt = commit_dt(&commit);
        BranchData {
            name,
            refname,
            branch_type,
            oid,
            commit_string,
            is_head,
            upstream_name,
            commit_dt,
        }
    }
}

pub fn get_refs(path: OsString) -> Vec<BranchData> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut result = Vec::new();
    let branches = repo.branches(None).expect("can't get branches");
    branches.for_each(|item| {
        let (branch, branch_type) = item.unwrap();
        let branch_data = BranchData::new(branch, branch_type);
        if branch_data.oid != Oid::zero() {
            result.push(branch_data);
        }
    });
    result.sort_by(|a, b| {
        // if a.is_head {
        //     return Ordering::Less;
        // }
        // if b.is_head {
        //     return Ordering::Greater;
        // }
        if a.branch_type == BranchType::Local
            && b.branch_type != BranchType::Local
        {
            return Ordering::Less;
        }
        if b.branch_type == BranchType::Local
            && a.branch_type != BranchType::Local
        {
            return Ordering::Greater;
        }
        b.commit_dt.cmp(&a.commit_dt)
    });
    result
}

// pub fn set_head(path: OsString, refname: &str) -> Result<(), String> {
//     trace!("set head.......{:?}", refname);
//     let repo = Repository::open(path.clone()).expect("can't open repo");
//     let result = repo.set_head(refname);
//     trace!("!======================> {:?}", result);
//     Ok(())
// }

fn git_checkout(
    // path: OsString,
    // branch_data: BranchData,
    repo: Repository,
    oid: Oid,
    refname: &str,
) -> Result<(), Error> {
    let mut builder = CheckoutBuilder::new();
    let opts = builder.safe();
    let commit = repo.find_commit(oid)?;
    repo.checkout_tree(commit.as_object(), Some(opts))?;
    repo.set_head(refname)?;
    Ok(())
}

pub fn checkout(
    path: OsString,
    oid: Oid,
    refname: &str,
    sender: Sender<crate::Event>,
) -> Result<(), String> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    if let Err(err) = git_checkout(repo, oid, refname) {
        trace!(
            "err on checkout {:?} {:?} {:?}",
            err.code(),
            err.class(),
            err.message()
        );
        // match err.code() {
        //     ErrorCode::Conflict => {
        //         return Err(String::from(""));
        //     }
        // }
        return Err(String::from(err.message()));
    }
    // todo -> message to update text_view
    get_current_repo_status(Some(path), sender);
    Ok(())
}

pub fn create_branch(
    path: OsString,
    new_branch_name: String,
    branch_data: BranchData,
    _sender: Sender<crate::Event>,
) -> Result<BranchData, String> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let commit = repo.find_commit(branch_data.oid).expect("cant find commit");
    let result = repo.branch(&new_branch_name, &commit, false);
    match result {
        Ok(branch) => Ok(BranchData::new(branch, BranchType::Local)),
        Err(error) => Err(String::from(error.message())),
    }
}

pub fn kill_branch(
    path: OsString,
    branch_data: BranchData,
    _sender: Sender<crate::Event>,
) -> Result<(), String> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let name = &branch_data.name;
    let kind = branch_data.branch_type;
    let mut branch = repo.find_branch(name, kind).expect("can't find branch");
    let result = branch.delete();
    if let Err(err) = result {
        trace!(
            "err on checkout {:?} {:?} {:?}",
            err.code(),
            err.class(),
            err.message()
        );
        // match err.code() {
        //     ErrorCode::Conflict => {
        //         return Err(String::from(""));
        //     }
        // }
        return Err(String::from(err.message()));
    }
    Ok(())
}

pub fn cherry_pick(
    path: OsString,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<BranchData, String> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let commit = repo.find_commit(branch_data.oid).expect("cant find commit");
    let result = repo.cherrypick(&commit, Some(&mut CherrypickOptions::new()));
    if let Err(err) = result {
        trace!(
            "err on checkout {:?} {:?} {:?}",
            err.code(),
            err.class(),
            err.message()
        );
        // match err.code() {
        //     ErrorCode::Conflict => {
        //         return Err(String::from(""));
        //     }
        // }
        return Err(String::from(err.message()));
    }
    debug!("cherry pick could not change the current branch, cause of merge conflict.
          So it need also update status.");
    let state = repo.state();
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(ObjectType::Commit)
        .expect("can't get commit from ref!");
    let commit = ob.peel_to_commit().expect("can't get commit from ob!");
    let branch = Branch::wrap(head_ref);
    let new_head = Head::new(&branch, &commit);
    sender
        .send_blocking(crate::Event::State(State::new(state)))
        .expect("Could not send through channel");
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");

    Ok(BranchData::new(branch, BranchType::Local))
}

pub fn merge(
    path: OsString,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<BranchData, String> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let annotated_commit = repo
        .find_annotated_commit(branch_data.oid)
        .expect("cant find commit");
    // let result = repo.merge(&[&annotated_commit], None, None);

    match repo.merge_analysis(&[&annotated_commit]) {
        Ok((analysis, _)) if analysis.is_up_to_date() => {
            info!("merge.uptodate");
        }

        Ok((analysis, preference))
            if analysis.is_fast_forward()
                && !preference.is_no_fast_forward() =>
        {
            debug!("-----------------------------------> {:?}", analysis);
            info!("merge.fastforward");
        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {
            debug!("-----------------------------------> {:?}", analysis);
            info!("merge.normal");
            let result = repo.merge(&[&annotated_commit], None, None);
            if let Err(err) = result {
                debug!(
                    "err on merge {:?} {:?} {:?}",
                    err.code(),
                    err.class(),
                    err.message()
                );
                return Err(String::from(err.message()));
            }
            // all changes are in index now
            let head_ref = repo.head().expect("can't get head");
            assert!(head_ref.is_branch());
            let current_branch = Branch::wrap(head_ref);
            let message = format!(
                "merge branch {} into {}",
                branch_data.name,
                current_branch.name().unwrap().unwrap().to_string()
            );
            commit(path, message, sender.clone());
            repo.cleanup_state().unwrap();
            // git_repository_state_cleanup(repo);
        }
        Ok((analysis, preference)) => {
            todo!("not implemented case {:?} {:?}", analysis, preference);
        }
        Err(err) => {
            panic!("error in merge_analysis {:?}", err.message());
        }
    }

    let state = repo.state();
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(ObjectType::Commit)
        .expect("can't get commit from ref!");
    let commit = ob.peel_to_commit().expect("can't get commit from ob!");
    let branch = Branch::wrap(head_ref);
    let new_head = Head::new(&branch, &commit);
    sender
        .send_blocking(crate::Event::State(State::new(state)))
        .expect("Could not send through channel");
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");

    // update staged changes
    // let tree_ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    // debug!("lets find treeeeeeeeeeeeeeeeeeee {:?}", tree_ob.id());
    // let current_tree = repo.find_tree(tree_ob.id()).expect("no working tree");
    // let git_diff = repo
    //     .diff_tree_to_index(Some(&current_tree), None, None)
    //     .expect("can't get diff tree to index");
    // sender
    //     .send_blocking(crate::Event::Staged(make_diff(
    //         git_diff,
    //         DiffKind::Staged,
    //     )))
    //     .expect("Could not send through channel");

    Ok(BranchData::new(branch, BranchType::Local))
}
