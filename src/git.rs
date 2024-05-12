pub mod merge;

use crate::gio;
// use crate::glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;

use chrono::{DateTime, FixedOffset, LocalResult, TimeZone};
use ffi::OsString;
use git2::build::CheckoutBuilder;
use git2::{
    ApplyLocation, ApplyOptions, AutotagOption, Branch, BranchType,
    CertificateCheckStatus, CherrypickOptions, Commit, Cred, CredentialType,
    Delta, Diff as GitDiff, DiffDelta, DiffFile, DiffFormat, DiffHunk,
    DiffLine, DiffLineType, DiffOptions, Direction, Error, ErrorClass,
    ErrorCode, FetchOptions, ObjectType, Oid, PushOptions, RemoteCallbacks,
    Repository, RepositoryState, ResetType, StashFlags, MergeOptions,
    IndexEntry, IndexEntryFlag, IndexConflict, IndexTime
};

// use libgit2_sys;
use log::{debug, info, trace};
use regex::Regex;
use std::cmp::Ordering;
//use std::time::SystemTime;
use std::{collections::HashSet, env, ffi, path, str};


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
}

#[derive(Debug, Clone)]
pub enum LineKind {
    None,
    Ours,
    Theirs,
    ConflictMarker(String)
}

#[derive(Debug, Clone)]
pub struct Line {
    pub view: View,
    pub origin: DiffLineType,
    pub content: String,
    pub new_line_no: Option<u32>,
    pub old_line_no: Option<u32>,
    pub kind: LineKind
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
            kind: LineKind::None
        };
    }
    pub fn hash(&self) -> String {
        // IT IS NOT ENOUGH! will be "Context" for
        // empty grey line!
        format!("{}{:?}", self.content, self.origin)
    }
}

pub const MARKER_OURS: &str = "<<<<<<<";
pub const MARKER_VS: &str = "=======";
pub const MARKER_THEIRS: &str = ">>>>>>>";

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
    pub has_conflicts: bool
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
            has_conflicts: false
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
            DiffKind::Unstaged | DiffKind::Conflicted => self.old_start,
            DiffKind::Staged => self.new_start,
        };
        let scope = parts.last().unwrap();
        if !scope.is_empty() {
            format!("Line {:} in{:}", line_no, scope)
        } else {
            format!("Line {:?}", line_no)
        }
    }

    pub fn push_line(&mut self, mut line: Line, prev_line_kind: LineKind) -> LineKind {
        if self.kind != DiffKind::Conflicted {
            match line.origin {
                DiffLineType::FileHeader
                    | DiffLineType::HunkHeader
                    | DiffLineType::Binary => {}
                _ => {
                    self.handle_max(&line.content);
                    self.lines.push(line)
                }
            }
            return LineKind::None;
        }
        trace!(":::::::::::::::::::::::::::::::: {:?}. prev line kind {:?}", line.content, prev_line_kind);
        if line.content.len() >= 7 {
            match &line.content[..7] {
                MARKER_OURS | MARKER_THEIRS | MARKER_VS => {
                    self.has_conflicts = true;
                    line.kind = LineKind::ConflictMarker(String::from(&line.content[..7]));
                }
                _ => {}
            }
        }

        let marker_ours = String::from(MARKER_OURS);
        let marker_vs = String::from(MARKER_VS);
        let marker_theirs = String::from(MARKER_THEIRS);

        match (prev_line_kind, &line.kind) {
            (LineKind::ConflictMarker(marker), LineKind::None) if marker == marker_ours => {
                trace!("sec match. ours after ours MARKER ??????????? {:?}", marker_ours);
                line.kind = LineKind::Ours
            }
            (LineKind::Ours, LineKind::None) => {
                trace!("sec match. ours after ours LINE");
                line.kind = LineKind::Ours
            }
            (LineKind::ConflictMarker(marker), LineKind::None) if marker == marker_vs => {
                trace!("sec match. theirs after vs MARKER");
                line.kind = LineKind::Theirs
            }
            (LineKind::Theirs, LineKind::None) => {
                trace!("sec match. theirs after theirs LINE");
                line.kind = LineKind::Theirs
            }
            (LineKind::None, LineKind::None) => {
                trace!("sec match. contenxt????")
            }
            (prev, LineKind::ConflictMarker(m)) => {
                trace!("sec match. pass this marker {:?}", m);
            }
            (LineKind::ConflictMarker(marker), LineKind::None) if marker == marker_theirs => {
                trace!("sec match. finish prev their marker {:?}", marker);
            }
            (prev, this) => {
                panic!("whats the case in markers? {:?} {:?}", prev, this)
            }
        }
        let this_kind = line.kind.clone();
        match line.origin {
            DiffLineType::FileHeader
            | DiffLineType::HunkHeader
            | DiffLineType::Binary => {}
            _ => {
                self.handle_max(&line.content);
                self.lines.push(line)
            }
        }
        trace!("........return this_kind {:?}", this_kind);
        trace!("");
        this_kind
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub view: View,
    pub path: OsString,
    // pub id: Oid,
    pub hunks: Vec<Hunk>,
    pub max_line_len: i32,
    pub kind: DiffKind,
}

