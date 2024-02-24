use crate::gio;
// use crate::glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;
use chrono::prelude::*;
use chrono::{
    DateTime, FixedOffset, LocalResult, TimeZone,
};
use ffi::OsString;
use git2::{
    ApplyLocation, ApplyOptions, Branch,
    BranchType, Commit, Cred, CredentialType,
    Delta, Diff as GitDiff, DiffDelta, DiffFile,
    DiffFormat, DiffHunk, DiffLine, DiffLineType,
    DiffOptions, Error, ObjectType, Oid,
    PushOptions, Reference, RemoteCallbacks,
    Repository,
};
use log::{debug, trace};
use regex::Regex;
use std::cmp::Ordering;
use std::{env, ffi, iter::zip, path, str};

fn get_current_repo(
    mut path_buff: path::PathBuf,
) -> Result<Repository, String> {
    let path = path_buff.as_path();
    Repository::open(path).or_else(|error| {
        println!("err while open repo {:?}", error);
        if !path_buff.pop() {
            return Err(
                "no repoitory found".to_string()
            );
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
            content: String::from(
                str::from_utf8(l.content())
                    .unwrap(),
            )
            .replace("\r\n", "")
            .replace('\n', ""),
        };
    }
    // line
    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone
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
        }
    }

    pub fn get_header_from(
        dh: &DiffHunk,
    ) -> String {
        String::from(
            str::from_utf8(dh.header()).unwrap(),
        )
        .replace("\r\n", "")
        .replace('\n', "")
    }

    pub fn reverse_header(
        header: String,
    ) -> String {
        // "@@ -1,3 +1,7 @@" -> "@@ -1,7 +1,3 @@"
        // "@@ -20,10 +24,11 @@ STAGING LINE..." -> "@@ -24,11 +20,10 @@ STAGING LINE..."
        // "@@ -54,7 +59,6 @@ do not call..." -> "@@ -59,6 +54,7 @@ do not call..."
        let re = Regex::new(r"@@ [+-]([0-9].*,[0-9]*) [+-]([0-9].*,[0-9].*) @@").unwrap();
        if let Some((whole, [nums1, nums2])) = re
            .captures_iter(&header)
            .map(|c| c.extract())
            .next()
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
    pub fn delta_in_lines(&self) -> i32 {
        // returns how much lines this hunk
        // will add to file (could be negative when lines are deleted)
        // self.new_start + self.new_lines - self.old_start - self.old_lines
        self.lines
            .iter()
            .map(|l| match l.origin {
                DiffLineType::Addition => 1,
                DiffLineType::Deletion => -1,
                _ => 0,
            })
            .sum()
    }

    pub fn related_to_other(
        &self,
        other: &Hunk,
    ) -> Related {
        // returns how this hunk is related to other hunk (in file)
        debug!(
            "related to other self.new_start {:?} self.new_lines {:?}
                  other.new_start {:?} other.new_lines {:?}",
            self.new_start, self.new_lines, other.new_start, other.new_lines
        );
        if self.new_start < other.new_start
            && self.new_start + self.new_lines
                < other.new_start
        {
            debug!("before");
            return Related::Before;
        }
        if self.new_start < other.new_start
            && self.new_start + self.new_lines
                > other.new_start
        {
            debug!("overlap");
            return Related::OverlapBefore;
        }
        if self.new_start == other.new_start
            && self.new_lines == other.new_lines
        {
            debug!("matched");
            return Related::Matched;
        }
        if self.new_start > other.new_start
            && self.new_start + self.new_lines
                < other.new_start + other.new_lines
        {
            debug!("overlap");
            return Related::OverlapAfter;
        }
        if self.new_start > other.new_start
            && self.new_start
                > other.new_start + other.new_lines
        {
            debug!("after");
            return Related::After;
        }
        panic!(
            "unknown case self.new_start {:?} self.new_lines {:?}
                  other.new_start {:?} other.new_lines {:?}",
            self.new_start,
            self.new_lines,
            other.new_start,
            other.new_lines
        );
        Related::After
    }

    pub fn title(&self) -> String {
        self.header.to_string()
    }

    // Hunk
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
            DiffLineType::FileHeader
            | DiffLineType::HunkHeader
            | DiffLineType::Binary => {}
            _ => self.lines.push(line),
        }
    }
    pub fn enrich_view(&mut self, other: &Hunk) {
        if self.lines.len() != other.lines.len() {
            // so :) what todo?
            panic!(
                "lines length are not the same {:?} {:?}",
                self.lines.len(),
                other.lines.len()
            );
        }
        for pair in
            zip(&mut self.lines, &other.lines)
        {
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

    pub fn enrich_view(
        &mut self,
        other: &mut File,
    ) {
        // used to maintain view state in existent hunks
        // there are 2 cases
        // 1. side from which hunks are moved out (eg unstaged during staging)
        // this one is simple, cause self.hunks and other.hunks are the same length.
        // just transfer views between them in order.
        // 2. side on which hunks are receiving new hunk (eg staged hunks during staging)
        // Like stage some hunks in file and then stage some more hunks to the same file!
        // New hunks could break old ones:
        // lines become changed and headers will be changed also
        // case 1.
        if self.hunks.len() == other.hunks.len() {
            for pair in
                zip(&mut self.hunks, &other.hunks)
            {
                pair.0.view =
                    pair.1.transfer_view();
                pair.0.enrich_view(pair.1);
            }
            return;
        }
        //case 2.
        // all hunks are ordered
        for hunk in self.hunks.iter_mut() {
            trace!("outer cycle");
            // go "insert" (no real insertion is required) every new hunk in old_hunks.
            // that new hunk which will be overlapped or before or after old_hunk - those will have
            // new view. (i believe overlapping is not possible)
            // insertion means - shift all rest old hunks according to lines delta
            // and only hunks which match exactly will be enriched by views of old
            // hunks. line_no actually does not matter - they will be shifted.
            // but props like rendered, expanded will be copied for smoother rendering
            for other_hunk in other.hunks.iter_mut()
            {
                trace!(
                    "inner cycle for other_hunk"
                );
                match hunk
                    .related_to_other(other_hunk)
                {
                    // me relative to other hunks
                    Related::Before => {
                        trace!(
                            "choose new hunk start"
                        );
                        trace!(
                            "just shift old hunk by my lines {:?}",
                            hunk.delta_in_lines()
                        );
                        // my lines - means diff in lines between my other_hunk and my new hunk
                        other_hunk.new_start =
                            ((other_hunk.new_start
                                as i32)
                                + hunk
                                    .delta_in_lines(
                                    ))
                                as u32;
                    }
                    Related::OverlapBefore => {
                        // insert diff betweeen old and new view
                        todo!("extend hunk by start diff");
                        // hm. old_lines are not included at all...
                        // other_hunk.new_lines += other_hunk.new_start - hunk.new_start;
                        // other_hunk.new_start = hunk.new_start;
                    }
                    Related::Matched => {
                        trace!("enrich!");
                        hunk.view = other_hunk
                            .transfer_view();
                        hunk.enrich_view(
                            other_hunk,
                        );
                        // no furtger processing
                        break;
                    }
                    Related::OverlapAfter => {
                        todo!(
                            "choose old hunk start"
                        );
                        // trace!("extend hunk by start diff");
                        // other_hunk.new_lines += hunk.new_start - other_hunk.new_start;
                        // hm. old lines are not present at all?
                    }
                    Related::After => {
                        trace!("nothing todo!");
                        // nothing to do
                    }
                }
            }
        }
    }

    // Dile
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

    pub fn enrich_view(
        &mut self,
        other: &mut Diff,
    ) {
        for file in &mut self.files {
            for of in &mut other.files {
                if file.path == of.path {
                    file.view = of.transfer_view();
                    file.enrich_view(of);
                }
            }
        }
    }
}

pub fn get_cwd_repo(
    sender: Sender<crate::Event>,
) -> Repository {
    let path_buff = env::current_exe()
        .expect("cant't get exe path");
    let repo = get_current_repo(path_buff)
        .expect("cant't get repo for current exe");
    let path = OsString::from(repo.path());
    sender
        .send_blocking(crate::Event::CurrentRepo(
            path.clone(),
        ))
        .expect("Could not send through channel");
    repo
}

#[derive(Debug, Clone, Default)]
pub struct Head {
    pub commit: String,
    pub branch: String,
    pub view: View,
    pub remote: bool,
}

impl Head {
    pub fn new(
        branch: &Branch,
        commit: &Commit,
    ) -> Self {
        Self {
            branch: String::from(
                branch.name().unwrap().unwrap(),
            ),
            commit: commit_string(commit),
            view: View::new_markup(),
            remote: false,
        }
    }
    pub fn enrich_view(&mut self, other: &Head) {
        self.view = other.transfer_view();
    }
    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone.dirty = true;
        clone
    }
}

