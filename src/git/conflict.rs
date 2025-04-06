use crate::git::{Hunk, LineKind, MARKER_OURS, MARKER_THEIRS, MARKER_VS, MINUS, SPACE};
use anyhow::{Context, Result};
use git2;
use log::info;
use similar;
use std::io::prelude::*;
use std::{fs, io, path, str};

pub fn write_conflict_diff<'a>(
    bytes: &mut Vec<u8>,
    path: &str,
    similar_diff: similar::TextDiff<'a, 'a, 'a, str>,
) {
    let mut file_header_written = false;
    let mut hunk: Vec<(bool, &str)> = Vec::new();
    let mut hunk_old_start = 0;
    let mut hunk_new_start = 0;
    let mut count_old = 0;
    let mut count_new = 0;
    let mut total_new = 0;
    let mut op = "";

    for change in similar_diff.iter_all_changes() {
        let value = change.value();
        let prefix: String = value.chars().take(7).collect();
        match (change.tag(), op, &prefix[..]) {
            (similar::ChangeTag::Insert, _, MARKER_OURS) => {
                assert!(op.is_empty());
                if !file_header_written {
                    bytes
                        .write_all(format!("diff --git \"a/{}\" \"b/{}\"\n", path, path).as_bytes())
                        .expect("cant write bytes");
                    bytes
                        .write_all(format!("--- \"a/{}\"\n", path).as_bytes())
                        .expect("cant write bytes");
                    bytes
                        .write_all(format!("+++ \"b/{}\"\n", path).as_bytes())
                        .expect("cant write bytes");
                    file_header_written = true;
                }
                hunk.push((false, "header"));
                hunk.push((true, value));
                op = "collect_ours";
                count_new += 1;

                // magic 1/perhaps similar counts from 0?
                hunk_new_start = change.new_index().unwrap() + 1;
                if let Some(_old_start) = change.old_index() {
                    panic!("STOP");
                } else {
                    hunk_old_start = hunk_new_start - total_new;
                }
            }
            (similar::ChangeTag::Insert, _, MARKER_VS) => {
                assert!(op == "collect_ours");
                hunk.push((true, value));
                op = "collect_theirs";
                count_new += 1;
            }
            (similar::ChangeTag::Insert, _, MARKER_THEIRS) => {
                assert!(op == "collect_theirs");
                count_new += 1;
                hunk.push((true, value));
                for (i, (plus, line)) in hunk.into_iter().enumerate() {
                    if i == 0 {
                        let header = format!(
                            "@@ -{},{} +{},{} @@\n",
                            hunk_old_start,
                            count_old,
                            hunk_new_start,
                            count_old + count_new
                        );
                        io::Write::write(bytes, header.as_bytes()).expect("cant write bytes");
                        continue;
                    } else if plus {
                        io::Write::write(bytes, &[b'+']).expect("cant write bytes");
                    } else {
                        io::Write::write(bytes, &[b' ']).expect("cant write bytes");
                    }
                    io::Write::write(bytes, line.as_bytes()).expect("cant write bytes");
                }
                hunk = Vec::new();
                op = "";
                total_new += count_new;
                count_new = 0;
                count_old = 0;
            }
            (_, "collect_ours", _) => {
                hunk.push((false, value));
                count_old += 1;
            }
            (_, "collect_theirs", _) => {
                hunk.push((true, value));
                count_new += 1;
            }
            (_, _, _) => {}
        }
    }
}

