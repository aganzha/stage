// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod branch;
pub mod commit;
pub mod conflict;
pub mod git_log;
pub mod merge;
pub mod remote;
pub mod stash;
pub mod tag;
pub mod test_conflict;
use crate::branch::BranchData;
use crate::commit::CommitRepr;
use crate::gio;
use crate::status_view::view::View;
use async_channel::Sender;

use chrono::{DateTime, FixedOffset};
use git2::build::CheckoutBuilder;
use git2::{
    ApplyLocation, ApplyOptions, Branch, Commit, Delta, Diff as GitDiff, DiffDelta, DiffFile,
    DiffFormat, DiffHunk, DiffLine, DiffLineType, DiffOptions, Error, ObjectType, Oid,
    RebaseOptions, Repository, RepositoryState, ResetType, StatusOptions,
};
use log::{debug, error, info, trace};
use regex::Regex;
//use std::time::SystemTime;
use core::ops::Range;
use std::fmt;
use std::num::ParseIntError;
use std::ops::{Add, Sub};
use std::path::PathBuf;
use std::str::FromStr;
use std::{collections::HashSet, str};
use syntect::easy::ScopeRangeIterator;
use syntect::parsing::{ParseState, Scope, ScopeStackOp, SyntaxSet};

pub fn make_diff_options() -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.indent_heuristic(true);
    opts.minimal(true);
    opts
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct HunkLineNo(u32);

impl HunkLineNo {
    pub fn new(num: u32) -> Self {
        Self(num)
    }
    pub fn as_u32(&self) -> u32 {
        self.0
    }
    pub fn as_i32(&self) -> i32 {
        self.0 as i32
    }
}
impl FromStr for HunkLineNo {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u32::from_str(s).map(HunkLineNo)
    }
}
impl fmt::Display for HunkLineNo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Sub for HunkLineNo {
    type Output = Self;
    fn sub(self, other: Self) -> Self::Output {
        Self(self.0 - other.0)
    }
}

impl Add for HunkLineNo {
    type Output = Self;
    fn add(self, other: Self) -> Self::Output {
        Self(self.0 + other.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    None,
    Ours(i32),
    Theirs(i32),
    ConflictMarker(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub view: View,
    pub origin: DiffLineType,
    pub new_line_no: Option<HunkLineNo>,
    pub old_line_no: Option<HunkLineNo>,
    pub kind: LineKind,
    pub content_idx: (usize, usize),
    pub syntax: Vec<(Range<usize>, Scope)>,
}

impl Default for Line {
    fn default() -> Self {
        Line {
            view: View::new(),
            origin: DiffLineType::Addition,
            new_line_no: None,
            old_line_no: None,
            kind: LineKind::None,
            content_idx: (0, 0),
            syntax: Vec::new(),
        }
    }
}

impl Line {
    pub fn content<'a>(&'a self, hunk: &'a Hunk) -> &'a str {
        &hunk.buf[self.content_idx.0..self.content_idx.0 + self.content_idx.1]
    }

    pub fn from_diff_line(
        l: &DiffLine,
        content_from: usize,
        content_to: usize,
        syntax: Vec<(Range<usize>, Scope)>,
    ) -> Self {
        Self {
            view: View::new(),
            origin: l.origin_value(),
            new_line_no: l.new_lineno().map(HunkLineNo),
            old_line_no: l.old_lineno().map(HunkLineNo),
            kind: LineKind::None,
            content_idx: (content_from, content_to),
            syntax,
        }
    }
    pub fn is_our_side_of_conflict(&self) -> bool {
        match &self.kind {
            LineKind::Ours(_) => true,
            LineKind::ConflictMarker(m) if m == MARKER_OURS => true,
            _ => false,
        }
    }
    pub fn is_their_side_of_conflict(&self) -> bool {
        match &self.kind {
            LineKind::Theirs(_) => true,
            LineKind::ConflictMarker(m) if m == MARKER_THEIRS => true,
            _ => false,
        }
    }
    pub fn is_side_of_conflict(&self) -> bool {
        self.is_our_side_of_conflict() || self.is_their_side_of_conflict()
    }

    pub fn repr(&self, title: &str, _chars_to_take: usize) -> String {
        format!(
            "{} new_line_no: {:?} old_line_no: {:?} knd: {:?} orgn: {:?}",
            title, self.new_line_no, self.old_line_no, self.kind, self.origin
        )
    }
}

pub const MARKER_OURS: &str = "<<<<<<<";
pub const MARKER_VS: &str = "=======";
pub const MARKER_THEIRS: &str = ">>>>>>>";

pub const MINUS: &str = "-";
pub const SPACE: &str = " ";

#[derive(Debug, Clone)]
pub struct Hunk {
    pub view: View,
    pub header: String,
    pub old_start: HunkLineNo,
    pub new_start: HunkLineNo,
    pub old_lines: u32,
    pub new_lines: u32,
    pub lines: Vec<Line>,
    pub kind: DiffKind,
    pub conflict_markers_count: i32,
    pub buf: String,
}

impl fmt::Display for Hunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.header)
    }
}

