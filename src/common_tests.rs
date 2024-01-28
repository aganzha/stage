use crate::{Diff, File, Hunk, Line, View};
use git2::DiffLineType;

pub fn create_line(name: String) -> Line {
    let mut line = Line {
        content: String::new(),
        origin: DiffLineType::Context,
        view: View::new(),
    };
    line.content = name.to_string();
    line
}

pub fn create_hunk(name: String) -> Hunk {
    let mut hunk = Hunk::new();
    hunk.header = name.to_string();
    for i in 0..3 {
        hunk.lines.push(create_line(format!(
            "{} -> line {}",
            hunk.header, i
        )))
    }
    hunk
}

pub fn create_file(name: String) -> File {
    let mut file = File::new();
    file.path = name.to_string().into();
    for i in 0..3 {
        file.hunks.push(create_hunk(format!(
            "{} -> hunk {}",
            name, i
        )));
    }
    file
}

pub fn create_diff() -> Diff {
    let mut diff = Diff::new();
    for i in 0..3 {
        diff.files.push(create_file(format!(
            "file{}.rs",
            i
        )));
    }
    diff
}
