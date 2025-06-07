// SPDX-FileCopyrightText: 2025 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::git::{Hunk, LineKind, MARKER_OURS, MARKER_THEIRS, MARKER_VS, MINUS, SPACE};
use anyhow::{bail, Context, Result};
use git2;
use log::{debug, info};
use similar;
use std::io::prelude::*;
use std::{fs, io, path, str};

pub fn write_conflict_diff<'a>(
    bytes: &mut Vec<u8>,
    path: &str,
    similar_diff: similar::TextDiff<'a, 'a, 'a, str>,
) -> Result<bool> {
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
                if !op.is_empty() {
                    bail!("op is not empty during parse");
                }
                if !file_header_written {
                    bytes.write_all(
                        format!("diff --git \"a/{}\" \"b/{}\"\n", path, path).as_bytes(),
                    )?;
                    bytes.write_all(format!("--- \"a/{}\"\n", path).as_bytes())?;
                    bytes.write_all(format!("+++ \"b/{}\"\n", path).as_bytes())?;
                    file_header_written = true;
                }
                hunk.push((false, "header"));
                hunk.push((true, value));
                op = "collect_ours";
                count_new += 1;

                // magic 1/perhaps similar counts from 0?
                hunk_new_start = change.new_index().context("cant parse changes")? + 1;
                if let Some(_old_start) = change.old_index() {
                    panic!("STOP");
                } else {
                    hunk_old_start = hunk_new_start - total_new;
                }
            }
            (similar::ChangeTag::Insert, _, MARKER_VS) => {
                if op != "collect_ours" {
                    bail!("op != collect_ours during parse");
                }
                hunk.push((true, value));
                op = "collect_theirs";
                count_new += 1;
            }
            (similar::ChangeTag::Insert, _, MARKER_THEIRS) => {
                if op != "collect_theirs" {
                    bail!("op != collect_theirs during parse");
                }
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
                        io::Write::write(bytes, header.as_bytes())?;
                        continue;
                    } else if plus {
                        io::Write::write(bytes, b"+")?;
                    } else {
                        io::Write::write(bytes, b" ")?;
                    }
                    io::Write::write(bytes, line.as_bytes())?;
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
    if !op.is_empty() {
        // iterrupted during parse cause manual editing
        bail!("broken parsing");
    }
    Ok(!bytes.is_empty())
}

pub fn get_diff<'a>(
    repo: &'a git2::Repository,
    paths_to_stage: &mut Vec<path::PathBuf>,
    paths_to_unstage: &mut Vec<path::PathBuf>,
) -> Result<Option<git2::Diff<'a>>> {
    // so, when file is in conflict during merge, this means nothing
    // was staged to that file, cause merging in such state is PROHIBITED!

    // what is important here: all conflicts hunks must accommodate
    // both side: ours and theirs. if those come separated everything
    // will be broken!
    info!(".........git.conflict.get_diff");
    let index = repo.index()?;
    println!("___________1");
    let conflicts = index.conflicts()?;
    println!("___________2");
    let mut has_conflicts = false;
    let mut conflict_paths = Vec::new();
    for conflict in conflicts {
        let conflict = conflict?;
        if let Some(our) = conflict.our {
            let pth = String::from_utf8(our.path)?;
            conflict_paths.push(pth);
            has_conflicts = true;
        } else {
            // let entry = conflict.their.context("no theirs")?;
            // let path = path::PathBuf::from(str::from_utf8(&entry.path)?);
            // why we want to stage those files????
            // what does no theirs mean? why it is conflicted then?
            // paths_to_stage.push(path);
            let their = conflict.their.context("no theirs")?.path;
            let pth = String::from_utf8(their)?;
            debug!("NO OUR IN CONFLICT {:?}", pth);
            conflict_paths.push(pth);
            has_conflicts = true;
        }
    }
    println!("___________3");
    if !has_conflicts {
        return Ok(None);
    }

    let ob = repo.revparse_single("HEAD^{tree}")?;
    println!("_______________4444444444444444444444444444");
    let current_tree = repo.find_tree(ob.id())?;
    println!("______________555555555555555555555");
    let mut bytes: Vec<u8> = Vec::new();
    for str_path in conflict_paths {
        let path = path::Path::new(&str_path);
        let abs_file_path = repo.path().parent().context("no parent dir")?.join(path);
        println!("______________6666666666666666666666666");
        // let entry = current_tree.get_path(path::Path::new(&path))?;
        // path could not be in tree!
        if let Ok(entry) = current_tree.get_path(path::Path::new(&path)) {
            println!("_______________77777777777");
            let ob = entry.to_object(repo)?;
            println!("_______________88888888888888");
            let blob = ob.as_blob().context("cant get blob")?;
            println!("_____________9999999999999999999");
            let tree_content = String::from_utf8_lossy(blob.content());
            let file_bytes = fs::read(abs_file_path)?;
            let workdir_content = String::from_utf8_lossy(&file_bytes);
            let text_diff = similar::TextDiff::from_lines(&tree_content, &workdir_content);
            let mut current_bytes: Vec<u8> = Vec::new();

            match write_conflict_diff(&mut current_bytes, &str_path, text_diff) {
                Ok(write_result) => {
                    if write_result {
                        bytes.extend(current_bytes);
                    } else {
                        // not sure why paths_to_unstage was here.
                        // if nothing were written for file and
                        // no errors - means file is cleaned from conflicts.
                        // lets just stage it.
                        // paths_to_unstage.push(path.into());
                        paths_to_stage.push(path.into());
                    }
                }
                Err(error) => {
                    debug!("error while produce similar diff {:?}", error);
                    paths_to_unstage.push(path.into());
                }
            }
        } else {
            // if file is not in tree - it was deleted.
            // must be added to staged then
            // if add it to unstaged, it will just disapear from everywhere
            paths_to_stage.push(path.into());
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