impl Hunk {
    pub fn new(kind: DiffKind) -> Self {
        let view = View::new();
        view.expand(true);
        Self {
            view,
            header: String::new(),
            lines: Vec::new(),
            old_start: HunkLineNo(0),
            new_start: HunkLineNo(0),
            old_lines: 0,
            new_lines: 0,
            kind,
            conflict_markers_count: 0,
            buf: String::new(),
        }
    }

    pub fn get_header_from(dh: &DiffHunk) -> String {
        String::from(str::from_utf8(dh.header()).unwrap())
            .replace("\r\n", "")
            .replace('\n', "")
    }

    pub fn fill_from_git_hunk(&mut self, dh: &DiffHunk) {
        let header = Self::get_header_from(dh);
        self.header = header;
        self.old_start = HunkLineNo(dh.old_start());
        self.old_lines = dh.old_lines();
        self.new_start = HunkLineNo(dh.new_start());
        self.new_lines = dh.new_lines();
        self.buf =
            String::with_capacity(1 + 3 + self.old_lines as usize + self.new_lines as usize + 3);
    }

    pub fn shift_new_start_and_lines(header: &str, hunk_delta: i32, lines_delta: i32) -> String {
        let re = Regex::new(r"@@ [+-][0-9]+,[0-9]+ [+-]([0-9]+),([0-9]+) @@").unwrap();
        if let Some((_, [new_start, new_lines])) =
            re.captures_iter(header).map(|c| c.extract()).next()
        {
            let i_new_start: i32 = new_start.parse().expect("cant parse nums");
            let i_new_lines: i32 = new_lines.parse().expect("cant parse nums");

            return header.replace(
                &format!("{},{} @@", i_new_start, i_new_lines),
                &format!(
                    "{},{} @@",
                    i_new_start + hunk_delta,
                    i_new_lines + lines_delta
                ),
            );
        }
        panic!("cant replace num in header")
    }

    pub fn replace_new_lines(header: &str, delta: i32) -> String {
        let re = Regex::new(r"@@ [+-][0-9]+,[0-9]+ [+-][0-9]+,([0-9]+) @@").unwrap();
        if let Some((_, [nums])) = re.captures_iter(header).map(|c| c.extract()).next() {
            let old_nums: i32 = nums.parse().expect("cant parse nums");
            let new_nums: i32 = old_nums + delta;

            return header.replace(&format!(",{} @@", old_nums), &format!(",{} @@", new_nums));
        }
        panic!("cant replace num in header")
    }

    // used in reconsilation
    pub fn shift_new_start(header: &str, delta: i32) -> String {
        let re = Regex::new(r"@@ [+-][0-9]+,[0-9]+ ([+-]([0-9]+),[0-9]+ @@)").unwrap();
        if let Some((_, [whole_new, nums])) = re.captures_iter(header).map(|c| c.extract()).next() {
            let old_nums: i32 = nums.parse().expect("cant parse nums");

            let new_nums: i32 = old_nums + delta;
            let new = whole_new.replace(&format!("{},", old_nums), &format!("{},", new_nums));
            return header.replace(whole_new, &new);
        }
        panic!("cant replace num in header")
    }

    pub fn shift_old_start(header: &str, delta: i32) -> String {
        let re = Regex::new(r"(@@ [+-]([0-9]+),[0-9]+) [+-][0-9]+,[0-9]+ @@").unwrap();
        if let Some((_, [whole_new, nums])) = re.captures_iter(header).map(|c| c.extract()).next() {
            let old_nums: i32 = nums.parse().expect("cant parse nums");

            let new_nums: i32 = old_nums + delta;
            let new = whole_new.replace(&format!("{},", old_nums), &format!("{},", new_nums));
            return header.replace(whole_new, &new);
        }
        panic!("cant replace num in header")
    }

