use std::{env, str, path, ffi};
use git2::{Repository, StatusOptions, ObjectType,
           Oid, DiffFormat, DiffLine, DiffLineType,
           DiffFile, DiffHunk, DiffOptions, Index};
use crate::glib::{Sender};
use crate::gio;



fn get_current_repo(mut path_buff: path::PathBuf) -> Result<Repository, String> {
    let path = path_buff.as_path();
    Repository::open(path).or_else(|error| {
        println!("err while open repo {:?}", error);
        if !path_buff.pop() {
            return Err("no repoitory found".to_string());
        }
        return get_current_repo(path_buff);
    })
}



#[derive(Debug, Clone)]
pub struct View {
    pub line_no: i32,
    pub expanded: bool,
    pub rendered: bool,
    pub content: String
}

#[derive(Debug, Clone)]
pub enum LineKind {
    File,
    Hunk,
    Regular
}

#[derive(Debug, Clone)]
pub struct Line {
    pub view: Option<View>,
    pub origin: DiffLineType,
    pub content: String,
    pub kind: LineKind
}


impl Line {
    pub fn new() -> Self {
        Self {
            view: None,
            origin: DiffLineType::HunkHeader,
            content: String::new(),
            kind: LineKind::File
        }
    }
    pub fn from_diff_line(l: &DiffLine, k: LineKind) -> Self {
        return Self {
            view: None,
            origin: l.origin_value(),
            content: String::from(str::from_utf8(l.content()).unwrap())
                .replace("\r\n", "")
                .replace("\n", ""),
            kind: k
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub view: Option<View>,
    pub header: String,
    pub lines: Vec<Line>
}

impl Hunk {
    pub fn new() -> Self {
        Self {
            view: None,
            header: String::new(),
            lines: Vec::new()
        }
    }

    pub fn get_header_from(dh: &DiffHunk) -> String {
        String::from(str::from_utf8(dh.header()).unwrap())
            .replace("\r\n", "")
            .replace("\n", "")
    }

    pub fn push_line(&mut self, mut l: Line) {
        if self.lines.len() == 0 {
            l.kind = LineKind::File;
            println!("skiiiiiiiiiiiiip FILE {:?}", l.content);
        }
        if self.lines.len() == 1 {
            l.kind = LineKind::Hunk;
            println!("skiiiiiiiiiiiiip HUNK {:?}", l.content);
        }
        self.lines.push(l);
    }

    pub fn get_unique_name(&self) -> String {
        format!("{}", self.header)
    }
}

#[derive(Debug, Clone)]
pub struct File {
    pub view: Option<View>,
    pub path: ffi::OsString,
    pub id: Oid,
    pub hunks: Vec<Hunk>
}


impl File {
    pub fn new() -> Self {
        Self {
            view: None,
            path: ffi::OsString::new(),
            id: Oid::zero(),
            hunks: Vec::new()
        }
    }
    pub fn from_diff_file(f: &DiffFile) -> Self {
        return File {
            view: None,
            path: f.path().unwrap().into(),
            id: f.id(),
            hunks: Vec::new()
        }
    }

    pub fn push_hunk(&mut self, h: Hunk) {
        println!("Hunk {:?} for path {:?}", h.header, self.path);
        self.hunks.push(h);
    }

    pub fn get_unique_name(&self) -> String {
        format!("{}", self.path.to_str().unwrap())
    }
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub offset: i32,
    pub files: Vec<File>
}

impl Diff {
    pub fn new() -> Self {
        Self {
            offset: 0,
            files: Vec::new()
        }
    }
}

pub fn get_current_repo_status(sender: Sender<crate::Event>) {
    let path_buff_r = env::current_exe()
        .map_err(|e| format!("can't get repo from executable {:?}", e));
    if path_buff_r.is_err() {
        return
    }
    let some = get_current_repo(path_buff_r.unwrap());
    // TODO - remove if
    if let Ok(repo) =  some {
        let path = repo.path();
        sender.send(crate::Event::CurrentRepo(ffi::OsString::from(path)))
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
                if !new_file.path().is_some() {
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
                    if current_hunk.header == "" {
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
            sender.send(crate::Event::Status(diff))
                .expect("Could not send through channel");
        }
    }
}
