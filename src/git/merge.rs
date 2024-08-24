// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::git::{
    branch::BranchName, get_conflicted_v1, get_current_repo_status,
    make_diff_options, BranchData, DeferRefresh, Head, Hunk, Line, State,
    MARKER_DIFF_A, MARKER_DIFF_B, MARKER_HUNK, MARKER_OURS, MARKER_THEIRS,
    MARKER_VS, MINUS, NEW_LINE, PLUS, SPACE,
};
use async_channel::Sender;
use git2;
use gtk4::gio;
use log::{debug, info, trace};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::from_utf8,
};

pub const STAGE_FLAG: u16 = 0x3000;

pub fn final_commit(
    path: PathBuf,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let me = repo.signature()?;

    let my_oid = repo.revparse_single("HEAD^{commit}")?.id();

    let my_commit = repo.find_commit(my_oid)?;

    let message = repo.message()?;

    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());

    let tree_oid = repo.index()?.write_tree()?;

    let tree = repo.find_tree(tree_oid)?;

    repo.commit(Some("HEAD"), &me, &me, &message, &tree, &[&my_commit])?;

    repo.cleanup_state()?;
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    Ok(())
}

pub fn final_merge_commit(
    path: PathBuf,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let mut repo = git2::Repository::open(path.clone())?;
    let me = repo.signature()?;

    let my_oid = repo.revparse_single("HEAD^{commit}")?.id();

    let mut their_oid: Option<git2::Oid> = None;
    repo.mergehead_foreach(|oid_ref| -> bool {
        their_oid.replace(*oid_ref);
        true
    })?;

    let their_oid = their_oid.unwrap();
    info!("creating merge commit for {:?} {:?}", my_oid, their_oid);

    let my_commit = repo.find_commit(my_oid)?;
    let their_commit = repo.find_commit(their_oid)?;

    // let message = message.unwrap_or(repo.message().expect("cant get merge message"));

    let mut their_branch: Option<git2::Branch> = None;
    let refs = repo.references()?;
    for r in refs.into_iter().flatten() {
        if let Some(oid) = r.target() {
            if oid == their_oid {
                their_branch.replace(git2::Branch::wrap(r));
            }
        }
    }
    let their_branch = their_branch.unwrap();

    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());
    let my_branch = git2::Branch::wrap(head_ref);
    let message = format!(
        "merge branch {} into {}",
        their_branch.branch_name(),
        my_branch.branch_name()
    );

    let tree_oid = repo.index()?.write_tree()?;

    let tree = repo.find_tree(tree_oid)?;

    repo.commit(
        Some("HEAD"),
        &me,
        &me,
        &message,
        &tree,
        &[&my_commit, &their_commit],
    )?;

    repo.cleanup_state()?;
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    Ok(())
}

pub fn branch(
    path: PathBuf,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
    mut defer: Option<DeferRefresh>,
) -> Result<Option<BranchData>, git2::Error> {
    info!("merging {:?}", branch_data.name);
    let repo = git2::Repository::open(path.clone())?;
    let annotated_commit = repo.find_annotated_commit(branch_data.oid)?;

    match repo.merge_analysis(&[&annotated_commit]) {
        Ok((analysis, _)) if analysis.is_up_to_date() => {
            info!("merge.uptodate");
        }

        Ok((analysis, preference))
            if analysis.is_fast_forward()
                && !preference.is_no_fast_forward() =>
        {
            info!("merge.fastforward");
            let ob = repo.find_object(
                branch_data.oid,
                Some(git2::ObjectType::Commit),
            )?;
            sender
                .send_blocking(crate::Event::LockMonitors(true))
                .expect("Could not send through channel");
            repo.checkout_tree(
                &ob,
                Some(git2::build::CheckoutBuilder::new().safe()),
            )?;
            sender
                .send_blocking(crate::Event::LockMonitors(false))
                .expect("Could not send through channel");
            repo.reset(&ob, git2::ResetType::Soft, None)?;
        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {
            info!("merge.normal");
            sender
                .send_blocking(crate::Event::LockMonitors(true))
                .expect("Could not send through channel");

            repo.merge(&[&annotated_commit], None, None)?;
            sender
                .send_blocking(crate::Event::LockMonitors(false))
                .expect("Could not send through channel");

            let index = repo.index()?;
            if index.has_conflicts() {
                // udpate repo status via defer
                if defer.is_none() {
                    defer.replace(DeferRefresh::new(
                        path.clone(),
                        sender.clone(),
                        true,
                        false,
                    ));
                }
                return Ok(None);
            }
            final_merge_commit(path, sender.clone())?;
        }
        Ok((analysis, preference)) => {
            todo!("not implemented case {:?} {:?}", analysis, preference);
        }
        Err(err) => {
            panic!("error in merge_analysis {:?}", err.message());
        }
    }

    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());
    let ob = head_ref.peel(git2::ObjectType::Commit)?;
    let commit = ob.peel_to_commit()?;
    let branch = git2::Branch::wrap(head_ref);
    let new_head = Head::new(&branch.branch_name(), &commit);
    sender
        .send_blocking(crate::Event::State(State::new(
            repo.state(),
            branch.branch_name(),
        )))
        .expect("Could not send through channel");
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");
    BranchData::from_branch(branch, git2::BranchType::Local)
}