    pub fn reverse_header(header: &str) -> String {
        // "@@ -1,3 +1,7 @@" -> "@@ -1,7 +1,3 @@"
        // "@@ -20,10 +24,11 @@ STAGING LINE..." -> "@@ -24,11 +20,10 @@ STAGING LINE..."
        // "@@ -54,7 +59,6 @@ do not call..." -> "@@ -59,6 +54,7 @@ do not call..."
        let re = Regex::new(r"@@ [+-]([0-9]+,[0-9]+) [+-]([0-9]+,[0-9]+) @@").unwrap();
        if let Some((whole, [nums1, nums2])) = re.captures_iter(header).map(|c| c.extract()).next()
        {
            // for (whole, [nums1, nums2]) in re.captures_iter(&header).map(|c| c.extract()) {
            let result = whole
                .replacen(nums1, "mock", 1)
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

    pub fn push_line(
        &mut self,
        diff_line: &DiffLine,
        prev_line_kind: LineKind,
        syntax_parse_state: Option<ParseState>,
        ss: &SyntaxSet,
    ) -> LineKind {
        let mut content = str::from_utf8(diff_line.content()).unwrap_or("!!!unreadable unicode!!!");
        if let Some(striped) = content.strip_suffix("\r\n") {
            content = striped;
        }
        if let Some(striped) = content.strip_suffix('\n') {
            content = striped;
        }
        if let Some(striped) = content.strip_prefix("\r\n") {
            content = striped;
        }
        if let Some(striped) = content.strip_prefix('\n') {
            content = striped;
        }
        let mut syntax_ranges = Vec::new();
        if let Some(mut syntax_parse_state) = syntax_parse_state {
            if let Ok(ops) = syntax_parse_state.parse_line(content, ss) {
                for (range, op) in ScopeRangeIterator::new(&ops, content) {
                    if let ScopeStackOp::Push(s) = op {
                        syntax_ranges.push((range, *s));
                    }
                }
            }
        }
        let mut line =
            Line::from_diff_line(diff_line, self.buf.len(), content.len(), syntax_ranges);
        self.buf.push_str(content);
        if self.kind != DiffKind::Conflicted {
            match line.origin {
                DiffLineType::FileHeader | DiffLineType::HunkHeader | DiffLineType::Binary => {}
                _ => self.lines.push(line),
            }
            return LineKind::None;
        }
        trace!(
            ":::::::::::::::::::::::::::::::: {:?}. prev line kind {:?}",
            content,
            prev_line_kind
        );
        let prefix: String = content.chars().take(7).collect();

        match &prefix[..] {
            MARKER_OURS | MARKER_THEIRS | MARKER_VS => {
                self.conflict_markers_count += 1;
                line.kind = LineKind::ConflictMarker(prefix);
            }
            _ => {}
        }

        let marker_ours = String::from(MARKER_OURS);
        let marker_vs = String::from(MARKER_VS);
        let marker_theirs = String::from(MARKER_THEIRS);

        match (prev_line_kind, &line.kind) {
            (LineKind::ConflictMarker(marker), LineKind::None) if marker == marker_ours => {
                trace!("sec match. ours after ours MARKER {:?}", marker_ours);
                line.kind = LineKind::Ours(self.conflict_markers_count)
            }
            (LineKind::Ours(_), LineKind::None) => {
                trace!("sec match. ours after ours LINE");
                line.kind = LineKind::Ours(self.conflict_markers_count)
            }
            (LineKind::ConflictMarker(marker), LineKind::None) if marker == marker_vs => {
                trace!("sec match. theirs after vs MARKER");
                line.kind = LineKind::Theirs(self.conflict_markers_count)
            }
            (LineKind::Theirs(_), LineKind::None) => {
                trace!("sec match. theirs after theirs LINE");
                line.kind = LineKind::Theirs(self.conflict_markers_count)
            }
            (LineKind::None, LineKind::None) => {
                trace!("sec match. contenxt????")
            }
            (_prev, LineKind::ConflictMarker(m)) => {
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
            DiffLineType::FileHeader | DiffLineType::HunkHeader | DiffLineType::Binary => {}
            _ => self.lines.push(line),
        }
        this_kind
    }

    // /// by given Line inside conflict returns
    // /// the conflict offset from hunk start
    // pub fn get_conflict_offset_by_line(&self, line: &Line) -> i32 {
    //     let mut conflict_offset_inside_hunk: i32 = 0;
    //     for (i, l) in self.lines.iter().enumerate() {
    //         if l.content(self).starts_with(MARKER_OURS) {
    //             conflict_offset_inside_hunk = i as i32;
    //         }
    //         if l == line {
    //             break;
    //         }
    //     }
    //     conflict_offset_inside_hunk
    // }
}

#[derive(Debug, Clone)]
pub struct File {
    pub view: View,
    pub path: PathBuf,
    pub hunks: Vec<Hunk>,
    pub kind: DiffKind,
    pub status: Delta,
}

impl File {
    pub fn new(kind: DiffKind) -> Self {
        Self {
            view: View::new(),
            path: PathBuf::new(),
            hunks: Vec::new(),
            kind,
            status: Delta::Unmodified,
        }
    }
    pub fn from_diff_file(f: &DiffFile, kind: DiffKind, status: Delta) -> Self {
        let path: PathBuf = f.path().unwrap().into();

        File {
            view: View::new(),
            path,
            hunks: Vec::new(),
            kind,
            status,
        }
    }

    pub fn push_hunk(&mut self, h: Hunk) {
        self.hunks.push(h);
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DiffKind {
    Staged,
    Unstaged,
    Conflicted,
    Untracked,
    Commit,
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub files: Vec<File>,
    pub view: View,
    pub kind: DiffKind,
}

impl Diff {
    pub fn new(kind: DiffKind) -> Self {
        let view = View::new();
        view.expand(true);
        view.child_dirty(true);
        Self {
            files: Vec::new(),
            view,
            kind,
        }
    }

    pub fn push_file(&mut self, f: File) {
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
            .any(|h| h.conflict_markers_count > 0)
    }
}

#[derive(Debug, Clone)]
pub struct State {
    pub state: RepositoryState,
    pub subject: String,
    pub view: View,
}

impl State {
    pub fn new(state: RepositoryState, subject: String) -> Self {
        Self {
            state,
            subject,
            view: View::new(),
        }
    }
    pub fn need_final_commit(&self) -> bool {
        matches!(
            self.state,
            RepositoryState::Merge | RepositoryState::CherryPick | RepositoryState::Revert
        )
    }
    pub fn need_rebase_continue(&self) -> bool {
        matches!(self.state, RepositoryState::RebaseMerge)
    }

    fn from_git_state(state: git2::RepositoryState, path: PathBuf) -> State {
        let mut subject = String::from("");
        if let Some(path_to_read_subject) = match state {
            RepositoryState::CherryPick => {
                let mut pth = path.clone();
                pth.push(CHERRY_PICK_HEAD);
                Some(pth)
            }
            RepositoryState::Revert => {
                let mut pth = path.clone();
                pth.push(REVERT_HEAD);
                Some(pth)
            }
            _ => None,
        } {
            subject = std::fs::read_to_string(path_to_read_subject)
                .expect("Should have been able to read the file")
                .replace('\n', "");
        }
        State::new(state, subject)
    }
}

#[derive(Debug, Clone)]
pub struct Head {
    pub oid: Oid,
    // TODO! get rid of it!
    // pub branch_name: Option<BranchName>,
    pub is_upstream: bool,
    pub log_message: String,
    pub raw_message: String,
    pub view: View,
    pub commit_dt: DateTime<FixedOffset>,
    pub branch: Option<BranchData>,
}

impl Head {
    pub fn new(commit: &Commit, is_upstream: bool) -> Self {
        Self {
            oid: commit.id(),
            is_upstream,
            // branch_name: None,
            log_message: commit.log_message(),
            raw_message: commit.raw_message(),
            view: View::new(),
            commit_dt: commit.dt(),
            branch: None,
        }
    }
    pub fn set_branch(&mut self, branch: BranchData) {
        // self.branch_name = Some(branch.name.clone());
        self.branch.replace(branch);
    }
}

pub fn get_head(path: PathBuf) -> Result<Head, Error> {
    let repo = Repository::open(path)?;
    let head_ref = repo.head()?;
    let ob = head_ref.peel(ObjectType::Commit)?;
    let commit = ob.peel_to_commit()?;
    let mut head = Head::new(&commit, false);
    if head_ref.is_branch() {
        if let Some(branch_data) =
            BranchData::from_branch(&Branch::wrap(head_ref), git2::BranchType::Local)?
        {
            head.set_branch(branch_data);
        }
    }
    Ok(head)
}

pub fn get_upstream(path: PathBuf) -> Result<Head, Error> {
    trace!("get upstream");
    let repo = Repository::open(path)?;
    let head_ref = repo.head()?;
    if !head_ref.is_branch() {
        return Err(git2::Error::from_str(
            "Head ref is not branch in get_upstream",
        ));
    }
    let branch = Branch::wrap(head_ref);
    if let Ok(upstream) = branch.upstream() {
        let upstream_ref = upstream.get();
        let ob = upstream_ref.peel(ObjectType::Commit)?;
        let commit = ob.peel_to_commit()?;
        let mut new_upstream = Head::new(&commit, true);
        if let Some(branch_data) = BranchData::from_branch(&upstream, git2::BranchType::Remote)? {
            new_upstream.set_branch(branch_data);
        }
        return Ok(new_upstream);
    }
    Err(git2::Error::from_str("No upstream yet"))
}

pub const CHERRY_PICK_HEAD: &str = "CHERRY_PICK_HEAD";
pub const REVERT_HEAD: &str = "REVERT_HEAD";

pub fn get_current_repo_status(
    current_path: Option<PathBuf>,
    sender: Sender<crate::Event>,
) -> Result<(), Error> {
    // path could came from command args or from choosing path
    // by user
    let path = {
        if let Some(path) = current_path {
            path
        } else {
            std::env::current_exe().expect("cant't get exe path")
        }
    };
    let repo = Repository::discover(path.clone())?;
    let path = PathBuf::from(repo.path());
    sender
        .send_blocking(crate::Event::CurrentRepo(path.clone()))
        .expect("Could not send through channel");

    // get state
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            let repo = Repository::open(path.clone()).expect("can't open repo");
            let state = repo.state();
            let state = State::from_git_state(state, path.clone());
            sender
                .send_blocking(crate::Event::State(state))
                .expect("Could not send through channel");
        }
    });

    // get HEAD
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            match get_head(path.clone()) {
                Ok(head) => {
                    sender
                        .send_blocking(crate::Event::Head(Some(head)))
                        .expect("Could not send through channel");
                }
                Err(err) => {
                    error!("cant get Head {:?}", err);
                    sender
                        .send_blocking(crate::Event::Head(None))
                        .expect("Could not send through channel");
                }
            }
            match get_upstream(path.clone()) {
                Ok(head) => {
                    sender
                        .send_blocking(crate::Event::Upstream(Some(head)))
                        .expect("Could not send through channel");
                }
                Err(err) => {
                    error!("cant get Upstream {:?}", err);
                    sender
                        .send_blocking(crate::Event::Upstream(None))
                        .expect("Could not send through channel");
                }
            }
        }
    });

    // get branches
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            let branches = branch::get_branches(path).expect("cant get branches");
            sender
                .send_blocking(crate::Event::Branches(branches))
                .expect("Could not send through channel");
        }
    });

    // get staged
    gio::spawn_blocking({
        // get_staged
        let sender = sender.clone();
        let path = path.clone();
        move || {
            let repo = Repository::open(path).expect("can't open repo");
            let git_diff = {
                if let Ok(ob) = repo.revparse_single("HEAD^{tree}") {
                    let tree = repo.find_tree(ob.id()).expect("no working tree");
                    repo.diff_tree_to_index(Some(&tree), None, Some(&mut make_diff_options()))
                        .expect("can't get diff tree to index")
                } else {
                    repo.diff_tree_to_index(None, None, Some(&mut make_diff_options()))
                        .expect("can't get diff tree to index")
                }
            };
            let diff = make_diff(&git_diff, DiffKind::Staged);
            sender
                .send_blocking(crate::Event::Staged(if diff.is_empty() {
                    None
                } else {
                    Some(diff)
                }))
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
            stash::list(path, sender);
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

    // bugs in libgit2
    // https://github.com/libgit2/libgit2/issues/6232
    // this one is for staging killed hunk
    // https://github.com/libgit2/libgit2/issues/6643

    // get conflicted
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            merge::try_finalize_conflict(path, sender, None).unwrap();
        }
    });

    get_unstaged(&repo, sender.clone());

    Ok(())
}

