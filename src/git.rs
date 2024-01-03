use crate::glib::Sender;
use git2::{DiffFile, DiffFormat, DiffHunk, DiffLine, DiffLineType, Oid, Repository};
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

    pub fn get_unique_name(&self) -> String {
        self.header.to_string()
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
    pub path: ffi::OsString,
    pub id: Oid,
    pub hunks: Vec<Hunk>,
}

impl File {
    pub fn new() -> Self {
        Self {
            view: View::new(),
            path: ffi::OsString::new(),
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

    pub fn get_unique_name(&self) -> String {
        self.path.to_str().unwrap().to_string()
    }
}

impl Default for File {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub files: Vec<File>,
    pub view: View,
}

impl Diff {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            view: View::new(),
        }
    }
}

impl Default for Diff {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Status {
    pub staged: Diff,
    pub unstaged: Diff,
    pub views: Vec<View>
}

impl Status {
    pub fn new() -> Self {
        Self {
            staged: Diff::new(),
            unstaged: Diff::new(),
            views: Vec::new()
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Self::new()
    }
}

pub fn get_current_repo_status(sender: Sender<crate::Event>) {
    let path_buff_r =
        env::current_exe().map_err(|e| format!("can't get repo from executable {:?}", e));
    if path_buff_r.is_err() {
        return;
    }
    let some = get_current_repo(path_buff_r.unwrap());
    // TODO - remove if
    if let Ok(repo) = some {
        let path = repo.path();
        sender
            .send(crate::Event::CurrentRepo(ffi::OsString::from(path)))
            .expect("Could not send through channel");

        // let head = repo.head().unwrap();
        // let commit = head. peel_to_commit().unwrap();
        // let tree = commit.tree().unwrap();
        // THIS IS STAGED CHANGES
        // if let Ok(git_diff) = repo.diff_tree_to_index(Some(&tree), Some(&repo.index().unwrap()), None) {
        // this is UNSTAGED CHANGES
        if let Ok(git_diff) = repo.diff_index_to_workdir(None, None) {
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
            sender
                .send(crate::Event::Status(diff))
                .expect("Could not send through channel");
        }
    }
}
