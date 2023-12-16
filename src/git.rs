use std::{env, str, path, ffi};
use git2::{Repository, StatusOptions, ObjectType, Oid, DiffFormat, DiffLine, DiffLineType, DiffFile};
use crate::glib::{Sender};
use crate::gio;

fn get_current_repo() -> Result<Repository, String> {
    let mut path_buff = env::current_exe()
        .map_err(|e| format!("can't get repo from executable {:?}", e))?;
    loop {
        let path = path_buff.as_path();
        let repo_r = Repository::open(path);
        if let Ok(repo) =repo_r {
            return Ok(repo)
        } else {
            if !path_buff.pop() {
                break
            }
        }
    }
    Err("no repoitory found".to_string())
}

#[derive(Debug, Clone)]
pub struct Line {
    origin: DiffLineType,
    content: String
}


impl Line {
    pub fn new() -> Self {
        Self {
            origin: DiffLineType::HunkHeader,
            content: String::new()
        }
    }
    pub fn from_diff_line(l: &DiffLine) -> Self {
        return Self {
            origin: l.origin_value(),
            content: String::from(str::from_utf8(l.content()).unwrap())
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hunk {
    header: String,
    lines: Vec<Line>
}

impl Hunk {
    pub fn new() -> Self {
        Self {
            header: String::new(),
            lines: Vec::new()
        }
    }
}

#[derive(Debug, Clone)]
pub struct File {
    path: ffi::OsString,
    id: Oid,
    hunks: Vec<Hunk>
}


impl File {
    pub fn new() -> Self {
        Self {
            path: ffi::OsString::new(),
            id: Oid::zero(),
            hunks: Vec::new()
        }
    }
    pub fn from_diff_file(f: &DiffFile) -> Self {
        return File {
            path: f.path().unwrap().into(),
            id: f.id(),
            hunks: Vec::new()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diff {
    files: Vec<File>
}

impl Diff {
    pub fn new() -> Self {
        Self {
            files: Vec::new()
        }
    }
}

pub fn get_current_repo_status(sender: Sender<crate::Event>) {

    if let Ok(repo) = get_current_repo() {
        let path = repo.path();
        sender.send(crate::Event::CurrentRepo(ffi::OsString::from(path)))
            .expect("Could not send through channel");
        if let Ok(git_diff) = repo.diff_index_to_workdir(None, None) {
            let mut diff = Diff::new();
            let mut current_file = File::new();
            let mut current_hunk = Hunk::new();
            let _res = git_diff.print(DiffFormat::Patch, |diff_delta, o_diff_hunk, diff_line| {

                let old_file = diff_delta.old_file();
                let oid = old_file.id();
                if oid.is_zero() {
                    todo!();
                }
                if let Some(path) = old_file.path() {
                    if current_file.id.is_zero() {
                        // init new file
                        current_file = File::from_diff_file(&old_file);
                    }
                    if current_file.id != oid {
                        // go to next file
                        // push current_hunk to file and init new empty hunk
                        current_file.hunks.push(current_hunk.clone());
                        current_hunk = Hunk::new();
                        // push current_file to diff and change to new file
                        diff.files.push(current_file.clone());
                        current_file = File::from_diff_file(&old_file);

                    }
                    if let Some(diff_hunk) = o_diff_hunk {
                        let hh = String::from(str::from_utf8(diff_hunk.header()).unwrap());
                        if current_hunk.header == "" {
                            // init hunk
                            current_hunk.header = hh.clone();
                        }
                        if current_hunk.header != hh {
                            // go to next hunk
                            current_file.hunks.push(current_hunk.clone());
                            current_hunk = Hunk::new();
                            current_hunk.header = hh.clone();
                        }
                        current_hunk.lines.push(Line::from_diff_line(&diff_line));
                    } else {
                        // this is file header line.
                        current_hunk.lines.push(Line::from_diff_line(&diff_line));
                    }
                } else {
                    todo!();
                }

                true
            });
            current_file.hunks.push(current_hunk);
            diff.files.push(current_file);
            println!("finallllllllllllllllllllllll");
            for f in diff.files {
                println!("file {:?}", f.path);
                for h in f.hunks {
                    println!("hunk {:?}", h.header);
                    for l in h.lines {
                        print!("line {:} <-- {:?}", l.content, l.origin);
                    }
                }
            }
        }
    }
}

pub fn blob_text_from_id(repo: &Repository, id: Oid) -> Option<String> {
    if !id.is_zero() {
        let ob = repo.find_object(id, Some(ObjectType::Blob));
        println!("Ooooooooooooooob {:?}", ob);
        if let Ok(blob) = ob {
            let bl = blob.as_blob();
            if let Some(the_blob) = bl {
                let bytes = the_blob.content();
                let s = str::from_utf8(bytes).unwrap();
                return Some(String::from(s))
            }
        }
    }
    None
}



pub fn stage_changes(repo: Repository, _sender: Sender<crate::Event>) {
    // gio::spawn_blocking(|| {
        println!("oooooooooooooooooo {:?}", repo.is_empty());
    // });
}