impl File {
    pub fn new(kind: DiffKind) -> Self {
        Self {
            view: View::new(),
            path: OsString::new(),
            // id: Oid::zero(),
            hunks: Vec::new(),
            max_line_len: 0,
            kind,
        }
    }
    pub fn from_diff_file(f: &DiffFile, kind: DiffKind) -> Self {
        let path: OsString = f.path().unwrap().into();
        let len = path.len();
        File {
            view: View::new(),
            path,
            // id: f.id(),
            hunks: Vec::new(),
            max_line_len: len as i32,
            kind,
        }
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
    Conflicted
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

    pub fn is_empty(&self) -> bool {
        return self.files.is_empty()
    }
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
            view.squashed = true;
        }
        Self { state, view }
    }
    pub fn is_merging(&self) -> bool {
        return self.state == RepositoryState::Merge
    }
}

#[derive(Debug, Clone)]
pub struct Head {
    pub oid: Oid,
    pub commit: String,
    pub branch: String,
    pub view: View,
    pub remote: bool,
}

impl Head {
    pub fn new(branch: &Branch, commit: &Commit) -> Self {
        Self {
            oid: commit.id(),
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
    // path could came from command args or from choosing path
    // by user
    let path = {
        if current_path.is_some() {
            current_path.unwrap()
        } else {
            OsString::from(
                env::current_exe().expect("cant't get exe path").as_path(),
            )
        }
    };
    let repo = Repository::discover(path.clone()).expect("can't open repo");
    let path = OsString::from(repo.path());
    sender
        .send_blocking(crate::Event::CurrentRepo(path.clone()))
        .expect("Could not send through channel");

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
        // get staged
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
            let diff = make_diff(&git_diff, DiffKind::Staged);
            sender
                .send_blocking(crate::Event::Staged(diff))
                .expect("Could not send through channel");
        }
    });
    // TODO! not need to call stashes every time when status is required!
    // call it just once on app start!
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


    let index = repo.index().expect("cant get index");
    if index.has_conflicts() {
        // https://github.com/libgit2/libgit2/issues/6232
        // this one is for staging killed hunk
        // https://github.com/libgit2/libgit2/issues/6643
        gio::spawn_blocking({
            let sender = sender.clone();
            let path = path.clone();
            move || {
                get_conflicted_v1(path, sender);
            }
        });
    } else {
        sender
            .send_blocking(crate::Event::Conflicted(Diff::new(DiffKind::Conflicted)))
            .expect("Could not send through channel");
    }

    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant' get diff index to workdir");
    let diff = make_diff(&git_diff, DiffKind::Unstaged);
    sender
        .send_blocking(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");

}

pub fn get_conflicted_v1(path: OsString, sender: Sender<crate::Event>) {
    // so, when file is in conflictduring merge, this means nothing
    // was staged to that file, cause mergeing in such state is PROHIBITED!
    // sure? Yes, it must be true!
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut opts = DiffOptions::new();
    for conflict in conflicts {
        let conflict = conflict.unwrap();
        let our = conflict.our.unwrap();
        let our_path = String::from_utf8(our.path).unwrap();
        opts.pathspec(our_path);
    }
    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo.diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
        .expect("cant get diff");
    let diff = make_diff(&git_diff, DiffKind::Conflicted);
    sender
        .send_blocking(crate::Event::Conflicted(diff))
        .expect("Could not send through channel");

}