pub fn abort(
    path: PathBuf,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    info!("git.abort merge");
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path.clone())?;
    let mut checkout_builder = git2::build::CheckoutBuilder::new();

    let index = repo.index()?;
    let conflicts = index.conflicts()?;
    let mut has_conflicts = false;
    for conflict in conflicts.flatten() {
        if let Some(our) = conflict.our {
            checkout_builder.path(our.path);
            has_conflicts = true;
        }
    }
    if !has_conflicts {
        panic!("no way to abort merge without conflicts");
    }

    let ob = repo.revparse_single("HEAD^{tree}")?;
    let current_tree = repo.find_tree(ob.id())?;
    let git_diff = repo.diff_tree_to_index(
        Some(&current_tree),
        None,
        Some(&mut make_diff_options()),
    )?;
    git_diff.foreach(
        &mut |d: git2::DiffDelta, _| {
            let path = d.new_file().path().expect("cant get path");
            checkout_builder.path(path);
            true
        },
        None,
        None,
        None,
    )?;

    let head_ref = repo.head()?;

    let ob = head_ref.peel(git2::ObjectType::Commit)?;

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.reset(&ob, git2::ResetType::Hard, Some(&mut checkout_builder))?;

    // cleanup conflicted
    sender
        .send_blocking(crate::Event::Conflicted(
            None,
            Some(State::new(repo.state(), "".to_string())),
        ))
        .expect("Could not send through channel");

    Ok(())
}

