use crate::{Diff, File, Hunk, Line, View};

pub fn create_line(prefix: i32) -> Line {
    let mut line = Line::new();
    line.content = format!("line {}", prefix);
    line.kind = crate::LineKind::Regular;
    line
}

pub fn create_hunk(prefix: i32) -> Hunk {
    let mut hunk = Hunk::new();
    hunk.header = format!("hunk {}", prefix);
    for i in 0..3 {
        hunk.lines.push(create_line(i))
    }
    hunk
}

pub fn create_file(prefix: i32) -> File {
    let mut file = File::new();
    file.path = format!("file{}.rs", prefix).into();
    for i in 0..3 {
        file.hunks.push(create_hunk(i))
    }
    file
}

pub fn create_diff() -> Diff {
    let mut diff = Diff::new();
    for i in 0..3 {
        diff.files.push(create_file(i));
    }
    diff
}

#[test]
fn test_diff_add() {
    println!("++++++++++++++++++++++++");
    let mut diff = create_diff();
    let mut other = create_diff();
    diff.add(other);
}