pub fn get_untracked(path: OsString, sender: Sender<crate::Event>) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut opts = DiffOptions::new();

    let opts = opts.show_untracked_content(true);

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_workdir_with_index(Some(&current_tree), Some(opts))
        .expect("can't get diff");
    let mut untracked = Untracked::new();
    let _ = git_diff.foreach(
        &mut |delta: DiffDelta, _num| {
            if delta.status() == Delta::Untracked {
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

impl Default for UntrackedFile {
    fn default() -> Self {
        Self::new()
    }
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
            path,
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

impl Default for Untracked {
    fn default() -> Self {
        Self::new()
    }
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


pub fn make_diff(git_diff: &GitDiff, kind: DiffKind) -> Diff {
    let mut diff = Diff::new(kind.clone());
    let mut current_file = File::new(kind.clone());
    let mut current_hunk = Hunk::new(kind.clone());
    let mut prev_line_kind = LineKind::None;

    let _res = git_diff.print(
        DiffFormat::Patch,
        |diff_delta, o_diff_hunk, diff_line| {
            let status = diff_delta.status();
            if status == Delta::Conflicted {
                if kind == DiffKind::Staged || kind == DiffKind::Unstaged {
                    return true;
                }
            }
            let file: DiffFile = match status {
                Delta::Modified | Delta::Conflicted => diff_delta.new_file(),
                Delta::Deleted => diff_delta.old_file(),
                Delta::Added => match diff.kind {
                    DiffKind::Staged => diff_delta.new_file(),
                    DiffKind::Unstaged => {
                        todo!("delta added in unstaged {:?}", diff_delta)
                    },
                    DiffKind::Conflicted => {
                        todo!("delta added in conflicted {:?}", diff_delta)
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

            if file.path().is_none() {
                todo!();
            }
            // build up diff structure
            if current_file.path.is_empty() {
                // init new file
                current_file = File::from_diff_file(&file, kind.clone());
            }
            if current_file.path != file.path().unwrap() {
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
                    prev_line_kind = LineKind::None;
                    current_hunk.fill_from(&diff_hunk)
                }
                if current_hunk.header != hh {
                    // go to next hunk
                    prev_line_kind = LineKind::None;
                    current_file.push_hunk(current_hunk.clone());
                    current_hunk = Hunk::new(kind.clone());
                    current_hunk.fill_from(&diff_hunk)
                }
                prev_line_kind = current_hunk.push_line(Line::from_diff_line(&diff_line), prev_line_kind.clone());
            } else {
                // this is file header line.
                let line = Line::from_diff_line(&diff_line);
                prev_line_kind = current_hunk.push_line(line, prev_line_kind.clone())
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
    if pth.is_file() {
        index.add_path(pth).expect("cant add path");
    }
    else if pth.is_dir() {
        index.add_all([pth], git2::IndexAddOption::DEFAULT, None).expect("cant add path");
    }
    else {
        panic!("unknown path {:?}", pth);
    }
    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender);
}

pub fn stage_via_apply(
    path: OsString,
    filter: ApplyFilter,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone()).expect("can't open repo");

    let git_diff = match filter.subject {
        // The index will be used for the “old_file” side of the delta,
        // and the working directory will be used
        // for the “new_file” side of the delta.
        ApplySubject::Stage => repo
            .diff_index_to_workdir(None, None)
            .expect("can't get diff"),
        // The tree you pass will be used for the “old_file”
        // side of the delta, and the index???
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
        ApplySubject::Kill => {
            // diff_index_to_workdir with reverse does not work: it is empty :(
            // if index is empty and workdir changed - straight index (reverse=false)
            // shows unstaged hunk. BUT if reverse - it does show NOTHING for some reason.
            // why????
            let mut opts = DiffOptions::new();
            opts.reverse(true);
            // allow empty chunks!
            opts.include_unmodified(true);
            repo.diff_index_to_workdir(None, Some(&mut opts))
            // reverse doesn work either, it is empty!
            // let ob =
            //     repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            // let current_tree =
            //     repo.find_tree(ob.id()).expect("no working tree");
            // problem here: this diff is incorrect, when stage part of file
            // and want to kill another part. hunks headers are different!
            // repo.diff_tree_to_workdir(
            //     Some(&current_tree),
            //     Some(DiffOptions::new().reverse(true)), // reverse!!!
            // )
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
                        hunk_header == &Hunk::reverse_header(header)
                    }
                    ApplySubject::Kill => {
                        let reversed = Hunk::reverse_header(header);
                        hunk_header == &reversed
                    }
                };
            }
        }
        true
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: OsString = dd.new_file().path().unwrap().into();
            return filter.file_id == path.into_string().unwrap();
        }
        todo!("diff without delta");
    });
    let apply_location = match filter.subject {
        ApplySubject::Stage | ApplySubject::Unstage => ApplyLocation::Index,
        ApplySubject::Kill => ApplyLocation::WorkDir,
    };
    // this was for debug
    // let diff = make_diff(&git_diff, DiffKind::Unstaged);
    // sender
    //     .send_blocking(crate::Event::Conflicted(diff))
    //     .expect("Could not send through channel");
    // debug!("APPLY LOCATION {:?}", apply_location);

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
            let diff = make_diff(&git_diff, DiffKind::Staged);
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
    let diff = make_diff(&git_diff, DiffKind::Unstaged);
    sender
        .send_blocking(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}

pub fn get_parents_for_commit(path: OsString) -> Vec<Oid> {

    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    let mut result = Vec::new();
    let id = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse")
        .id();
    result.push(id);
    match repo.state() {
        RepositoryState::Clean => {
        },
        RepositoryState::Merge => {
            repo.mergehead_foreach(|oid: &Oid| -> bool {
                result.push(*oid);
                true
            }).expect("cant get merge heads");
        },
        _ => {
            todo!("commit in another state")
        }
    }
    result
}


pub fn commit(path: OsString, message: String, sender: Sender<crate::Event>) {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");

    // let ob = repo.revparse_single("HEAD^{commit}")
    //     .expect("fail revparse");
    // let id = repo.revparse_single("HEAD^{commit}")
    //     .expect("fail revparse").id();
    // let parent_commit = repo.find_commit(id).expect("cant find parent commit");
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


    let commits = get_parents_for_commit(path.clone())
        .into_iter()
        .map(|oid| repo.find_commit(oid).unwrap())
        .collect::<Vec<Commit>>();
    debug!("oooooooooooooooooooooooo {:?}", commits);

    match &commits[..] {
        [commit] => {
            let tree = repo.find_tree(tree_oid).expect("can't find tree");
            repo.commit(Some("HEAD"), &me, &me, &message, &tree, &[&commit])
                .expect("can't commit");
        }
        [commit, merge_commit] => {
            let mut merge_message = match repo.message() {
                Ok(mut msg) => {
                    if !message.is_empty() {
                        msg.push_str("\n");
                        msg.push_str(&message);
                    }
                    msg
                },
                Error => message
            };
            repo.commit(Some("HEAD"), &me, &me, &merge_message, &tree, &[&commit, &merge_commit])
                .expect("can't commit");
            repo.cleanup_state().expect("cant cleanup state");
        }
        _ => {
            todo!("multiple parents")
        }
    }
    // update staged changes
    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_index(Some(&current_tree), None, None)
        .expect("can't get diff tree to index");
    sender
        .send_blocking(crate::Event::Staged(make_diff(
            &git_diff,
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
            let diff = make_diff(&git_diff, DiffKind::Unstaged);
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
    user_pass: Option<(String, String)>,
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

    assert!(head_ref.is_branch());
    let branch = Branch::wrap(head_ref);
    let upstream = branch.upstream().unwrap();

    let u_oid = upstream.get().target().unwrap();
    let mut head_ref = repo.head().expect("can't get head");
    let log_message = format!(
        "(HEAD -> {}, {}) HEAD@{0}: pull: Fast-forward",
        branch.name().unwrap().unwrap(),
        upstream.name().unwrap().unwrap()
    );

    // think about it! perhaps it need to call merge analysys
    // during pull! if its fast formard - ok. if not - do merge, please.
    // see what git suggests:
    // Pulling without specifying how to reconcile divergent branches is
    // discouraged. You can squelch this message by running one of the following
    // commands sometime before your next pull:

    //   git config pull.rebase false  # merge (the default strategy)
    //   git config pull.rebase true   # rebase
    //   git config pull.ff only       # fast-forward only

    // You can replace "git config" with "git config --global" to set a default
    // preference for all repositories. You can also pass --rebase, --no-rebase,
    // or --ff-only on the command line to override the configured default per
    // invocation.
    let mut builder = CheckoutBuilder::new();
    let opts = builder.safe();
    let commit = repo.find_commit(u_oid).expect("can't find commit");

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    let result = repo.checkout_tree(commit.as_object(), Some(opts));
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

    match result {
        Ok(_) => {
            head_ref
                .set_target(u_oid, &log_message)
                .expect("cant set target");
            get_head(path.clone(), sender.clone());
        }
        Err(err) => {
            debug!(
                "errrrrrrrrrrror {:?} {:?} {:?}",
                err,
                err.code(),
                err.class()
            );
            match (err.code(), err.class()) {
                (ErrorCode::Conflict, ErrorClass::Checkout) => sender
                    .send_blocking(crate::Event::CheckoutError(
                        u_oid,
                        log_message,
                        String::from(err.message()),
                    ))
                    .expect("cant send through channel"),
                (code, class) => {
                    panic!("unknown checkout error {:?} {:?}", code, class)
                }
            };
        }
    }
}

const PLAIN_PASSWORD: &str = "plain text password required";

pub fn set_remote_callbacks(
    callbacks: &mut RemoteCallbacks,
    user_pass: &Option<(String, String)>,
) {
    // const PLAIN_PASSWORD: &str = "plain text password required";
    callbacks.credentials({
        let user_pass = user_pass.clone();
        move |url, username_from_url, allowed_types| {
            debug!("auth credentials url {:?}", url);
            // "git@github.com:aganzha/stage.git"
            debug!(
                "auth credentials username_from_url {:?}",
                username_from_url
            );
            debug!("auth credentials allowed_types {:?}", allowed_types);
            if allowed_types.contains(CredentialType::SSH_KEY) {
                let result =
                    Cred::ssh_key_from_agent(username_from_url.unwrap());
                debug!(
                    "got auth memory result. is it ok? {:?}",
                    result.is_ok()
                );
                return result;
            }
            if allowed_types == CredentialType::USER_PASS_PLAINTEXT {
                if let Some((user_name, password)) = &user_pass {
                    return Cred::userpass_plaintext(user_name, password);
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
    user_pass: Option<(String, String)>,
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
            sender
                .send_blocking(crate::Event::PushUserPass(
                    remote_branch,
                    tracking_remote,
                ))
                .expect("cant send through channel");
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
    pub fn from_branch(
        branch: Branch,
        branch_type: BranchType,
    ) -> Result<Self, Error> {
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
        let ob = bref.peel(ObjectType::Commit)?;
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

pub fn checkout_branch(
    path: OsString,
    mut branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> BranchData {
    info!("checkout branch");
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let commit = repo
        .find_commit(branch_data.oid)
        .expect("can't find commit");

    if branch_data.branch_type == BranchType::Remote {
        // handle case when checkout remote branch and local branch
        // is ahead of remote
        let head_ref = repo.head().expect("can't get head");
        assert!(head_ref.is_branch());
        let ob = head_ref
            .peel(ObjectType::Commit)
            .expect("can't get commit from ref!");
        let commit = ob.peel_to_commit().expect("can't get commit from ob!");
        if repo
            .graph_descendant_of(commit.id(), branch_data.oid)
            .expect("error comparing commits")
        {
            debug!("skip checkout ancestor tree");
            let branch = Branch::wrap(head_ref);
            return BranchData::from_branch(branch, BranchType::Local)
                .expect("cant get branch");
        }
    }
    let mut builder = CheckoutBuilder::new();
    let opts = builder.safe();

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");

    repo.checkout_tree(commit.as_object(), Some(opts))
        .expect("can't checkout tree");
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

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
            branch_data = BranchData::from_branch(branch, BranchType::Local)
                .expect("cant get branch");
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
    let branch_data = BranchData::from_branch(branch, BranchType::Local)
        .expect("cant get branch");
    if need_checkout {
        checkout_branch(path, branch_data, sender)
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
    if kind == BranchType::Remote {
        gio::spawn_blocking({
            let path = path.clone();
            let name = name.clone();
            move || {
                let repo =
                    Repository::open(path.clone()).expect("can't open repo");
                let mut remote = repo
                    .find_remote("origin") // TODO here is hardcode
                    .expect("no remote");
                let mut opts = PushOptions::new();
                let mut callbacks = RemoteCallbacks::new();
                set_remote_callbacks(&mut callbacks, &None);
                opts.remote_callbacks(callbacks);

                let refspec =
                    format!(":refs/heads/{}", name.replace("origin/", ""),);
                remote
                    .push(&[refspec], Some(&mut opts))
                    .expect("cant push to remote");
            }
        });
    }

    let result = branch.delete();
    if let Err(err) = result {
        trace!(
            "err on checkout {:?} {:?} {:?}",
            err.code(),
            err.class(),
            err.message()
        );
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

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    let result = repo.cherrypick(&commit, Some(&mut CherrypickOptions::new()));
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

    if let Err(err) = result {
        trace!(
            "err on checkout {:?} {:?} {:?}",
            err.code(),
            err.class(),
            err.message()
        );
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

    Ok(BranchData::from_branch(branch, BranchType::Local)
        .expect("cant get branch"))
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
        result.push(StashData::new(num, *oid, title.to_string()));
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
    let _oid = repo
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
    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    repo.stash_apply(stash_data.num, None)
        .expect("cant apply stash");
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");
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
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(ObjectType::Commit)
        .expect("can't get commit from ref!");
    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    repo.reset(&ob, ResetType::Hard, None)
        .expect("cant reset hard");
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");
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
        let mut parts: Vec<&str> = pth.split('/').collect();
        trace!("entry in index {:?}", parts);
        if parts.len() > 1 {
            parts.pop();
            directories.insert(parts.join("/"));
        }
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
            let diff = make_diff(&git_diff, DiffKind::Unstaged);
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
    pub message: String,
    pub commit_dt: DateTime<FixedOffset>,
    pub author: String,
    pub diff: Diff,
}

impl Default for CommitDiff {
    fn default() -> Self {
        CommitDiff {
            oid: Oid::zero(),
            message: String::from(""),
            commit_dt: DateTime::<FixedOffset>::MIN_UTC.into(),
            author: String::from(""),
            diff: Diff::new(DiffKind::Unstaged),
        }
    }
}

impl CommitDiff {
    pub fn new(commit: Commit, diff: Diff) -> Self {
        CommitDiff {
            oid: commit.id(),
            message: commit.message().unwrap_or("").replace('\n', ""),
            commit_dt: commit_dt(&commit),
            author: String::from(commit.author().name().unwrap_or("")),
            diff,
        }
    }
    pub fn from_commit(commit: Commit) -> Self {
        CommitDiff {
            oid: commit.id(),
            message: commit.message().unwrap_or("").replace('\n', ""),
            commit_dt: commit_dt(&commit),
            author: String::from(commit.author().name().unwrap_or("")),
            diff: Diff::new(DiffKind::Unstaged),
        }
    }
}

pub fn get_commit_diff(
    path: OsString,
    oid: Oid,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path).expect("can't open repo");
    let commit = repo.find_commit(oid).expect("cant find commit");
    let tree = commit.tree().expect("no get tree from commit");
    let parent = commit.parent(0).expect("cant get commit parent");

    let parent_tree = parent.tree().expect("no get tree from PARENT commit");
    let git_diff = repo
        .diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)
        .expect("can't get diff tree to index");
    let commit_diff =
        CommitDiff::new(commit, make_diff(&git_diff, DiffKind::Staged));
    sender
        .send_blocking(crate::Event::CommitDiff(commit_diff))
        .expect("Could not send through channel");
}

pub fn update_remote(
    path: OsString,
    _sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>,
) -> Result<(), ()> {
    let repo = Repository::open(path).expect("can't open repo");
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");

    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    remote
        .connect_auth(Direction::Fetch, Some(callbacks), None)
        .expect("cant connect");
    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    remote.prune(Some(callbacks)).expect("cant prune");

    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    callbacks.update_tips({
        move |updated_ref, oid1, oid2| {
            debug!("updat tips {:?} {:?} {:?}", updated_ref, oid1, oid2);
            true
        }
    });

    let mut opts = FetchOptions::new();
    opts.remote_callbacks(callbacks);
    let refs: [String; 0] = [];
    remote
        .fetch(&refs, Some(&mut opts), None)
        .expect("cant fetch");
    let mut callbacks = RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);
    remote
        .update_tips(Some(&mut callbacks), true, AutotagOption::Auto, None)
        .expect("cant update");

    Ok(())
}

pub fn checkout_oid(
    path: OsString,
    sender: Sender<crate::Event>,
    oid: Oid,
    ref_log_msg: Option<String>,
) {
    // DANGEROUS! see in status_view!
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let commit = repo.find_commit(oid).expect("can't find commit");
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let branch = Branch::wrap(head_ref);
    let log_message = match ref_log_msg {
        None => {
            format!("HEAD -> {}, {}", branch.name().unwrap().unwrap(), oid)
        }
        Some(msg) => msg,
    };
    let mut builder = CheckoutBuilder::new();
    let builder = builder.safe().allow_conflicts(true);

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    repo.checkout_tree(commit.as_object(), Some(builder))
        .expect("cant checkout oid");
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

    let mut head_ref = repo.head().expect("can't get head");
    head_ref
        .set_target(oid, &log_message)
        .expect("cant set target");
    get_current_repo_status(Some(path), sender);
}

const COMMIT_PAGE_SIZE: i32 = 500;

pub fn revwalk(
    path: OsString,
    start: Option<Oid>,
    search_term: Option<String>,
) -> Vec<CommitDiff> {
    let repo = Repository::open(path.clone()).expect("cant open repo");
    let mut revwalk = repo.revwalk().expect("cant get revwalk");
    revwalk.simplify_first_parent().expect("cant simplify");
    let mut i = 0;
    if let Some(oid) = start {
        revwalk.push(oid).expect("cant push oid to revlog");
    } else {
        revwalk.push_head().expect("no head for refwalk?");
    }
    let mut result: Vec<CommitDiff> = Vec::new();
    for oid in revwalk {
        let oid = oid.expect("no oid in rev");
        let commit = repo.find_commit(oid).expect("can't find commit");
        if let Some(ref term) = search_term {
            let mut found = false;
            for el in [commit.message().unwrap_or("").to_lowercase(),
                commit.author().name().unwrap_or("").to_lowercase()] {
                if el.contains(term) {
                    found = true;
                    break;
                }
            }
            if !found {
                continue;
            }
        }
        result.push(CommitDiff::from_commit(commit));
        i += 1;
        if i == COMMIT_PAGE_SIZE {
            break;
        }
    }
    result
}


pub fn resolve_conflict_v1(
    path: OsString,
    file_path: OsString,
    hunk_header: String,
    origin: DiffLineType,
    sender: Sender<crate::Event>,
) {
    debug!("resolve!");
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut opts = DiffOptions::new();
    let mut current_conflict: Option<IndexConflict> = None;
    for conflict in conflicts {
        if let Ok(conflict) = conflict {
            if let Some(ref our) = conflict.our {
                if file_path.to_str().unwrap() == String::from_utf8(our.path.clone()).unwrap() {
                    current_conflict.replace(conflict);
                }
            }
        }
    }
    let mut current_conflict = current_conflict.unwrap();
    let mut index = repo.index().expect("cant get index");

    // vv --------------------------------------------------------
    // delete whole conflict hunk and store lines which user
    // choosed to later apply them
    debug!(".........START removing conflicted file from index");
    index.remove_path(std::path::Path::new(&file_path)).expect("cant remove path");

    opts.pathspec(file_path.clone());
    opts.reverse(true);
    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo.diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
        .expect("cant get diff");

    let reversed_header = Hunk::reverse_header(hunk_header.clone());

    debug!(".........reverse header to apply to workdir to delete {:?}", &reversed_header);

    // ~~~~~~~~~~~~~~~~~~ store choosed lines ~~~~~~~~~~~~~~~~
    // lets store hunk lines, which will be removed from diff
    let mut choosed_lines = String::from("");
    let mut collect: bool = false;
    git_diff.foreach(
        &mut |delta: DiffDelta, _num| { // file cb
            true
        },
        None, // binary cb
        None, // hunk cb
        Some(&mut |_delta: DiffDelta, odh: Option<DiffHunk>, dl: DiffLine| {
            if let Some(dh) = odh {
                let header = Hunk::get_header_from(&dh);
                if header == reversed_header {
                    let content = String::from(
                        str::from_utf8(dl.content()).unwrap()
                    ).replace("\r\n", "").replace('\n', "");
                    debug!(".........collect {:?} and line in comparison: {:?}", collect, &content);
                    if content.len() >= 3 {
                        match &content[..3] {
                            "<<<" => {
                                debug!("..start collecting OUR SIDE");
                                // collect = true;
                            },
                            "===" => {
                                debug!("..stop collecting OR start collecting THEIR side");
                                // collect = false;
                                collect = true;
                            }
                            ">>>" => {
                                debug!("..stop collecting");
                                collect = false;
                            }
                            _ => {
                                if collect {
                                    debug!("collect this line!");
                                    choosed_lines.push_str(&content);
                                }
                            }
                        }
                    } else {
                        if collect {
                            debug!("collect this line!");
                            choosed_lines.push_str(&content);
                        }
                    }
                }
            }
            true
        })
    ).expect("cant iter on diff");
    debug!("~~~~~~~ choosed lines before delete: {}", choosed_lines);
    // ~~~~~~~~~~~~~~~~~~ store choosed lines ~~~~~~~~~~~~~~~~


    let mut options = ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            debug!("--apply delete patch. hunk callback {:?} {:?} == {:?}", header, Hunk::reverse_header(hunk_header.clone()), header == Hunk::reverse_header(hunk_header.clone()));
            return header == reversed_header
        }
        false
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: OsString = dd.new_file().path().unwrap().into();
            debug!("--apply delete patch. delta callback {:?} {:?} {:?}", file_path, path, file_path == path);
            return file_path == path;
        }
        todo!("diff without delta");
    });


    sender.send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.apply(&git_diff, ApplyLocation::WorkDir, Some(&mut options))
        .expect("can't apply patch");
    // ^^ -----------------------------------------------------
    debug!("..... conflict removed from workdir");
    // NOW. if user choosed our side, this means NOTHING
    // else todo with this current conflict. changes already were
    // reverted to our side! next part is valid only if chooser
    // used THEIR side! are you sure? yes. conflicts are removed
    // and workdir is restored according to the our tree (HEAD in this branch)!


    // vv --------------------------- apply hunk from choosed side

    // so. it is not possible to find tree from blob.
    // lets put this blob to index, maybe?
    let their_entry = current_conflict.their.as_mut().unwrap();
    let their_original_flags = their_entry.flags;

    debug!(">>>>> flags before {:?}", their_original_flags);
    their_entry.flags = their_entry.flags & !STAGE_FLAG;
    debug!(">>>>> flags after mask {:?}", their_entry.flags);

    index.add(their_entry).expect("cant add entry");
    let mut opts = DiffOptions::new();
    opts.pathspec(file_path.clone());
    // reverse means index will be NEW side cause we are adding hunk to workdir
    opts.reverse(true);
    let git_diff = repo.diff_index_to_workdir(Some(&index), Some(&mut opts))
        .expect("cant get diff");

    // restore stage flag to conflict again
    their_entry.flags = their_original_flags;
    debug!(">>>>> flags after restore {:?}", their_entry.flags);

    // let diff = make_diff(&git_diff, DiffKind::Staged);
    // sender
    //     .send_blocking(crate::Event::Staged(diff))
    //     .expect("Could not send through channel");

    // vv ~~~~~~~~~~~~~~~~ select hunk header for choosed lines
    let mut hunk_header_to_apply = String::from("");
    let mut current_hunk_header = String::from("");
    let mut found_lines = String::from("");

    debug!("..... choosing hunk to apply for choosed lines");

    let result = git_diff.foreach(
        &mut |_delta: DiffDelta, _num| { // file cb
            true
        },
        None, // binary cb
        None, // hunk cb
        Some(&mut |_delta: DiffDelta, odh: Option<DiffHunk>, dl: DiffLine| {
            if let Some(dh) = odh {
                if !hunk_header_to_apply.is_empty() {
                    // all done
                    debug!("+++ all good. return");
                    return false;
                }
                let header = Hunk::get_header_from(&dh);
                if header != current_hunk_header {
                    // handle next header (or first one)
                    debug!("++++ thats new hunk header and current one {:?} {:?}", header, current_hunk_header);
                    if found_lines == choosed_lines {
                        debug!("!!!!!!!!!!!!!!!!!! match!");
                        hunk_header_to_apply = current_hunk_header.clone();
                        // all done
                        debug!("allllllllllllll done");
                        return false;
                    }
                    debug!("+++ reset found lines");
                    current_hunk_header = header;
                    found_lines = String::from("");
                }
                if dl.origin_value() == origin {
                    let content = String::from(
                        str::from_utf8(dl.content()).unwrap()
                    ).replace("\r\n", "").replace('\n', "");
                    found_lines.push_str(&content);
                    debug!("++++ thats current line and total found lines {:?} {:?}", &content, &found_lines)
                }
            }
            true
        })
    );
    if result.is_ok() {
        // handle case when choosed hunk is last one
        assert!(hunk_header_to_apply.is_empty());
        debug!("++++ outside of loop. found lines {:?}", found_lines);
        if found_lines == choosed_lines {
            hunk_header_to_apply = current_hunk_header;
        }
    }
    if hunk_header_to_apply.is_empty() {
        panic!("cant find header for choosed_lines {:?}", choosed_lines);
    }
    // ^^ ~~~~~~~~~~~~~~~~ select hunk header for choosed lines

    let mut options = ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            debug!("**** apply hunk callback {:?} {:?} == {:?}", header, hunk_header_to_apply, header == hunk_header_to_apply);
            return header == hunk_header_to_apply
        }
        false
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: OsString = dd.new_file().path().unwrap().into();
            debug!("**** apply delta callback {:?} {:?} {:?}", file_path, path, file_path == path);
            return file_path == path;
        }
        todo!("diff without delta");
    });
    repo.apply(&git_diff, ApplyLocation::WorkDir, Some(&mut options))
        .expect("can't apply patch");
    // ^^ ----------------------------


    sender.send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // remove from index again to restore conflict
    index.remove_path(std::path::Path::new(&file_path)).expect("cant remove path");

    // ------------------------------------------
    // restore conflict file in index if not all conflicts were resolved
    if let Some(entry) = current_conflict.ancestor {
        index.add(&entry).expect("cant add ancestor");
        debug!("ancestor added!");
    }
    if let Some(entry) = current_conflict.our {
        debug!("our added!");
        index.add(&entry).expect("cant add our");
    }
    if let Some(entry) = current_conflict.their {
        debug!("their added!");
        index.add(&entry).expect("cant add their");
    }
    index.write().expect("cant write index");
    // ^^ -------------------------------------------
    get_conflicted_v1(path, sender);
}


// pub const GIT_INDEX_ENTRY_STAGEMASK: u16 = 0x3000;
pub const STAGE_FLAG: u16 = 0x3000;

pub fn resolve_conflict(
    path: OsString,
    file_path: OsString,
    hunk_header: String,
    origin: DiffLineType,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone()).expect("cant open repo");
    let mut index = repo.index().expect("cant get index");

    for entry in index.iter() {
        debug!(".. {:?} {:?}", entry.flags, String::from_utf8_lossy(&entry.path));
    }
    debug!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee {:?}", file_path);

    let conflicts = index.conflicts().expect("no conflicts");
    let mut entry: Option<IndexEntry> = None;
    let my_path = "src/TODO.txt";
    for conflict in conflicts {
        let conflict = conflict.unwrap();
        let mut choosed = {
            // deletion is our side due to blob diff:
            // our is old side and merged branch is new side
            if origin == DiffLineType::Deletion {
                conflict.our.unwrap()
            } else {
                conflict.their.unwrap()
            }
        };
        if String::from_utf8_lossy(&choosed.path) != my_path {
            continue
        }
        choosed.flags = choosed.flags & !STAGE_FLAG;
        entry.replace(choosed);
        break;
    }
    index.add(&entry.unwrap()).expect("cant add entry");
    for stage in 1..4 {
        index.remove(path::Path::new(my_path), stage).expect("cant remove entry");
    }
    for entry in index.iter() {
        debug!(".. {:?} {:?}", entry.flags, String::from_utf8_lossy(&entry.path));
    }
    index.write().expect("cant write index");
    let mut options = DiffOptions::new();
    options.reverse(true);
    options.pathspec(my_path);
    // allow empty chunks!
    options.include_unmodified(true);
    let git_diff = repo
        .diff_index_to_workdir(Some(&index), Some(&mut options))
        .expect("cant' get diff index to workdir");
    repo.apply(&git_diff, ApplyLocation::WorkDir, None)
        .expect("can't apply patch");
    get_current_repo_status(Some(path), sender)
    // let diff = make_diff(&git_diff, DiffKind::Unstaged);
    // sender
    //     .send_blocking(crate::Event::Unstaged(diff))
    //     .expect("Could not send through channel");
    // Index.add_path - will mark file as resolved
    // now it need somehow organize diff.
}


pub fn debug(path: OsString) {
    let repo = Repository::open(path.clone()).expect("cant open repo");
    repo.cleanup_state().expect("cant cleanup state");
}
