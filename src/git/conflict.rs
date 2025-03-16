use crate::gio;
use crate::git::{
    get_untracked, make_diff, make_diff_options, Diff, DiffKind, MARKER_OURS, MARKER_THEIRS,
    MARKER_VS,
};
use async_channel::Sender;
use git2;
use log::{debug, info};
use similar;
use std::{fs, io, path, str};

pub fn write_conflict_diff<'a>(
    bytes: &mut Vec<u8>,
    path: &str,
    similar_diff: similar::TextDiff<'a, 'a, 'a, str>,
) {
    io::Write::write(
        bytes,
        format!("diff --git \"a/{}\" \"b/{}\"\n", path, path).as_bytes(),
    );
    io::Write::write(bytes, format!("--- \"a/{}\"\n", path).as_bytes());
    io::Write::write(bytes, format!("+++ \"b/{}\"\n", path).as_bytes());
    // let text_diff = u_diff.diff;
    // for change in u_diff.iter_all_changes() {
    //     debug!("zzzzzzzzzzzzzzz {:?}", change);
    // }
    // let mut writing = false;
    let mut hunk: Vec<(bool, &str)> = Vec::new();
    let mut hunk_old_start = 0;
    let mut hunk_new_start = 0;
    let mut count_old = 0;
    let mut count_new = 0;
    let mut total_new = 0;
    let mut op = "";
    // let mut collect_ours = false;
    // let mut collect_theirs = false;

    for change in similar_diff.iter_all_changes() {
        debug!("[[[[ {:?}", change);
        let value = change.value();
        let prefix: String = value.chars().take(7).collect();
        match (change.tag(), op, &prefix[..]) {
            (similar::ChangeTag::Insert, _, MARKER_OURS) => {
                assert!(op == "");
                hunk.push((false, "header"));
                //let val = format!("+{}", &value);
                hunk.push((true, value));
                op = "collect_ours";
                count_new += 1;

                // hunk_old_start = change.old_index().unwrap();
                hunk_new_start = change.new_index().unwrap();
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
                let header = format!(
                    "@@ -{},{} +{},{} @@\n",
                    hunk_old_start,
                    count_old,
                    hunk_new_start,
                    count_old + count_new
                );
                hunk[0] = (false, &header);
                for (plus, line) in hunk {
                    if plus {
                        io::Write::write(bytes, &[b'+']);
                    }
                    io::Write::write(
                        bytes,
                        line.as_bytes(), //format!("{}\n", line).as_bytes(),
                    );
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

pub fn get_diff(
    path: path::PathBuf,
    interhunk: Option<u32>,
    sender: Sender<crate::Event>,
) -> Option<Diff> {
    // thought
    // TODO! file path options!
    // it is called now from track_changes, so it need to update only 1 file!
    // thought

    // so, when file is in conflict during merge, this means nothing
    // was staged to that file, cause merging in such state is PROHIBITED!

    // what is important here: all conflicts hunks must accommodate
    // both side: ours and theirs. if those come separated everything
    // will be broken!
    info!(".........git.conflict.get_diff");
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut opts = make_diff_options();

    if let Some(interhunk) = interhunk {
        opts.interhunk_lines(interhunk);
    }
    let mut missing_theirs: Vec<git2::IndexEntry> = Vec::new();
    let mut has_conflicts = false;
    let mut conflict_paths = Vec::new();
    for conflict in conflicts {
        let conflict = conflict.unwrap();
        if let Some(our) = conflict.our {
            let pth = String::from_utf8(our.path).unwrap();
            opts.pathspec(pth.clone());
            conflict_paths.push(pth);
            has_conflicts = true;
        } else {
            missing_theirs.push(conflict.their.unwrap())
        }
    }
    // file was deleted in current branch (not in merging one)
    // it will be not displayed. lets just delete it from index
    // and display as untracked (no other good ways exists yet)
    for entry in &missing_theirs {
        let pth = path::PathBuf::from(str::from_utf8(&entry.path).unwrap());
        index.remove_path(&pth).unwrap();
    }
    if !missing_theirs.is_empty() {
        debug!("moving file to untracked during conflict");
        index.write().unwrap();
        gio::spawn_blocking({
            let sender = sender.clone();
            let path = path.clone();
            move || {
                get_untracked(path, sender);
            }
        });
    }
    if !has_conflicts {
        return None;
    }

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
        .expect("cant get diff");

    let mut diff = make_diff(&git_diff, DiffKind::Conflicted);

    if diff.is_empty() {
        return None;
    }
    let patch = git2::Patch::from_diff(&git_diff, 0).unwrap();
    let mut patch = patch.unwrap();
    let patch_str = patch.to_buf().unwrap();
    let patch_str = patch_str.as_str().unwrap();
    for line in patch_str.lines() {
        debug!("_____{:?}", line);
    }
    for path in conflict_paths {
        let abs_file_path = repo.path().parent().unwrap().join(path::Path::new(&path));
        debug!("file path of conflict {:?}", abs_file_path);
        let entry = current_tree
            .get_path(path::Path::new(&path))
            .expect("no entry");
        let ob = entry.to_object(&repo).expect("no object");
        let blob = ob.as_blob().expect("no blob");
        let tree_content = String::from_utf8_lossy(blob.content());
        let file_bytes = fs::read(abs_file_path).expect("no file");
        let workdir_content = String::from_utf8_lossy(&file_bytes);
        let text_diff = similar::TextDiff::from_lines(&tree_content, &workdir_content);
        // what do i want here: implement custom to_writer
        // method, which will iterate not over hunks, but
        // over conflict markers!
        // let mut unified_diff = text_diff.unified_diff();
        // unified_diff.context_radius(3);
        // unified_diff.header(&format!("\"a/{}\"", path), &format!("\"b/{}\"", path));

        //debug!("____________________________ {}", unified_diff);
        let mut bytes: Vec<u8> = Vec::new();
        // io::Write::write(
        //     &mut bytes,
        //     format!("diff --git \"a/{}\" \"b/{}\"\n", path, path).as_bytes(),
        // );
        // unified_diff.to_writer(&mut bytes);
        write_conflict_diff(&mut bytes, &path, text_diff);

        debug!("oooooooooooooooooooooooooooooooo {:?}", bytes.len());
        let body = String::from_utf8_lossy(&bytes);
        for line in body.lines() {
            debug!("__________{:?}", line);
        }
        // debug!("................. {:?}", String::from_utf8_lossy(&bytes));
        let another_git_diff = git2::Diff::from_buffer(&bytes).unwrap();
        //debug!("??????????????????? {:?} {:?}", path, another_git_diff.is_ok());
        let mut another_diff = make_diff(&another_git_diff, DiffKind::Staged);
        return Some(make_diff(&another_git_diff, DiffKind::Conflicted));
        // debug!("____________________________ {}", another_diff);
    }
    // // if intehunk is unknown it need to check missing hunks
    // // (either ours or theirs could go to separate hunk)
    // // and recreate diff to accomodate both ours and theirs in single hunk
    // if let Some(interhunk) = interhunk {
    //     diff.interhunk.replace(interhunk);
    // } else {
    //     // this vec store tuples with last line_no of prev hunk
    //     // and first line_no of next hunk
    //     let mut hunks_to_join = Vec::new();
    //     let mut prev_conflict_line: Option<HunkLineNo> = None;
    //     for file in &diff.files {
    //         for hunk in &file.hunks {
    //             let (first_marker, last_marker) =
    //                 hunk.lines.iter().fold((None, None), |acc, line| {
    //                     match (acc.0, acc.1, &line.kind) {
    //                         (None, _, LineKind::ConflictMarker(m)) => (Some(m), Some(m)),
    //                         (Some(_), _, LineKind::ConflictMarker(m)) => (acc.0, Some(m)),
    //                         _ => acc,
    //                     }
    //                 });
    //             match (first_marker, last_marker) {
    //                 (None, _) => {
    //                     // hunk without conflicts?
    //                     // just skip it
    //                 }
    //                 (Some(_), None) => {
    //                     panic!("imposible case");
    //                 }
    //                 (Some(first), Some(last)) if first == MARKER_THEIRS || first == MARKER_VS => {
    //                     // hunk is not started with ours
    //                     // store prev hunk last line and this hunk start to join em
    //                     hunks_to_join.push((prev_conflict_line.unwrap(), hunk.old_start));
    //                     if last == MARKER_OURS {
    //                         prev_conflict_line
    //                             .replace(HunkLineNo::new(hunk.old_start.as_u32() + hunk.old_lines));
    //                     } else {
    //                         prev_conflict_line = None;
    //                     }
    //                 }
    //                 (_, Some(m)) if m != MARKER_THEIRS => {
    //                     // hunk is not ended with theirs
    //                     // store prev hunk last line and this hunk start to join em
    //                     assert!(prev_conflict_line.is_none());
    //                     prev_conflict_line
    //                         .replace(HunkLineNo::new(hunk.old_start.as_u32() + hunk.old_lines));
    //                 }
    //                 (Some(start), Some(end)) => {
    //                     assert!(start == MARKER_OURS);
    //                     assert!(end == MARKER_THEIRS);
    //                 }
    //             }
    //         }
    //     }
    //     if !hunks_to_join.is_empty() {
    //         let interhunk = hunks_to_join
    //             .iter()
    //             .fold(HunkLineNo::new(0), |acc, from_to| {
    //                 if acc < from_to.1 - from_to.0 {
    //                     return from_to.1 - from_to.0;
    //                 }
    //                 acc
    //             });
    //         opts.interhunk_lines(interhunk.as_u32());
    //         let git_diff = repo
    //             .diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
    //             .expect("cant get diff");
    //         diff = make_diff(&git_diff, DiffKind::Conflicted);
    //         diff.interhunk.replace(interhunk.as_u32());
    //     }
    // }
    panic!("STOP");
    Some(diff)
}