fn get_unstaged(repo: &git2::Repository, sender: Sender<crate::Event>) {
    let git_diff = repo
        .diff_index_to_workdir(None, Some(&mut make_diff_options()))
        .unwrap();
    let diff = make_diff(&git_diff, DiffKind::Unstaged);
    sender
        .send_blocking(crate::Event::Unstaged(if diff.is_empty() {
            None
        } else {
            Some(diff)
        }))
        .expect("Could not send through channel");
}

pub fn get_untracked(path: PathBuf, sender: Sender<crate::Event>) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut opts = make_diff_options();

    let opts = opts.include_untracked(true);

    let git_diff = {
        if let Ok(ob) = repo.revparse_single("HEAD^{tree}") {
            let tree = repo.find_tree(ob.id()).expect("cant find tree");
            repo.diff_tree_to_workdir_with_index(Some(&tree), Some(opts))
                .expect("can't get diff")
        } else {
            repo.diff_tree_to_workdir_with_index(None, Some(opts))
                .expect("can't get diff")
        }
    };

    let mut untracked = Diff::new(DiffKind::Untracked);

    let _ = git_diff.foreach(
        &mut |delta: DiffDelta, _num| {
            if delta.status() == Delta::Untracked {
                let path: PathBuf = delta.new_file().path().unwrap().into();
                let mut file = File::new(DiffKind::Untracked);
                file.path = path;
                untracked.push_file(file);
            }
            true
        },
        None,
        None,
        None,
    );
    if untracked.is_empty() {
        sender
            .send_blocking(crate::Event::Untracked(None))
            .expect("Could not send through channel");
    } else {
        sender
            .send_blocking(crate::Event::Untracked(Some(untracked)))
            .expect("Could not send through channel");
    }
}