pub fn choose_conflict_side_of_blob<'a>(
    raw: &'a str,
    hunk_deltas: &mut Vec<(&'a str, i32)>,
    choosed_line_offset: i32,
    choosed_hunk_header: &'a str,
    ours_choosed: bool,
) -> String {
    // this will create patch, which !INSIDE CONFLICT!
    // will try to restore to original tree those
    // killing all markers. choosen side would be
    // cleanup up from -. another side will be killed with -
    let mut acc = Vec::new();

    let mut lines = raw.lines();

    let mut line_offset_inside_hunk: i32 = -1; // first line in hunk will be 0

    while let Some(line) = lines.next() {
        if !line.is_empty() && line[1..].starts_with(MARKER_OURS) {
            // is it marker that we need?
            line_offset_inside_hunk += 1;
            let mut this_is_current_conflict = false;

            let last_hunk_header = hunk_deltas.last().unwrap().0;
            if last_hunk_header == choosed_hunk_header
                && line_offset_inside_hunk == choosed_line_offset
            {
                this_is_current_conflict = true;
            }
            // TODO! do not need to handle all hunks here!
            // it is possible to keep them as is!!!!!!!!!!!!!!!

            // only it could be usefull - when 'choose ours/theirs' in all of conflicts

            // if predicate(
            //     line_offset_inside_hunk,
            //     hunk_deltas.last().unwrap().0,
            // ) {
            //     this_is_current_conflict = true;
            // }
            trace!(
                "======== current conflict? {:?}  last delta = {:?}",
                this_is_current_conflict,
                hunk_deltas.last().unwrap().0
            );
            if this_is_current_conflict {
                // this marker will be deleted
                trace!(
                    "current conflict. will delete OUR marker by keeping -"
                );
                acc.push(line);
                acc.push(NEW_LINE);
            } else {
                // do not delete it for now
                acc.push(SPACE);
                acc.push(&line[1..]);
                acc.push(NEW_LINE);
                // delta += 1;
                let hd = hunk_deltas.last().unwrap();
                let le = hunk_deltas.len();
                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                trace!(
                    "......remain marker ours (delete -) when not current conflict {:?}",
                    line
                );
            }
            // go deeper inside OURS
            'ours: while let Some(line) = lines.next() {
                line_offset_inside_hunk += 1;
                if !line.is_empty() && line[1..].starts_with(MARKER_VS) {
                    if this_is_current_conflict {
                        // this marker will be deleted
                        acc.push(line);
                        acc.push(NEW_LINE);
                    } else {
                        // do not delete it for now
                        acc.push(SPACE);
                        acc.push(&line[1..]);
                        acc.push(NEW_LINE);
                        let hd = hunk_deltas.last().unwrap();
                        let le = hunk_deltas.len();
                        hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                        trace!(
                            "......remain marker vs when not current conflict {:?}",
                            line
                        );
                    }
                    // go deeper inside THEIRS
                    for line in lines.by_ref() {
                        line_offset_inside_hunk += 1;
                        if !line.is_empty()
                            && line[1..].starts_with(MARKER_THEIRS)
                        {
                            if this_is_current_conflict {
                                // this marker will be deleted
                                acc.push(line);
                                acc.push(NEW_LINE);
                            } else {
                                // do not delete it for now
                                acc.push(" ");
                                acc.push(&line[1..]);
                                acc.push(NEW_LINE);
                                // delta += 1;
                                let hd = hunk_deltas.last().unwrap();
                                let le = hunk_deltas.len();
                                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                                trace!("......remain marker theirs (kill -) when not current conflict {:?}", line);
                            }
                            // conflict is over
                            // go out to next conflict
                            break 'ours;
                        } else {
                            // THEIR lines between === and >>>>
                            // this lines are deleted in this diff
                            // lets adjust it
                            if this_is_current_conflict {
                                if ours_choosed {
                                    // theirs will be deleted
                                    // #1.theirs
                                    trace!("......kill THEIRS (force -) cause OURS choosed {:?}", line);
                                    acc.push(MINUS);
                                    acc.push(&line[1..]);
                                    acc.push(NEW_LINE);
                                } else {
                                    // do not delete theirs!
                                    acc.push(SPACE);
                                    acc.push(&line[1..]);
                                    acc.push(NEW_LINE);
                                    // delta += 1;
                                    let hd = hunk_deltas.last().unwrap();
                                    let le = hunk_deltas.len();
                                    hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                                    trace!(
                                        "......remain theirs (kill -) cause theirs choosed {:?}",
                                        line
                                    );
                                }
                            } else {
                                // do not delete for now
                                // #2.theirs
                                acc.push(SPACE);
                                acc.push(&line[1..]);
                                acc.push(NEW_LINE);
                                let hd = hunk_deltas.last().unwrap();
                                let le = hunk_deltas.len();
                                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                                trace!("......remain theirs (kill -) when not current conflict {:?}", line);
                            }
                        }
                    }
                } else {
                    // OUR lines between <<< and ====
                    // in this diff they are not deleted
                    if this_is_current_conflict {
                        if ours_choosed {
                            // remain our lines
                            trace!(
                                "111111111111......choose ours. push line as is cause OUR chosed {:?}",
                                line
                            );
                            acc.push(SPACE);
                            acc.push(&line[1..]);
                            acc.push(NEW_LINE);
                        } else {
                            // delete our lines!
                            acc.push(MINUS);
                            acc.push(&line[1..]);
                            acc.push(NEW_LINE);
                            let hd = hunk_deltas.last().unwrap();
                            let le = hunk_deltas.len();
                            hunk_deltas[le - 1] = (hd.0, hd.1 - 1);
                            trace!(
                                "......delete ours (FORCE -) cause THEIR chosed {:?}",
                                line
                            );
                        }
                    } else {
                        // remain our lines
                        trace!(
                            "......REMAIN ours (should be as is but force kill -) cause this is not current conflict {:?}",
                            line
                        );
                        // here got the bug, when absolutelly 2 equal lines and git choses ours
                        // instead of theirs. theirs have -, but it will be killed, but it is also
                        // need to kill - in ours!
                        acc.push(SPACE);
                        acc.push(&line[1..]);
                        acc.push(NEW_LINE);
                    }
                }
            }
        } else {
            // line not belonging to conflict
            if !line.is_empty() && line[1..].contains(MARKER_HUNK) {
                hunk_deltas.push((line, 0));
                trace!(
                    "----------->reset offset for hunk {:?} {:?}",
                    line_offset_inside_hunk,
                    line
                );
                line_offset_inside_hunk = -1;
            } else {
                trace!(
                    "increment offset for line {:?} {:?}",
                    line_offset_inside_hunk,
                    line
                );
                line_offset_inside_hunk += 1;
            }
            // BUT! there is also changes outside of conflict!
            // those must be applied in reverse!
            // example. Here is their side chosen:
            // + line1
            // + line2
            // -<<<<<
            // - our line
            // - ===========
            // their line
            // ->>>>>>>>
            // first 2 lines have a + sign outside of conflict
            // they needed to restore to old tree. But i do not
            // need old tree. I need new one! So all ops(signs)
            // OUTSIDE conflict should be killed!!!
            // old logic below. new logic above. ---------------
            if line.starts_with(MINUS) && !line.starts_with(MARKER_DIFF_B) {
                debug!("whats the case? see nots below");
                debug!("???????? {:?} {:?}", hunk_deltas, line);
                acc.push(SPACE);
                acc.push(&line[1..]);
                acc.push(NEW_LINE);
                let hd = hunk_deltas.last().unwrap();
                let le = hunk_deltas.len();
                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                // ?????????????????????
                // see necxt clause. looks like its more important
                // when 1 hunk have multiple conflicts
                // perhaps here will be conflicts resolved
                // in previous turn. They already stripped off
                // conflicts markers, but their choosen lines
                // will be marked for deletion (there are no such lines
                // in tree and the diff is reversed
            } else if line.starts_with(PLUS)
                && !line.starts_with(MARKER_DIFF_A)
            {
                debug!("it is trying to restore old lines with +");
                debug!("do not do that! do not push line at all!");
                let hd = hunk_deltas.last().unwrap();
                let le = hunk_deltas.len();
                hunk_deltas[le - 1] = (hd.0, hd.1 - 1);
            } else {
                acc.push(line);
                acc.push(NEW_LINE);
            }
        }
    }
    acc.iter().fold("".to_string(), |cur, nxt| cur + nxt)
}

