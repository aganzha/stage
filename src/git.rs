pub mod branch;
pub mod commit;
pub mod merge;
pub mod remote;
pub mod git_log;
use crate::branch::BranchData;
use crate::commit::{commit_string};
use crate::status_view::render::View;
use crate::gio;
use async_channel::Sender;


use git2::build::CheckoutBuilder;
use git2::{
    ApplyLocation, ApplyOptions, AutotagOption, Branch, BranchType,
    CertificateCheckStatus, CherrypickOptions, Commit, Cred, CredentialType,
    Delta, Diff as GitDiff, DiffDelta, DiffFile, DiffFormat, DiffHunk,
    DiffLine, DiffLineType, DiffOptions, Direction, Error, ErrorClass,
    ErrorCode, FetchOptions, ObjectType, Oid, PushOptions, RemoteCallbacks,
    Repository, RepositoryState, ResetType, StashFlags,
};

// use libgit2_sys;
use log::{debug, trace};
use regex::Regex;
//use std::time::SystemTime;
use std::path::PathBuf;
use std::{collections::HashSet, env, path, str};


#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    None,
    Ours,
    Theirs,
    ConflictMarker(String),
}

#[derive(Debug, Clone)]
pub struct Line {
    pub view: View,
    pub origin: DiffLineType,
    pub content: String,
    pub new_line_no: Option<u32>,
    pub old_line_no: Option<u32>,
    pub kind: LineKind,
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
            kind: LineKind::None,
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
    pub has_conflicts: bool,
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
            kind,
            has_conflicts: false,
        }
    }

    pub fn get_header_from(dh: &DiffHunk) -> String {
        String::from(str::from_utf8(dh.header()).unwrap())
            .replace("\r\n", "")
            .replace('\n', "")
    }

    pub fn handle_max(&mut self, line: &str) {
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

    pub fn replace_new_lines(header: &String, delta: i32) -> String {
        let re = Regex::new(r"@@ [+-][0-9]+,[0-9]+ [+-][0-9]+,([0-9]+) @@")
            .unwrap();
        if let Some((_, [nums])) =
            re.captures_iter(&header).map(|c| c.extract()).next()
        {
            let mut inums: i32 = nums.parse().expect("cant parse nums");
            inums += delta;
            return header.replace(nums, &inums.to_string());
        }
        panic!("cant replace num in header")
    }

    // THE REGEX IS WRONG! remove .* !!!!!!!!!!!!! for +
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

    pub fn push_line(
        &mut self,
        mut line: Line,
        prev_line_kind: LineKind,
    ) -> LineKind {
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
        trace!(
            ":::::::::::::::::::::::::::::::: {:?}. prev line kind {:?}",
            line.content,
            prev_line_kind
        );
        if line.content.len() >= 7 {
            match &line.content[..7] {
                MARKER_OURS | MARKER_THEIRS | MARKER_VS => {
                    self.has_conflicts = true;
                    line.kind = LineKind::ConflictMarker(String::from(
                        &line.content[..7],
                    ));
                }
                _ => {}
            }
        }

        let marker_ours = String::from(MARKER_OURS);
        let marker_vs = String::from(MARKER_VS);
        let marker_theirs = String::from(MARKER_THEIRS);

        match (prev_line_kind, &line.kind) {
            (LineKind::ConflictMarker(marker), LineKind::None)
                if marker == marker_ours =>
            {
                trace!(
                    "sec match. ours after ours MARKER ??????????? {:?}",
                    marker_ours
                );
                line.kind = LineKind::Ours
            }
            (LineKind::Ours, LineKind::None) => {
                trace!("sec match. ours after ours LINE");
                line.kind = LineKind::Ours
            }
            (LineKind::ConflictMarker(marker), LineKind::None)
                if marker == marker_vs =>
            {
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
            (_prev, LineKind::ConflictMarker(m)) => {
                trace!("sec match. pass this marker {:?}", m);
            }
            (LineKind::ConflictMarker(marker), LineKind::None)
                if marker == marker_theirs =>
            {
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
    pub path: PathBuf,
    // pub id: Oid,
    pub hunks: Vec<Hunk>,
    pub max_line_len: i32,
    pub kind: DiffKind,
}

impl File {
    pub fn new(kind: DiffKind) -> Self {
        Self {
            view: View::new(),
            path: PathBuf::new(),
            // id: Oid::zero(),
            hunks: Vec::new(),
            max_line_len: 0,
            kind,
        }
    }
    pub fn from_diff_file(f: &DiffFile, kind: DiffKind) -> Self {
        let path: PathBuf = f.path().unwrap().into();
        let len = path.as_os_str().len();
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
    Conflicted,
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
        self.files.is_empty()
    }

    pub fn has_conflicts(&self) -> bool {
        self.files
            .iter()
            .flat_map(|f| &f.hunks)
            .any(|h| h.has_conflicts)
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
        self.state == RepositoryState::Merge
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

pub fn get_head(path: PathBuf, sender: Sender<crate::Event>) {
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

pub fn get_upstream(path: PathBuf, sender: Sender<crate::Event>) {
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
    current_path: Option<PathBuf>,
    sender: Sender<crate::Event>,
) {
    // path could came from command args or from choosing path
    // by user
    let path = {
        if let Some(path) = current_path {
            path
        } else {
            env::current_exe().expect("cant't get exe path")
        }
    };
    let repo = Repository::discover(path.clone()).expect("can't open repo");
    let path = PathBuf::from(repo.path());
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
        // get_staged
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
                let diff = get_conflicted_v1(path);
                sender
                    .send_blocking(crate::Event::Conflicted(diff))
                    .expect("Could not send through channel");
            }
        });
    } else {
        sender
            .send_blocking(crate::Event::Conflicted(Diff::new(
                DiffKind::Conflicted,
            )))
            .expect("Could not send through channel");
    }
    // get_unstaged
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant' get diff index to workdir");
    let diff = make_diff(&git_diff, DiffKind::Unstaged);
    sender
        .send_blocking(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}

pub fn get_conflicted_v1(path: PathBuf) -> Diff {
    // so, when file is in conflictduring merge, this means nothing
    // was staged to that file, cause mergeing in such state is PROHIBITED!
    // sure? Yes, it must be true!
    let repo = Repository::open(path).expect("can't open repo");
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
    let git_diff = repo
        .diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
        .expect("cant get diff");

    make_diff(&git_diff, DiffKind::Conflicted)
}

pub fn get_untracked(path: PathBuf, sender: Sender<crate::Event>) {
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
                let path: PathBuf = delta.new_file().path().unwrap().into();
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
    pub path: PathBuf,
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
            path: PathBuf::new(),
            view: View::new(),
        }
    }
    pub fn from_path(path: PathBuf) -> Self {
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
    pub fn push_file(&mut self, path: PathBuf) {
        let le = path.as_os_str().len();
        if le as i32 > self.max_line_len {
            self.max_line_len = le as i32;
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
            if status == Delta::Conflicted
                && (kind == DiffKind::Staged || kind == DiffKind::Unstaged)
            {
                return true;
            }
            let file: DiffFile = match status {
                Delta::Modified | Delta::Conflicted => diff_delta.new_file(),
                Delta::Deleted => diff_delta.old_file(),
                Delta::Added => match diff.kind {
                    DiffKind::Staged => diff_delta.new_file(),
                    DiffKind::Unstaged => {
                        todo!("delta added in unstaged {:?}", diff_delta)
                    }
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
            if current_file.path.capacity() == 0 {
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
                prev_line_kind = current_hunk.push_line(
                    Line::from_diff_line(&diff_line),
                    prev_line_kind.clone(),
                );
            } else {
                // this is file header line.
                let line = Line::from_diff_line(&diff_line);
                prev_line_kind =
                    current_hunk.push_line(line, prev_line_kind.clone())
            }

            true
        },
    );
    if !current_hunk.header.is_empty() {
        current_file.push_hunk(current_hunk);
    }
    if current_file.path.capacity() != 0 {
        diff.push_file(current_file);
    }
    diff
}

pub fn stage_untracked(
    path: PathBuf,
    file: UntrackedFile,
    sender: Sender<crate::Event>,
) {
    trace!("stage untracked! {:?}", file.path);
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let pth = path::Path::new(&file.path);
    if pth.is_file() {
        index.add_path(pth).expect("cant add path");
    } else if pth.is_dir() {
        index
            .add_all([pth], git2::IndexAddOption::DEFAULT, None)
            .expect("cant add path");
    } else {
        panic!("unknown path {:?}", pth);
    }
    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender);
}

pub fn stage_via_apply(
    path: PathBuf,
    filter: ApplyFilter,
    sender: Sender<crate::Event>,
) {
    // TODO! destruct filter to args. put file in pathspec for diff opts
    let repo = Repository::open(path.clone()).expect("can't open repo");

    let mut opts = DiffOptions::new();
    opts.pathspec(&filter.file_id);

    let git_diff = match filter.subject {
        ApplySubject::Stage => repo
            .diff_index_to_workdir(None, Some(&mut opts))
            .expect("can't get diff"),
        ApplySubject::Unstage => {
            opts.reverse(true);
            let ob =
                repo.revparse_single("HEAD^{tree}").expect("fail revparse");
            let current_tree =
                repo.find_tree(ob.id()).expect("no working tree");
            repo.diff_tree_to_index(Some(&current_tree), None, Some(&mut opts))
                .expect("can't get diff")
        }
        ApplySubject::Kill => {
            opts.reverse(true);
            repo.diff_index_to_workdir(None, Some(&mut opts))
                .expect("cant get diff")
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
            let path: PathBuf = dd.new_file().path().unwrap().into();
            return filter.file_id
                == path.into_os_string().into_string().unwrap();
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

pub fn get_stashes(path: PathBuf, sender: Sender<crate::Event>) -> Stashes {
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
    path: PathBuf,
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
    path: PathBuf,
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
    path: PathBuf,
    stash_data: StashData,
    sender: Sender<crate::Event>,
) -> Stashes {
    let mut repo = Repository::open(path.clone()).expect("can't open repo");
    repo.stash_drop(stash_data.num).expect("cant drop stash");
    get_stashes(path, sender)
}

pub fn reset_hard(path: PathBuf, sender: Sender<crate::Event>) {
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

pub fn get_directories(path: PathBuf) -> HashSet<String> {
    let repo = Repository::open(path).expect("can't open repo");
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
    path: PathBuf,
    file_path: PathBuf,
    sender: Sender<crate::Event>,
) {
    // TODO throttle!
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let index = repo.index().expect("cant get index");
    let file_path = file_path
        .into_os_string()
        .into_string()
        .expect("wrong path");
    for entry in index.iter() {
        let entry_path = format!("{}", String::from_utf8_lossy(&entry.path));
        if file_path.ends_with(&entry_path) {
            trace!("got modified file {:?}", file_path);
            // TODO. so. here ir need to collect dif only for 1 file.
            // why all? but there way not, ti update just 1 file!
            // but it is easy, really (just use existent diff and update only 1 file in it!)
            let git_diff = repo
                .diff_index_to_workdir(Some(&index), None)
                .expect("cant' get diff index to workdir");
            let diff = make_diff(&git_diff, DiffKind::Unstaged);
            sender
                .send_blocking(crate::Event::Unstaged(diff))
                .expect("Could not send through channel");
            break;
        }
    }
    if index.has_conflicts() {
        // same here - update just 1 file, please
        let diff = get_conflicted_v1(path);
        sender
            .send_blocking(crate::Event::Conflicted(diff))
            .expect("Could not send through channel");
    }
}


pub fn checkout_oid(
    path: PathBuf,
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

pub fn debug(path: PathBuf) {
    let repo = Repository::open(path.clone()).expect("cant open repo");
    repo.cleanup_state().expect("cant cleanup state");
}