pub fn make_diff(git_diff: &GitDiff, kind: DiffKind) -> Diff {
    let mut diff = Diff::new(kind);
    let mut current_file = File::new(kind);
    let mut current_hunk = Hunk::new(kind);
    let mut prev_line_kind = LineKind::None;
    let ss = SyntaxSet::load_defaults_newlines(); // note we load the version with newlines

    let _res = git_diff.print(DiffFormat::Patch, |diff_delta, o_diff_hunk, diff_line| {
        let status = diff_delta.status();
        if status == Delta::Conflicted && (kind == DiffKind::Staged || kind == DiffKind::Unstaged) {
            return true;
        }
        let file: DiffFile = match status {
            Delta::Modified | Delta::Conflicted => diff_delta.new_file(),
            Delta::Deleted => diff_delta.old_file(),
            Delta::Added => match diff.kind {
                DiffKind::Staged | DiffKind::Commit => diff_delta.new_file(),
                DiffKind::Unstaged => {
                    todo!("delta added in unstaged {:?}", diff_delta)
                }
                DiffKind::Conflicted => {
                    todo!("delta added in conflicted {:?}", diff_delta)
                }
                DiffKind::Untracked => {
                    panic!("untracked is not used with git diffs")
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

        let current_path = file.path().unwrap();
        let mut syntax_parse_state: Option<ParseState> = None;
        if let Ok(Some(syntax)) = ss.find_syntax_for_file(current_path) {
            syntax_parse_state.replace(ParseState::new(syntax));
        }
        // build up diff structure
        if current_file.path.capacity() == 0 {
            // init new file
            current_file = File::from_diff_file(&file, kind, status);
        }
        if current_file.path != current_path {
            // go to next file
            // push current_hunk to file and init new empty hunk
            current_file.push_hunk(current_hunk.clone());
            current_hunk = Hunk::new(kind);
            // push current_file to diff and change to new file
            diff.push_file(current_file.clone());
            current_file = File::from_diff_file(&file, kind, status);
        }
        if let Some(diff_hunk) = o_diff_hunk {
            let hh = Hunk::get_header_from(&diff_hunk);
            if current_hunk.header.is_empty() {
                // init hunk
                prev_line_kind = LineKind::None;
                current_hunk.fill_from_git_hunk(&diff_hunk)
            }
            if current_hunk.header != hh {
                // go to next hunk
                prev_line_kind = LineKind::None;
                current_file.push_hunk(current_hunk.clone());
                current_hunk = Hunk::new(kind);
                current_hunk.fill_from_git_hunk(&diff_hunk)
            }
            prev_line_kind =
                current_hunk.push_line(&diff_line, prev_line_kind.clone(), syntax_parse_state, &ss);
        } else {
            // this is file header line.
            prev_line_kind =
                current_hunk.push_line(&diff_line, prev_line_kind.clone(), syntax_parse_state, &ss)
        }

        true
    });
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
    file_path: Option<PathBuf>,
    sender: Sender<crate::Event>,
) -> Result<(), Error> {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    if let Some(file_path) = file_path {
        let pth = path.parent().unwrap().join(&file_path);
        if pth.is_file() {
            index.add_path(file_path.as_path()).expect("cant add path");
        } else if pth.is_dir() {
            index
                .add_all([file_path], git2::IndexAddOption::DEFAULT, None)
                .expect("cant add path");
        } else if pth.is_symlink() {
            return Err(Error::from_str(&format!("symlink path {:?}", pth)));
        } else {
            return Err(Error::from_str(&format!("unknown path {:?}", pth)));
        }
    } else {
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .expect("cant add path");
    }

    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender).expect("cant get status");
    Ok(())
}

pub fn stage_via_apply(
    path: PathBuf,
    file_path: Option<PathBuf>,
    hunk_header: Option<String>,
    subject: crate::StageOp,
    sender: Sender<crate::Event>,
) -> Result<(), Error> {
    info!(
        "stage via apply {:?} {:?} {:?}",
        file_path, hunk_header, subject
    );
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = Repository::open(path.clone())?;

    let mut opts = make_diff_options();

    if let Some(file_path) = &file_path {
        opts.pathspec(file_path.clone());
    }

    let git_diff = match subject {
        crate::StageOp::Stage => repo.diff_index_to_workdir(None, Some(&mut opts))?,
        crate::StageOp::Unstage => {
            opts.reverse(true);
            if let Ok(ob) = repo.revparse_single("HEAD^{tree}") {
                let current_tree = repo.find_tree(ob.id()).expect("no working tree");
                repo.diff_tree_to_index(Some(&current_tree), None, Some(&mut opts))?
            } else {
                repo.diff_tree_to_index(None, None, Some(&mut opts))?
            }
        }
        crate::StageOp::Kill => {
            opts.reverse(true);
            repo.diff_index_to_workdir(None, Some(&mut opts))?
        }
    };

    let mut options = ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if let Some(hunk_header) = &hunk_header {
            if let Some(dh) = odh {
                let header = Hunk::get_header_from(&dh);
                return match subject {
                    crate::StageOp::Stage => hunk_header == &header,
                    crate::StageOp::Unstage => hunk_header == &Hunk::reverse_header(&header),
                    crate::StageOp::Kill => {
                        let reversed = Hunk::reverse_header(&header);
                        hunk_header == &reversed
                    }
                };
            }
        }
        true
    });

    options.delta_callback(|odd| -> bool {
        if let Some(file_path) = &file_path {
            if let Some(dd) = odd {
                let path: PathBuf = dd.new_file().path().unwrap().into();
                return file_path == &path;
            }
        }
        true
    });
    let apply_location = match subject {
        crate::StageOp::Stage | crate::StageOp::Unstage => ApplyLocation::Index,
        crate::StageOp::Kill => ApplyLocation::WorkDir,
    };

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");
    repo.apply(&git_diff, apply_location, Some(&mut options))?;

    Ok(())
}