pub fn choose_conflict_side_of_hunk(
    path: PathBuf,
    file_path: PathBuf,
    hunk: Hunk,
    line: Line,
    interhunk: Option<u32>,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    debug!(
        "choose_conflict_side_of_hunk {:?} Line: {:?} Interhunk: {:?}",
        hunk.header,
        line.content(&hunk),
        interhunk
    );
    let repo = git2::Repository::open(path.clone())?;
    let mut index = repo.index()?;
    let conflicts = index.conflicts()?;

    let mut entries: Vec<git2::IndexEntry> = Vec::new();
    let mut conflict_paths: HashSet<PathBuf> = HashSet::new();
    for conflict in conflicts.flatten() {
        if let Some(entry) = conflict.our {
            conflict_paths
                .insert(PathBuf::from(from_utf8(&entry.path).unwrap()));
            entries.push(entry);
        }
        if let Some(entry) = conflict.their {
            conflict_paths
                .insert(PathBuf::from(from_utf8(&entry.path).unwrap()));
            entries.push(entry);
        }
        if let Some(entry) = conflict.ancestor {
            conflict_paths
                .insert(PathBuf::from(from_utf8(&entry.path).unwrap()));
            entries.push(entry);
        }
    }
    for path in conflict_paths {
        index.remove_path(path.as_path())?
    }
    // if not write index here
    // op will be super slow!
    index.write()?;

    let restore_index = move |file_path: &PathBuf| {
        // remove from index again to restore conflict
        // and also to clear from other side tree
        index
            .remove_path(Path::new(file_path))
            .expect("cant remove path");
        for entry in entries {
            index.add(&entry).expect("cant restore entry");
        }
        index.write().expect("cant restore index");
    };

    let current_tree = match repo
        .revparse_single("HEAD^{tree}")
        .and_then(|ob| repo.find_tree(ob.id()))
    {
        Ok(tree) => tree,
        Err(error) => {
            restore_index(&file_path);
            return Err(error);
        }
    };

    let mut opts = make_diff_options();
    let mut opts = opts.pathspec(&file_path).reverse(true);
    if let Some(ih) = interhunk {
        opts.interhunk_lines(ih);
    }

    let git_diff = match repo
        .diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
    {
        Ok(gd) => gd,
        Err(error) => {
            restore_index(&file_path);
            return Err(error);
        }
    };

    let mut patch = match git2::Patch::from_diff(&git_diff, 0) {
        Ok(patch) => patch.unwrap(),
        Err(error) => {
            restore_index(&file_path);
            return Err(error);
        }
    };

    let buff = match patch.to_buf() {
        Ok(buff) => buff,
        Err(error) => {
            restore_index(&file_path);
            return Err(error);
        }
    };
    let raw = buff.as_str().unwrap();
    debug!("OLD body BEFORE patch ___________________");
    for line in raw.lines() {
        debug!("{}", line);
    }
    let ours_choosed = line.is_our_side_of_conflict();
    let mut hunk_deltas: Vec<(&str, i32)> = Vec::new();

    let conflict_offset_inside_hunk = hunk.get_conflict_offset_by_line(&line);

    let reversed_header = Hunk::reverse_header(&hunk.header);

    let mut new_body = choose_conflict_side_of_blob(
        raw,
        &mut hunk_deltas,
        conflict_offset_inside_hunk,
        &reversed_header,
        ours_choosed,
    );
    debug!("new body for patch ___________________");
    for line in new_body.lines() {
        debug!("{}", line);
    }

    // so. not only new lines are changed. new_start are changed also!!!!!!
    // it need to add delta of prev hunk int new start of next hunk!!!!!!!!
    let mut prev_delta = 0;

    let mut updated_reversed_header = String::from("");

    for (hh, delta) in hunk_deltas {
        let new_header =
            Hunk::shift_new_start_and_lines(hh, prev_delta, delta);
        trace!("adjusting delta >> prev delta {:?}, delta {:?} hh {:?} new_header {:?}", prev_delta, delta, hh, new_header);
        new_body = new_body.replace(hh, &new_header);
        if hh == reversed_header {
            updated_reversed_header = new_header;
        }
        prev_delta += delta;
    }
    trace!(
        "reverse headers! {:?} vssssssssssssss      {:?}",
        reversed_header,
        updated_reversed_header
    );

    let git_diff = match git2::Diff::from_buffer(new_body.as_bytes()) {
        Ok(gd) => gd,
        Err(error) => {
            restore_index(&file_path);
            return Err(error);
        }
    };
    trace!("CREEEEEEEEEEEEEATED DIFF!");

    let mut apply_options = git2::ApplyOptions::new();

    apply_options.hunk_callback(|odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            return header == updated_reversed_header;
        }
        false
    });
    apply_options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: PathBuf = dd.new_file().path().unwrap().into();
            return file_path == path;
        }
        todo!("diff without delta");
    });

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    let apply_error = repo
        .apply(
            &git_diff,
            git2::ApplyLocation::WorkDir,
            Some(&mut apply_options),
        )
        .err();

    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    restore_index(&file_path);

    if let Some(error) = apply_error {
        return Err(error);
    }

    cleanup_last_conflict_for_file(
        path,
        file_path.clone(),
        interhunk,
        sender,
    )?;
    Ok(())
}

