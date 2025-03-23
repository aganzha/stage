use crate::git::{
    MARKER_OURS, MARKER_THEIRS,
    MARKER_VS,
};
use anyhow::{Context, Error, Result};
use git2;
use log::{debug, info};
use similar;
use std::io::prelude::*;
use std::{fs, io, path, str};

pub fn write_conflict_diff<'a>(
    bytes: &mut Vec<u8>,
    path: &str,
    similar_diff: similar::TextDiff<'a, 'a, 'a, str>,
) {
    io::Write::write(
        bytes,
        format!("diff --git \"a/{}\" \"b/{}\"\n", path, path).as_bytes(),
    )
    .expect("cant write bytes");
    io::Write::write(bytes, format!("--- \"a/{}\"\n", path).as_bytes()).expect("cant write bytes");
    io::Write::write(bytes, format!("+++ \"b/{}\"\n", path).as_bytes()).expect("cant write bytes");

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
                hunk.push((false, "header"));
                hunk.push((true, value));
                op = "collect_ours";
                count_new += 1;

                // magic 1/perhaps similar counts from 0?
                hunk_new_start = change.new_index().unwrap() + 1;
                if let Some(old_start) = change.old_index() {
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
        let conflict = conflict.unwrap();
        if let Some(our) = conflict.our {
            let pth = String::from_utf8(our.path).unwrap();
            conflict_paths.push(pth);
            has_conflicts = true;
        } else if let Some(paths) = paths_to_clean {
            let entry = conflict.their.unwrap();
            let path = path::PathBuf::from(str::from_utf8(&entry.path)?);
            paths.push(path);
        }
    }
    if !has_conflicts {
        return Ok(None);
    }

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");

    let mut bytes: Vec<u8> = Vec::new();
    for path in conflict_paths {
        let abs_file_path = repo
            .path()
            .parent()
            .context("no parent dir")?
            .join(path::Path::new(&path));
        debug!("file path of conflict {:?}", abs_file_path);
        let entry = current_tree.get_path(path::Path::new(&path))?;
        let ob = entry.to_object(repo)?;
        let blob = ob.as_blob().context("cant get blob")?;
        let tree_content = String::from_utf8_lossy(blob.content());
        let file_bytes = fs::read(abs_file_path)?;
        let workdir_content = String::from_utf8_lossy(&file_bytes);
        let text_diff = similar::TextDiff::from_lines(&tree_content, &workdir_content);
        write_conflict_diff(&mut bytes, &path, text_diff);
    }
    if bytes.is_empty() {
        return Err(Error::msg("no bytes in conflict"));
    }
    Ok(Some(git2::Diff::from_buffer(&bytes)?))
}