pub struct DeferRefresh {
    pub path: PathBuf,
    pub sender: Sender<crate::Event>,
    pub update_status: bool,
    pub unlock_monitors: bool,
}

impl DeferRefresh {
    pub fn new(
        path: PathBuf,
        sender: Sender<crate::Event>,
        update_status: bool,
        unlock_monitors: bool,
    ) -> Self {
        Self {
            path,
            sender,
            update_status,
            unlock_monitors,
        }
    }
}

impl Drop for DeferRefresh {
    fn drop(&mut self) {
        if self.update_status {
            gio::spawn_blocking({
                let path = self.path.clone();
                let sender = self.sender.clone();
                move || {
                    get_current_repo_status(Some(path), sender).expect("cant get status");
                }
            });
        }
        if self.unlock_monitors {
            self.sender
                .send_blocking(crate::Event::LockMonitors(false))
                .expect("can send through channel");
        }
    }
}

pub fn reset_hard(
    path: PathBuf,
    ooid: Option<Oid>,
    sender: Sender<crate::Event>,
) -> Result<bool, Error> {
    let repo = Repository::open(path.clone())?;
    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());

    let ob = if let Some(oid) = ooid {
        repo.find_object(oid, Some(ObjectType::Commit))?
    } else {
        head_ref.peel(ObjectType::Commit)?
    };

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");

    let result = repo.reset(&ob, ResetType::Hard, None).err();

    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");
    if let Some(error) = result {
        return Err(error);
    }
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender).expect("cant get status");
        }
    });
    Ok(true)
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

