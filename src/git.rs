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
    DiffDelta, DiffFile, DiffFormat, DiffHunk, DiffLine, DiffLineType,
    DiffOptions, Error, FetchOptions, ObjectType, Oid, PushOptions,
    RemoteCallbacks, Repository, RepositoryState, ResetType,
    StashApplyOptions, StashFlags, AutotagOption, Direction
};
use log::{debug, info, trace};
use regex::Regex;
use std::cmp::Ordering;
//use std::time::SystemTime;
use std::{collections::HashSet, env, ffi, path, str};

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
    pub kind: DiffKind,
}

impl Hunk {
    pub fn new(kind: DiffKind) -> Self {
        Self {
            view: View::new(),
            header: String::new(),
            lines: Vec::new(),
            old_start: 0,
            new_start: 0,
            old_lines: 0,
            new_lines: 0,
            max_line_len: 0,
            kind: kind,
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

    pub fn title(&self) -> String {
        let parts: Vec<&str> = self.header.split("@@").collect();
        let line_no = match self.kind {
            DiffKind::Unstaged => self.old_start,
            DiffKind::Staged => self.new_start,
        };
        let scope = parts.get(parts.len() - 1).unwrap();
        if scope.len() > 0 {
            format!("Line {:} in{:}", line_no, scope)
        } else {
            format!("Line {:?}", line_no)
        }
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

#[derive(Debug, Clone)]
pub struct File {
    pub view: View,
    pub path: OsString,
    pub id: Oid,
    pub hunks: Vec<Hunk>,
    pub max_line_len: i32,
    pub kind: DiffKind,
}

impl File {
    pub fn new(kind: DiffKind) -> Self {
        Self {
            view: View::new(),
            path: OsString::new(),
            id: Oid::zero(),
            hunks: Vec::new(),
            max_line_len: 0,
            kind: kind,
        }
    }
    pub fn from_diff_file(f: &DiffFile, kind: DiffKind) -> Self {
        let path: OsString = f.path().unwrap().into();
        let len = path.len();
        return File {
            view: View::new(),
            path: path,
            id: f.id(),
            hunks: Vec::new(),
            max_line_len: len as i32,
            kind: kind,
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

    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
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
    // get stashes
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            get_stashes(path, sender);
        }
    });

    // get untracked
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            get_untracked(path, sender);
        }
    });

    // get unstaged
    // TODO! throttle monitor
    // Error { code: -1, klass: 2, message: "error reading file for hashing: " }
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant' get diff index to workdir");
    let diff = make_diff(git_diff, DiffKind::Unstaged);
    sender
        .send_blocking(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");

    // get untracked
    // TODO! put in separate thread (previous clause)
}

pub fn get_untracked(path: OsString, sender: Sender<crate::Event>) {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    let mut opts = DiffOptions::new();

    let opts = opts.show_untracked_content(true);

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_workdir_with_index(Some(&current_tree), Some(opts))
        .expect("can't get diff");
    let mut untracked = Untracked::new();
    git_diff.foreach(
        &mut |delta: DiffDelta, _num| {
            if delta.status() == Delta::Untracked {
                // debug!(":--------------------> {:?} {:?}", delta.status(), delta.new_file().path());
                let path: OsString = delta.new_file().path().unwrap().into();
                untracked.push_file(path);
            }
            true
        },
        None,
        None,
        None,
    );
    sender
        .send_blocking(crate::Event::Untracked(untracked))
        .expect("Could not send through channel");
}

#[derive(Debug, Clone)]
pub struct UntrackedFile {
    pub path: OsString,
    pub view: View,
}

impl UntrackedFile {
    pub fn new() -> Self {
        Self {
            path: OsString::new(),
            view: View::new(),
        }
    }
    pub fn from_path(path: OsString) -> Self {
        Self {
            path: path,
            view: View::new(),
        }
    }