pub fn commit_string(c: &Commit) -> String {
    format!(
        "{} {}",
        &c.id().to_string()[..7],
        c.message().unwrap_or("").replace("\n", "")
    )
}

pub fn commit_dt(
    c: &Commit,
) -> DateTime<FixedOffset> {
    let tz = FixedOffset::east_opt(
        c.time().offset_minutes() * 60,
    )
    .unwrap();
    match tz.timestamp_opt(c.time().seconds(), 0) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        _ => todo!("not implemented"),
    }
}

pub fn get_head(
    path: OsString,
    old_head: Option<Head>,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path)
        .expect("can't open repo");
    let head_ref =
        repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(ObjectType::Commit)
        .expect("can't get commit from ref!");
    let commit = ob
        .peel_to_commit()
        .expect("can't get commit from ob!");
    let branch = Branch::wrap(head_ref);
    let mut new_head = Head::new(&branch, &commit);
    if let Some(oh) = old_head {
        new_head.enrich_view(&oh);
    }
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");
}

pub fn get_upstream(
    path: OsString,
    old_upstream: Option<Head>,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path)
        .expect("can't open repo");
    let head_ref =
        repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let branch = Branch::wrap(head_ref);
    if let Ok(upstream) = branch.upstream() {
        let upstream_ref = upstream.get();
        let ob = upstream_ref
            .peel(ObjectType::Commit)
            .expect("can't get commit from ref!");
        let commit = ob
            .peel_to_commit()
            .expect("can't get commit from ob!");
        let mut new_upstream =
            Head::new(&upstream, &commit);
        new_upstream.remote = true;
        if let Some(ou) = old_upstream {
            new_upstream.enrich_view(&ou);
        }
        sender
            .send_blocking(crate::Event::Upstream(
                new_upstream,
            ))
            .expect(
                "Could not send through channel",
            );
    } else {
        // todo!("some branches could contain only pushRemote, but no
        //       origin. There will be no upstream then. It need to lookup
        //       pushRemote in config and check refs/remotes/<origin>/")
    };
}