// TODO! get rid of it. just call get_current_repo_status!
pub fn track_changes(
    path: PathBuf,
    file_path: PathBuf,
    //has_conflicted: bool,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let index = repo.index().expect("cant get index");
    let file_path = file_path
        .into_os_string()
        .into_string()
        .expect("wrong path");

    let mut status_opts = StatusOptions::new();
    status_opts.include_unmodified(false);
    let mut is_tracked = false;
    for entry in index.iter() {
        if file_path == String::from_utf8_lossy(&entry.path) {
            is_tracked = true;
            break;
        }
    }
    // conflicts could be resolved right in this file change manually
    // but it need to update conflicted anyways if we had them!
    // see else below!
    if index.has_conflicts() {
        let conflicts = index.conflicts().expect("cant get conflicts");
        for conflict in conflicts.flatten() {
            if let Some(our) = conflict.our {
                let conflict_path = String::from_utf8(our.path.clone()).unwrap();
                if file_path == conflict_path {
                    let cleanup_result = merge::try_finalize_conflict(
                        path.clone(),
                        sender.clone(),
                        Some(file_path.clone().into()),
                    );
                    if cleanup_result.is_err() {
                        debug!(
                            "error whyle trying finalize conflict {:?}",
                            cleanup_result.err()
                        );
                    }
                }
            }
        }
    }
    if is_tracked {
        get_unstaged(&repo, sender.clone());
    } else {
        get_untracked(path, sender);
    }
}