    pub fn title(&self) -> String {
        self.path.to_str().unwrap().to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Untracked {
    pub files: Vec<UntrackedFile>,
    pub view: View,
    pub max_line_len: i32,
}

impl Untracked {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            view: View::new(),
            max_line_len: 0,
        }
    }
    pub fn push_file(&mut self, path: OsString) {
        if path.len() as i32 > self.max_line_len {
            self.max_line_len = path.len() as i32;
        }
        let file = UntrackedFile::from_path(path);
        self.files.push(file);
    }
}

#[derive(Debug, Clone)]
pub enum ApplySubject {
    Stage,
    Unstage,
    Kill,
}

#[derive(Debug, Clone)]
pub struct ApplyFilter {
    pub file_id: String,
    pub hunk_id: Option<String>,
    pub subject: ApplySubject,
}

impl ApplyFilter {
    pub fn new(subject: ApplySubject) -> Self {
        Self {
            file_id: String::from(""),
            hunk_id: None,
            subject,
        }
    }
}

pub fn make_diff(git_diff: GitDiff, kind: DiffKind) -> Diff {
    let mut diff = Diff::new(kind.clone());
    let mut current_file = File::new(kind.clone());
    let mut current_hunk = Hunk::new(kind.clone());
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
                current_file = File::from_diff_file(&file, kind.clone());
            }
            if current_file.id != oid {
                // go to next file
                // push current_hunk to file and init new empty hunk
                current_file.push_hunk(current_hunk.clone());
                current_hunk = Hunk::new(kind.clone());
                // push current_file to diff and change to new file
                diff.push_file(current_file.clone());
                current_file = File::from_diff_file(&file, kind.clone());
            }
            if let Some(diff_hunk) = o_diff_hunk {
                let hh = Hunk::get_header_from(&diff_hunk);
                if current_hunk.header.is_empty() {
                    // init hunk
                    current_hunk.fill_from(&diff_hunk)
                }
                if current_hunk.header != hh {
                    // go to next hunk
                    current_file.push_hunk(current_hunk.clone());
                    current_hunk = Hunk::new(kind.clone());
                    current_hunk.fill_from(&diff_hunk)
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

pub fn stage_untracked(
    path: OsString,
    file: UntrackedFile,
    sender: Sender<crate::Event>,
) {
    trace!("stage untracked! {:?}", file.path);
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let pth = path::Path::new(&file.path);
    index.add_path(pth).expect("cant add path");
    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender);
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
        if let Some(hunk_header) = &filter.hunk_id {
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
            // let status = dd.status();
            // trace!("delta_callback in stage_via_apply status {:?}", status);
            // let new_file = dd.new_file();
            // let file = File::from_diff_file(&new_file, kind);
            // let path = file.path.into_string().unwrap();
            let path: OsString = dd.new_file().path().unwrap().into();
            return filter.file_id == path.into_string().unwrap();
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
        let path = path.clone();
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

    // get untracked
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            get_untracked(path, sender);
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

pub fn pull(
    path: OsString,
    sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>
) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");
    let head_ref = repo.head().expect("can't get head");

    let mut opts = FetchOptions::new();
    let mut callbacks = RemoteCallbacks::new();

    callbacks.update_tips({
        let path = path.clone();
        let sender = sender.clone();
        move |updated_ref, oid1, oid2| {
            debug!(
                "updated local references {:?} {:?} {:?}",
                updated_ref, oid1, oid2
            );
            sender
                .send_blocking(crate::Event::Toast(String::from(updated_ref)))
                .expect("cant send through channel");
            get_upstream(path.clone(), sender.clone());
            // todo what is this?
            true
        }
    });

    set_remote_callbacks(&mut callbacks, &user_pass);
    opts.remote_callbacks(callbacks);

    remote
        .fetch(&[head_ref.name().unwrap()], Some(&mut opts), None)
        .expect("cant fetch");

    // but checkout must be after fetch!!!
    assert!(head_ref.is_branch());
    let mut branch = Branch::wrap(head_ref);
    let upstream = branch.upstream().unwrap();
    // repo.set_head(upstream.get().name().unwrap()).expect("cant set head");
    let u_oid = upstream.get().target().unwrap();
    let mut head_ref = repo.head().expect("can't get head");
    let log_message = format!(
        "(HEAD -> {}, {}) HEAD@{0}: pull: Fast-forward",
        branch.name().unwrap().unwrap(),
        upstream.name().unwrap().unwrap()
    );

    let mut builder = CheckoutBuilder::new();
    let opts = builder.safe();
    let commit = repo.find_commit(u_oid).expect("can't find commit");
    repo.checkout_tree(commit.as_object(), Some(opts))
        .expect("can't checkout tree");
    head_ref
        .set_target(u_oid, &log_message)
        .expect("cant set target");
    get_head(path.clone(), sender.clone());
}

const PLAIN_PASSWORD: &str = "plain text password required";

pub fn set_remote_callbacks(callbacks: &mut RemoteCallbacks, user_pass: &Option<(String, String)>) {

    // const PLAIN_PASSWORD: &str = "plain text password required";
    callbacks.credentials({
        let user_pass = user_pass.clone();
        move |url, username_from_url, allowed_types| {
            debug!("auth credentials url {:?}", url);
            // "git@github.com:aganzha/stage.git"
            debug!("auth credentials username_from_url {:?}", username_from_url);
            debug!("auth credentials allowed_types {:?}", allowed_types);
            if allowed_types.contains(CredentialType::SSH_KEY) {
                let result = Cred::ssh_key_from_agent(username_from_url.unwrap());
                debug!("got auth memory result. is it ok? {:?}", result.is_ok());
                return result;
            }
            if allowed_types == CredentialType::USER_PASS_PLAINTEXT {
                if let Some((user_name, password)) = &user_pass {
                    return Cred::userpass_plaintext(&user_name, &password);
                }
                return Err(Error::from_str(PLAIN_PASSWORD));
            }
            todo!("implement other types");
        }
    });

    callbacks.push_transfer_progress(|s1, s2, s3| {
        debug!("push_transfer_progress {:?} {:?} {:?}", s1, s2, s3);
    });

    callbacks.transfer_progress(|progress| {
        debug!("transfer progress {:?}", progress.received_bytes());
        true
    });

    callbacks.pack_progress(|stage, s1, s2| {
        debug!("pack progress {:?} {:?} {:?}", stage, s1, s2);
    });

    callbacks.sideband_progress(|response| {
        debug!(
            "push.sideband progress {:?}",
            String::from_utf8_lossy(response)
        );
        true
    });

    callbacks.push_update_reference({
        move |ref_name, opt_status| {
            debug!("push update ref {:?}", ref_name);
            debug!("push status {:?}", opt_status);
            // TODO - if status is not None
            // it will need to interact with user
            assert!(opt_status.is_none());
            Ok(())
        }
    });

    callbacks.certificate_check(|_cert, error| {
        debug!("cert error? {:?}", error);
        Ok(CertificateCheckStatus::CertificateOk)
    });

    callbacks.push_negotiation(|update| {
        if !update.is_empty() {
            debug!(
                "push_negotiation {:?} {:?}",
                update[0].src_refname(),
                update[0].dst_refname()
            );
        }
        Ok(())
    });

}


pub fn push(
    path: OsString,
    remote_branch: String,
    tracking_remote: bool,
    sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>
) {
    debug!("remote branch {:?}", remote_branch);
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let head_ref = repo.head().expect("can't get head");
    debug!("push.head ref name {:?}", head_ref.name());
    assert!(head_ref.is_branch());
    let refspec = format!(
        "{}:refs/heads/{}",
        head_ref.name().unwrap(),
        remote_branch.replace("origin/", "")
    );
    debug!("push. refspec {}", refspec);
    let mut branch = Branch::wrap(head_ref);
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");

    let mut opts = PushOptions::new();
    let mut callbacks = RemoteCallbacks::new();

    callbacks.update_tips({
        let remote_branch = remote_branch.clone();
        let sender = sender.clone();
        move |updated_ref, oid1, oid2| {
            debug!(
                "updated local references {:?} {:?} {:?}",
                updated_ref, oid1, oid2
            );
            if tracking_remote {
                branch
                    .set_upstream(Some(&remote_branch))
                    .expect("cant set upstream");
            }
            sender
                .send_blocking(crate::Event::Toast(String::from(updated_ref)))
                .expect("cant send through channel");
            get_upstream(path.clone(), sender.clone());
            // todo what is this?
            true
        }
    });

    set_remote_callbacks(&mut callbacks, &user_pass);
    opts.remote_callbacks(callbacks);

    match remote.push(&[refspec], Some(&mut opts)) {
        Ok(_) => {}
        Err(error) if error.message() == PLAIN_PASSWORD => {
            sender.send_blocking(
                crate::Event::PushUserPass(remote_branch, tracking_remote)
            ).expect("cant send through channel");

        }
        Err(error) => {
            panic!("{}", error);
        }
    }
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
    pub fn from_branch(branch: Branch, branch_type: BranchType) -> Result<Self, Error> {
        let name = branch.name().unwrap().unwrap().to_string();
        let mut upstream_name: Option<String> = None;
        if let Ok(upstream) = branch.upstream() {
            upstream_name =
                Some(upstream.name().unwrap().unwrap().to_string());
        }
        let is_head = branch.is_head();
        let bref = branch.get();
        // can't get commit from ref!: Error { code: -3, klass: 3, message: "the reference 'refs/remotes/origin/HEAD' cannot be peeled - Cannot resolve reference" }
        let refname = bref.name().unwrap().to_string();
        let ob = bref
            .peel(ObjectType::Commit)?;
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
        Ok(BranchData {
            name,
            refname,
            branch_type,
            oid,
            commit_string,
            is_head,
            upstream_name,
            commit_dt,
        })
    }

    pub fn local_name(&self) -> String {
        self.name.replace("origin/", "")
    }
    pub fn remote_name(&self) -> String {
        format!("origin/{}", self.name.replace("origin/", ""))
    }
}

pub fn get_branches(path: OsString) -> Vec<BranchData> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut result = Vec::new();
    let branches = repo.branches(None).expect("can't get branches");
    branches.for_each(|item| {
        let (branch, branch_type) = item.unwrap();
        if let Ok(branch_data) = BranchData::from_branch(branch, branch_type) {
            if branch_data.oid != Oid::zero() {
                result.push(branch_data);
            }
        }
    });
    result.sort_by(|a, b| {
        // let head be always on top
        if a.is_head {
            return Ordering::Less;
        }
        if b.is_head {
            return Ordering::Greater;
        }

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

pub fn checkout(
    path: OsString,
    mut branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> BranchData {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut builder = CheckoutBuilder::new();
    let opts = builder.safe();
    let commit = repo
        .find_commit(branch_data.oid)
        .expect("can't find commit");
    repo.checkout_tree(commit.as_object(), Some(opts))
        .expect("can't checkout tree");
    match branch_data.branch_type {
        BranchType::Local => {}
        BranchType::Remote => {
            let created =
                repo.branch(&branch_data.local_name(), &commit, false);
            let mut branch = match created {
                Ok(branch) => branch,
                Err(_) => repo.find_branch(
                    &branch_data.local_name(),
                    BranchType::Local
                ).expect("branch was not created and not found among local branches")
            };
            branch
                .set_upstream(Some(&branch_data.remote_name()))
                .expect("cant set upstream");
            branch_data = BranchData::from_branch(branch, BranchType::Local).expect("cant get branch");
        }
    }
    repo.set_head(&branch_data.refname).expect("can't set head");
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    branch_data.is_head = true;
    branch_data
}

pub fn create_branch(
    path: OsString,
    new_branch_name: String,
    need_checkout: bool,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> BranchData {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let commit = repo.find_commit(branch_data.oid).expect("cant find commit");
    let branch = repo
        .branch(&new_branch_name, &commit, false)
        .expect("cant create branch");
    let branch_data = BranchData::from_branch(branch, BranchType::Local).expect("cant get branch");
    if need_checkout {
        checkout(path, branch_data, sender)
    } else {
        branch_data
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

    Ok(BranchData::from_branch(branch, BranchType::Local).expect("cant get branch"))
}

pub fn merge(
    path: OsString,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> BranchData {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let annotated_commit = repo
        .find_annotated_commit(branch_data.oid)
        .expect("cant find commit");
    // let result = repo.merge(&[&annotated_commit], None, None);

    let do_merge = || {
        let result = repo
            .merge(&[&annotated_commit], None, None)
            .expect("cant merge");
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
    };

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
            do_merge();
        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {
            debug!("-----------------------------------> {:?}", analysis);
            info!("merge.normal");
            do_merge();
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

    BranchData::from_branch(branch, BranchType::Local).expect("cant get branch")
}

#[derive(Debug, Clone)]
pub struct StashData {
    pub num: usize,
    pub title: String,
    pub oid: Oid,
}

impl StashData {
    pub fn new(num: usize, oid: Oid, title: String) -> Self {
        Self { num, oid, title }
    }
}

impl Default for StashData {
    fn default() -> Self {
        Self {
            oid: Oid::zero(),
            title: String::from(""),
            num: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Stashes {
    pub stashes: Vec<StashData>,
}
impl Stashes {
    pub fn new(stashes: Vec<StashData>) -> Self {
        Self { stashes }
    }
}

pub fn get_stashes(path: OsString, sender: Sender<crate::Event>) -> Stashes {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    let mut result = Vec::new();
    repo.stash_foreach(|num, title, oid| {
        result.push(StashData::new(num, oid.clone(), title.to_string()));
        true
    })
    .expect("cant get stash");
    let stashes = Stashes::new(result);
    sender
        .send_blocking(crate::Event::Stashes(stashes.clone()))
        .expect("Could not send through channel");
    stashes
}

pub fn stash_changes(
    path: OsString,
    stash_message: String,
    stash_staged: bool,
    sender: Sender<crate::Event>,
) -> Stashes {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");
    let flags = if stash_staged {
        StashFlags::empty()
    } else {
        StashFlags::KEEP_INDEX
    };
    let oid = repo
        .stash_save(&me, &stash_message, Some(flags))
        .expect("cant stash");
    gio::spawn_blocking({
        let path = path.clone();
        let sender = sender.clone();
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    get_stashes(path, sender)
}

pub fn apply_stash(
    path: OsString,
    stash_data: StashData,
    sender: Sender<crate::Event>,
) {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    // let opts = StashApplyOptions::new();
    repo.stash_apply(stash_data.num, None)
        .expect("cant apply stash");
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
}

pub fn drop_stash(
    path: OsString,
    stash_data: StashData,
    sender: Sender<crate::Event>,
) -> Stashes {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    repo.stash_drop(stash_data.num).expect("cant drop stash");
    get_stashes(path, sender)
}

pub fn reset_hard(path: OsString, sender: Sender<crate::Event>) {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(ObjectType::Commit)
        .expect("can't get commit from ref!");
    repo.reset(&ob, ResetType::Hard, None)
        .expect("cant reset hard");
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
}

pub fn get_directories(path: OsString) -> HashSet<String> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let index = repo.index().expect("cant get index");
    let mut directories = HashSet::new();
    for entry in index.iter() {
        let pth = String::from_utf8_lossy(&entry.path);
        let mut parts: Vec<&str> = pth.split("/").collect();
        trace!("entry in index {:?}", parts);
        if parts.len() > 0 {
            parts.pop();
        }
        directories.insert(parts.join("/"));
    }
    directories
}

pub fn track_changes(
    path: OsString,
    file_path: OsString,
    sender: Sender<crate::Event>,
) {
    // TODO throttle!
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let index = repo.index().expect("cant get index");
    let file_path = file_path.into_string().expect("wrong path");
    for entry in index.iter() {
        let entry_path = format!("{}", String::from_utf8_lossy(&entry.path));
        if file_path.ends_with(&entry_path) {
            trace!("got modifeied file {:?}", file_path);
            let git_diff = repo
                .diff_index_to_workdir(None, None)
                .expect("cant' get diff index to workdir");
            let diff = make_diff(git_diff, DiffKind::Unstaged);
            sender
                .send_blocking(crate::Event::Unstaged(diff))
                .expect("Could not send through channel");
            break;
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommitDiff {
    pub oid: Oid,
    pub commit_string: String,
    pub commit_dt: DateTime<FixedOffset>,
    pub diff: Diff,
}

impl Default for CommitDiff {
    fn default() -> Self {
        CommitDiff {
            oid: Oid::zero(),
            commit_string: String::from(""),
            commit_dt: DateTime::<FixedOffset>::MIN_UTC.into(),
            diff: Diff::new(DiffKind::Unstaged)
        }
    }
}

impl CommitDiff {
    pub fn new(commit: Commit, diff: Diff) -> Self {
        CommitDiff {
            oid: commit.id(),
            commit_string: commit_string(&commit),
            commit_dt: commit_dt(&commit),
            diff: diff
        }
    }
}

pub fn get_commit_diff(
    path: OsString,
    oid: Oid,
    sender: Sender<crate::Event>
) {
    let repo = Repository::open(path).expect("can't open repo");
    let commit = repo.find_commit(oid).expect("cant find commit");
    let tree = commit.tree().expect("no get tree from commit");
    let parent = commit.parent(0).expect("cant get commit parent");

    let parent_tree = parent.tree().expect("no get tree from PARENT commit");
    let git_diff = repo
        .diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)
        .expect("can't get diff tree to index");
    let commit_diff = CommitDiff::new(commit, make_diff(git_diff, DiffKind::Unstaged));
    sender.send_blocking(crate::Event::CommitDiff(commit_diff))
        .expect("Could not send through channel");
}


pub fn update_remote(path: OsString,  _sender: Sender<crate::Event>, user_pass: Option<(String, String)>) -> Result<(),()> {
    let repo = Repository::open(path).expect("can't open repo");
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");

    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    remote.connect_auth(Direction::Fetch, Some(callbacks), None).expect("cant connect");
    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    remote.prune(Some(callbacks)).expect("cant prune");

    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    callbacks.update_tips({
        move |updated_ref, oid1, oid2| {
            debug!(
                "updat tips {:?} {:?} {:?}",
                updated_ref, oid1, oid2
            );
            true
        }
    });

    let mut opts = FetchOptions::new();
    opts.remote_callbacks(callbacks);
    let refs: [String; 0]= [];
    remote.fetch(&refs, Some(&mut opts), None).expect("cant fetch");
    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);
    remote.update_tips(Some(&mut callbacks), true, AutotagOption::Auto, None).expect("cant update");

    Ok(())
}