pub fn get_current_repo_status(
    current_path: Option<OsString>,
    sender: Sender<crate::Event>,
) {
    let (repo, path) = {
        if let Some(path) = current_path {
            let repo =
                Repository::open(path.clone())
                    .expect("can't open repo");
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
            get_head(
                path.clone(),
                None,
                sender.clone(),
            );
            get_upstream(path, None, sender);
        }
    });

    // get staged
    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            let repo = Repository::open(path)
                .expect("can't open repo");
            let ob = repo
                .revparse_single("HEAD^{tree}")
                .expect("fail revparse");
            let current_tree = repo
                .find_tree(ob.id())
                .expect("no working tree");
            let git_diff = repo
                .diff_tree_to_index(
                    Some(&current_tree),
                    None,
                    None,
                )
                .expect(
                    "can't get diff tree to index",
                );
            let diff = make_diff(git_diff);
            sender
                .send_blocking(crate::Event::Staged(diff))
                .expect("Could not send through channel");
        }
    });
    // get unstaged
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant' get diff index to workdir");
    let diff = make_diff(git_diff);
    sender
        .send_blocking(crate::Event::Unstaged(diff))
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
                Delta::Modified => {
                    // all ok. code below will works
                    diff_delta.new_file()
                }
                Delta::Deleted => {
                    // all ok. code below will works
                    diff_delta.old_file()
                }
                _ => {
                    todo!()
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
                current_file =
                    File::from_diff_file(&file);
            }
            if current_file.id != oid {
                // go to next file
                // push current_hunk to file and init new empty hunk
                current_file.push_hunk(
                    current_hunk.clone(),
                );
                current_hunk = Hunk::new();
                // push current_file to diff and change to new file
                diff.files
                    .push(current_file.clone());
                current_file =
                    File::from_diff_file(&file);
            }
            if let Some(diff_hunk) = o_diff_hunk {
                let hh = Hunk::get_header_from(
                    &diff_hunk,
                );
                let old_start =
                    diff_hunk.old_start();
                let old_lines =
                    diff_hunk.old_lines();
                let new_start =
                    diff_hunk.new_start();
                let new_lines =
                    diff_hunk.new_lines();
                if current_hunk.header.is_empty() {
                    // init hunk
                    current_hunk.header =
                        hh.clone();
                    current_hunk.old_start =
                        old_start;
                    current_hunk.old_lines =
                        old_lines;
                    current_hunk.new_start =
                        new_start;
                    current_hunk.new_lines =
                        new_lines;
                }
                if current_hunk.header != hh {
                    // go to next hunk
                    current_file.push_hunk(
                        current_hunk.clone(),
                    );
                    current_hunk = Hunk::new();
                    current_hunk.header =
                        hh.clone();
                    current_hunk.old_start =
                        old_start;
                    current_hunk.old_lines =
                        old_lines;
                    current_hunk.new_start =
                        new_start;
                    current_hunk.new_lines =
                        new_lines;
                }
                current_hunk.push_line(
                    Line::from_diff_line(
                        &diff_line,
                    ),
                );
            } else {
                // this is file header line.
                current_hunk.push_line(
                    Line::from_diff_line(
                        &diff_line,
                    ),
                )
            }

            true
        },
    );
    if !current_hunk.header.is_empty() {
        current_file.push_hunk(current_hunk);
    }
    if !current_file.path.is_empty() {
        diff.files.push(current_file);
    }
    diff
}