pub fn get_diff<'a>(
    repo: &'a git2::Repository,
    paths_to_clean: &mut Option<&mut Vec<path::PathBuf>>,
) -> Result<Option<git2::Diff<'a>>> {
    // so, when file is in conflict during merge, this means nothing
    // was staged to that file, cause merging in such state is PROHIBITED!

    // what is important here: all conflicts hunks must accommodate
    // both side: ours and theirs. if those come separated everything
    // will be broken!
    info!(".........git.conflict.get_diff");
    let index = repo.index()?;
    let conflicts = index.conflicts()?;

    // let mut missing_theirs: Vec<git2::IndexEntry> = Vec::new();
    let mut has_conflicts = false;
    let mut conflict_paths = Vec::new();
    for conflict in conflicts {
        let conflict = conflict?;
        if let Some(our) = conflict.our {
            let pth = String::from_utf8(our.path)?;
            conflict_paths.push(pth);
            has_conflicts = true;
        } else if let Some(paths) = paths_to_clean {
            let entry = conflict.their.context("no theirs")?;
            let path = path::PathBuf::from(str::from_utf8(&entry.path)?);
            paths.push(path);
        }
    }

    if !has_conflicts {
        return Ok(None);
    }

    let ob = repo.revparse_single("HEAD^{tree}")?;
    let current_tree = repo.find_tree(ob.id())?;

    let mut bytes: Vec<u8> = Vec::new();
    for path in conflict_paths {
        let abs_file_path = repo
            .path()
            .parent()
            .context("no parent dir")?
            .join(path::Path::new(&path));
        let entry = current_tree.get_path(path::Path::new(&path))?;
        let ob = entry.to_object(repo)?;
        let blob = ob.as_blob().context("cant get blob")?;
        let tree_content = String::from_utf8_lossy(blob.content());
        let file_bytes = fs::read(abs_file_path)?;
        let workdir_content = String::from_utf8_lossy(&file_bytes);
        let text_diff = similar::TextDiff::from_lines(&tree_content, &workdir_content);
        let ratio = text_diff.ratio();
        let before = bytes.len();
        write_conflict_diff(&mut bytes, &path, text_diff);
        if bytes.len() == before || ratio == 1.0 {
            if let Some(paths) = paths_to_clean {
                let path_to_clean = path::PathBuf::from(path);
                paths.push(path_to_clean);
            }
        }
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    Ok(Some(git2::Diff::from_buffer(&bytes)?))
}

pub fn choose_conflict_side_of_hunk(
    file_path: &path::Path,
    hunk: &Hunk,
    ours: bool,
    bytes: &mut Vec<u8>,
) -> Result<()> {
    let pth = file_path.as_os_str().as_encoded_bytes();
    bytes.write_all("diff --git \"a/".as_bytes())?;
    bytes.write_all(pth)?;
    bytes.write_all("\" \"b/".as_bytes())?;
    bytes.write_all(pth)?;
    bytes.write_all("\"\n".as_bytes())?;
    bytes.write_all("--- \"a/".as_bytes())?;
    bytes.write_all(pth)?;
    bytes.write_all("\"\n".as_bytes())?;
    bytes.write_all("+++ \"b/".as_bytes())?;
    bytes.write_all(pth)?;
    bytes.write_all("\"\n".as_bytes())?;

    // it need to invert all signs in hunk. just kill theirs
    // hunk header must be reversed!
    let reversed_header = Hunk::reverse_header(&hunk.header);
    let start_delta = hunk.new_start.as_i32() - hunk.old_start.as_i32();
    let lines_delta = if ours {
        // in case of ours its not needed to change lines count, cause it is the same
        // original: @@ -16,40 +16,18 @@
        // 40 lines in tree in git. 18 lines in workdir. choosing ours means we get version from tree
        0
    } else {
        // in case of theirs it need manually to count theirs, cause in workdir there are both: ours and teirs
        let their_lines = hunk
            .lines
            .iter()
            .filter(|l| matches!(l.kind, LineKind::Theirs(_)))
            .count();
        their_lines as i32 - hunk.old_lines as i32
    };
    let reversed_header =
        Hunk::shift_new_start_and_lines(&reversed_header, start_delta, lines_delta);
    bytes.write_all(reversed_header.as_bytes())?;
    bytes.write_all("\n".as_bytes())?;
    for line in &hunk.lines {
        let content = line.content(hunk);
        match line.kind {
            LineKind::Ours(_) => {
                if ours {
                    bytes.write_all(SPACE.as_bytes())?;
                } else {
                    bytes.write_all(MINUS.as_bytes())?;
                }
            }
            LineKind::Theirs(_) => {
                if ours {
                    bytes.write_all(MINUS.as_bytes())?;
                } else {
                    bytes.write_all(SPACE.as_bytes())?;
                }
            }
            _ => {
                bytes.write_all(MINUS.as_bytes())?;
            }
        }
        bytes.write_all(content.as_bytes())?;
        bytes.write_all("\n".as_bytes())?;
    }
    Ok(())
}