pub fn cleanup_last_conflict_for_file(
    path: PathBuf,
    file_path: PathBuf,
    interhunk: Option<u32>,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let mut index = repo.index()?;

    let diff = get_conflicted_v1(path.clone(), interhunk);
    // 1 - all conflicts in all files are resolved - update all
    // 2 - only this file is resolved, but have other conflicts - update all
    // 3 - conflicts are remaining in all files - just update conflicted
    let mut update_status = true;
    if let Some(diff) = get_conflicted_v1(path.clone(), interhunk) {
        for file in &diff.files {
            if file.hunks.iter().any(|h| h.conflict_markers_count > 0) {
                if file.path == file_path {
                    update_status = false;
                }
            } else if file.path == file_path {
                // cleanup conflicts only for this file
                index.remove_path(Path::new(&file_path))?;
                index.add_path(Path::new(&file_path))?;
                index.write()?;
            }
        }
    } else {
        trace!("cleanup_last_conflict_for_file. no mor conflicts! restore file in index!");
        index.remove_path(Path::new(&file_path))?;
        index.add_path(Path::new(&file_path))?;
        index.write()?;
    }
    if update_status {
        gio::spawn_blocking({
            move || {
                get_current_repo_status(Some(path), sender);
            }
        });
        return Ok(());
    }
    sender
        .send_blocking(crate::Event::Conflicted(
            diff,
            Some(State::new(repo.state(), "".to_string())),
        ))
        .expect("Could not send through channel");
    Ok(())
}