pub fn stage_via_apply(
    mut unstaged: Option<Diff>,
    mut staged: Option<Diff>,
    is_staging: bool,
    path: OsString,
    filter: ApplyFilter,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone())
        .expect("can't open repo");
    // get actual diff for repo
    let git_diff = {
        if is_staging {
            repo.diff_index_to_workdir(None, None)
                .expect("can't get diff")
        } else {
            let ob = repo
                .revparse_single("HEAD^{tree}")
                .expect("fail revparse");
            let current_tree = repo
                .find_tree(ob.id())
                .expect("no working tree");
            repo.diff_tree_to_index(
                Some(&current_tree),
                None,
                Some(
                    DiffOptions::new()
                        .reverse(true),
                ),
            )
            .expect("can't get diff")
        }
    };
    // filter selected files and hunks
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
                    filter.hunk_header
                        == Hunk::reverse_header(
                            header,
                        )
                }
            };
        }
        false
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let status = dd.status();
            debug!("delta_callback in stage_via_apply status {:?}", status);
            let new_file = dd.new_file();
            let file =
                File::from_diff_file(&new_file);
            let path = file
                .path
                .into_string()
                .unwrap();
            return filter.file_path == path;
        }
        todo!("diff without delta");
        true
    });
    repo.apply(
        &git_diff,
        ApplyLocation::Index,
        Some(&mut options),
    )
    .expect("can't apply patch");

    // staged changes
    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            let repo = Repository::open(path)
                .expect("can't open repo");
            let ob = repo
                .revparse_single("HEAD^{tree}")
                .expect("fail revparse");
            let current_tree = repo
                .find_tree(ob.id())
                .expect("no working tree");
            let git_diff = repo
                .diff_tree_to_index(
                    Some(&current_tree),
                    None,
                    None,
                )
                .expect(
                    "can't get diff tree to index",
                );
            let mut diff = make_diff(git_diff);
            if let Some(s) = &mut staged {
                diff.enrich_view(s);
            }
            sender
                .send_blocking(crate::Event::Staged(diff))
                .expect("Could not send through channel");
        }
    });
    // unstaged changes
    let git_diff = repo
        .diff_index_to_workdir(None, None)
        .expect("cant get diff_index_to_workdir");
    let mut diff = make_diff(git_diff);
    if let Some(u) = &mut unstaged {
        diff.enrich_view(u);
    }
    sender
        .send_blocking(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}

