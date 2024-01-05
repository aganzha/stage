use crate::gio;
use crate::glib::Sender;

use ffi::OsString;
use git2::{
    ApplyLocation, ApplyOptions, Diff as GitDiff, DiffFile, DiffFormat, DiffHunk, DiffLine,
    DiffLineType, Oid, Repository,
};
use std::{env, ffi, path, str};

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
    pub tags: Vec<String>,
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
    pub kind: LineKind,
}

impl Line {
    pub fn new() -> Self {
        Self {
            view: View::new(),
            origin: DiffLineType::HunkHeader,
            content: String::new(),
            kind: LineKind::File,
        }
    }
    pub fn from_diff_line(l: &DiffLine, k: LineKind) -> Self {
        return Self {
            view: View::new(),
            origin: l.origin_value(),
            content: String::from(str::from_utf8(l.content()).unwrap())
                .replace("\r\n", "")
                .replace('\n', ""),
            kind: k,
        };
    }
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub view: View,
    pub header: String,
    pub lines: Vec<Line>,
}

impl Hunk {
    pub fn new() -> Self {
        Self {
            view: View::new(),
            header: String::new(),
            lines: Vec::new(),
        }
    }

    pub fn get_header_from(dh: &DiffHunk) -> String {
        String::from(str::from_utf8(dh.header()).unwrap())
            .replace("\r\n", "")
            .replace('\n', "")
    }

    pub fn push_line(&mut self, mut l: Line) {
        if self.lines.is_empty() {
            l.kind = LineKind::File;
        }
        if self.lines.len() == 1 {
            l.kind = LineKind::Hunk;
        }
        self.lines.push(l);
    }

    pub fn title(&self) -> String {
        self.header.to_string()
    }

    pub fn enrich_views(&mut self, other: Hunk) {
        for line in &mut self.lines {
            for ol in &other.lines {
                if line.content == ol.content {
                    line.view = ol.view.transfer();
                }
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
        for hunk in &mut self.hunks {
            for oh in &other.hunks {
                if hunk.header == oh.header {
                    hunk.view = oh.view.transfer();
                    hunk.enrich_views(oh.clone());
                }
            }
        }
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
                    file.view = of.view.transfer();
                    file.enrich_views(of.clone());
                }
            }
        }
    }
}

pub fn get_current_repo_status(sender: Sender<crate::Event>) {
    let path_buff_r =
        env::current_exe().map_err(|e| format!("can't get repo from executable {:?}", e));
    if path_buff_r.is_err() {
        println!("error while open current repo {:?}", path_buff_r);
        todo!("signal no repo for user to choose one");
    }
    let some = get_current_repo(path_buff_r.unwrap());
    if some.is_err() {
        println!("error while open current repo");
        todo!("signal no repo for user to choose one");
    }
    let repo = some.unwrap();

    let path = OsString::from(repo.path());

    sender
        .send(crate::Event::CurrentRepo(path.clone()))
        .expect("Could not send through channel");

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
            if current_hunk.header.is_empty() {
                // init hunk
                current_hunk.header = hh.clone();
            }
            if current_hunk.header != hh {
                // go to next hunk
                current_file.push_hunk(current_hunk.clone());
                current_hunk = Hunk::new();
                current_hunk.header = hh.clone();
            }
            current_hunk.push_line(Line::from_diff_line(&diff_line, LineKind::Regular));
        } else {
            // this is file header line.
            current_hunk.push_line(Line::from_diff_line(&diff_line, LineKind::File))
        }

        true
    });
    current_file.push_hunk(current_hunk);
    diff.files.push(current_file);
    diff
}

pub fn stage_via_apply(
    unstaged: Diff,
    staged: Option<Diff>,
    path: OsString,
    filter: ApplyFilter,
    sender: Sender<crate::Event>,
) {
    let repo = Repository::open(path.clone()).expect("can't open repo");
    let diff = repo
        .diff_index_to_workdir(None, None)
        .expect("can't get diff");
    let mut options = ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if filter.hunk_header.is_empty() {
            return true;
        }
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            return filter.hunk_header == header;
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
    repo.apply(&diff, ApplyLocation::Index, Some(&mut options))
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
            if staged.is_some() {
                diff.enrich_views(staged.unwrap());
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
    diff.enrich_views(unstaged);
    sender
        .send(crate::Event::Unstaged(diff))
        .expect("Could not send through channel");
}