pub fn abort_rebase(path: PathBuf, sender: Sender<crate::Event>) -> Result<(), Error> {
    let _updater = DeferRefresh::new(path.clone(), sender, true, true);

    let repo = Repository::open(path)?;

    let mut builder = CheckoutBuilder::new();
    builder.safe().allow_conflicts(true);

    let mut rebase_options = RebaseOptions::new();
    let rebase_options = rebase_options.checkout_options(builder);

    let mut rebase = repo.open_rebase(Some(rebase_options))?;
    rebase.abort()?;
    Ok(())
}

pub fn continue_rebase(path: PathBuf, sender: Sender<crate::Event>) -> Result<(), Error> {
    let _updater = DeferRefresh::new(path.clone(), sender, true, true);

    let repo = Repository::open(path)?;

    let mut builder = CheckoutBuilder::new();
    builder.safe().allow_conflicts(true);

    let mut rebase_options = RebaseOptions::new();
    let rebase_options = rebase_options.checkout_options(builder);

    let mut rebase = repo.open_rebase(Some(rebase_options))?;

    let me = repo.signature()?;
    rebase.commit(None, &me, None)?;
    loop {
        if let Some(result) = rebase.next() {
            debug!("rebase result {:?}", result);
            rebase.commit(None, &me, None)?;
        } else {
            rebase.finish(Some(&me))?;
            break;
        }
    }
    Ok(())
}

pub fn rebase(
    path: PathBuf,
    upstream: Oid,
    _onto: Option<Oid>,
    sender: Sender<crate::Event>,
) -> Result<bool, Error> {
    let _defer = DeferRefresh::new(path.clone(), sender, true, true);

    let repo = Repository::open(path)?;
    let upstream_commit = repo.find_annotated_commit(upstream)?;

    let mut builder = CheckoutBuilder::new();
    builder.safe().allow_conflicts(true);

    let mut rebase_options = RebaseOptions::new();
    let rebase_options = rebase_options.checkout_options(builder);

    let mut rebase = repo.rebase(None, Some(&upstream_commit), None, Some(rebase_options))?;
    debug!(
        "THATS REBASE {:?} {:?}",
        rebase.operation_current(),
        rebase.len()
    );
    let me = repo.signature()?;
    loop {
        if let Some(result) = rebase.next() {
            debug!("MAIN got result in rebase ..... {:?}", result);
            let op = result?;
            debug!("MAIN rebase op {:?} {:?}", op.id(), op.kind());
            rebase.commit(None, &me, None)?;
        } else {
            debug!("MAIN rebase is over!");
            rebase.finish(Some(&me))?;
            break;
        }
    }
    Ok(true)
}