pub fn commit_staged(
    head: Option<Head>,
    upstream: Option<Head>,
    path: OsString,
    message: String,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone())
        .expect("can't open repo");
    let me = repo
        .signature()
        .expect("can't get signature");
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
    let tree = repo
        .find_tree(tree_oid)
        .expect("can't find tree");
    let ob = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse");
    let parent = repo
        .find_commit(ob.id())
        .expect("can't find commit");
    repo.commit(
        Some("HEAD"),
        &me,
        &me,
        &message,
        &tree,
        &[&parent],
    )
    .expect("can't commit");

    // update staged changes
    let ob = repo
        .revparse_single("HEAD^{tree}")
        .expect("fail revparse");
    let current_tree = repo
        .find_tree(ob.id())
        .expect("no working tree");
    let git_diff = repo
        .diff_tree_to_index(
            Some(&current_tree),
            None,
            None,
        )
        .expect("can't get diff tree to index");
    sender
        .send_blocking(crate::Event::Staged(
            make_diff(git_diff),
        ))
        .expect("Could not send through channel");
    get_head(path, head, sender)
}

pub fn push(
    upstream: Option<Head>,
    path: OsString,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone())
        .expect("can't open repo");
    let head_ref =
        repo.head().expect("can't get head");
    debug!("head ref name {:?}", head_ref.name());
    assert!(head_ref.is_branch());
    let mut remote = repo
        .find_remote("origin") // TODO harcode
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
            get_upstream(
                path.clone(),
                upstream.clone(),
                sender.clone(),
            );
            // todo what is this?
            true
        }
    });
    callbacks.push_update_reference({
        move |ref_name, opt_status| {
            trace!(
                "push update red {:?}",
                ref_name
            );
            trace!("push status {:?}", opt_status);
            // TODO - if status is not None
            // it will need to interact with user
            assert!(opt_status.is_none());
            Ok(())
        }
    });
    callbacks.credentials(|url, username_from_url, allowed_types| {
        debug!("auth credentials url {:?}", url);
        // "git@github.com:aganzha/stage.git"
        trace!("auth credentials username_from_url {:?}", username_from_url);
        trace!("auth credentials allowed_types url {:?}", allowed_types);
        if allowed_types.contains(CredentialType::SSH_KEY) {
            return Cred::ssh_key_from_agent(
                username_from_url.unwrap()
            )
        }
        todo!("implement other types");
    });
    opts.remote_callbacks(callbacks);
    remote
        .push(
            &[head_ref.name().unwrap()],
            Some(&mut opts),
        )
        .expect("cant push to remote");
}

#[derive(Debug, Clone)]
pub struct BranchData {
    pub name: String,
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
            branch_type: BranchType::Local,
            oid: Oid::zero(),
            commit_string: String::from(""),
            is_head: false,
            upstream_name: None,
            commit_dt:
                DateTime::<FixedOffset>::MIN_UTC
                    .into(),
        }
    }
}

impl BranchData {
    pub fn new(
        branch: Branch,
        branch_type: BranchType,
    ) -> Self {
        let name = branch
            .name()
            .unwrap()
            .unwrap()
            .to_string();
        let mut upstream_name: Option<String> =
            None;
        if let Ok(upstream) = branch.upstream() {
            upstream_name = Some(
                upstream
                    .name()
                    .unwrap()
                    .unwrap()
                    .to_string(),
            );
        }
        let is_head = branch.is_head();
        let bref = branch.get();
        let ob = bref
            .peel(ObjectType::Commit)
            .expect("can't get commit from ref!");
        let commit = ob
            .peel_to_commit()
            .expect("can't get commit from ob!");
        let commit_string = commit_string(&commit);
        let oid = branch.get().target().unwrap();

        let commit_dt = commit_dt(&commit);
        BranchData {
            name,
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
    let repo = Repository::open(path.clone())
        .expect("can't open repo");
    let mut result = Vec::new();
    let branches = repo
        .branches(None)
        .expect("can't get branches");
    branches.for_each(|item| {
        let (branch, branch_type) = item.unwrap();
        result.push(BranchData::new(
            branch,
            branch_type,
        ));
    });
    result.sort_by(|a, b| {
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
        return b.commit_dt.cmp(&a.commit_dt);
    });
    result
}
