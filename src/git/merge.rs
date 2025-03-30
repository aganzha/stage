// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::git::{
    branch::BranchName, conflict, get_current_repo_status, make_diff, make_diff_options,
    BranchData, DeferRefresh, DiffKind, Hunk, Line, State,
};
use anyhow::Result;
use async_channel::Sender;
use git2;
use gtk4::gio;
use log::{debug, info};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::from_utf8,
};

//pub const STAGE_FLAG: u16 = 0x3000;

pub fn final_commit(path: PathBuf, sender: Sender<crate::Event>) -> Result<(), git2::Error> {
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
            get_current_repo_status(Some(path), sender).expect("cant get status");
        }
    });
    Ok(())
}

pub fn final_merge_commit(path: PathBuf, sender: Sender<crate::Event>) -> Result<(), git2::Error> {
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
        if let Some(ref_name) = r.name() {
            if ref_name.starts_with("refs/tags/") {
                continue;
            }
        }
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
        BranchName::from(&their_branch),
        BranchName::from(&my_branch)
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
            get_current_repo_status(Some(path), sender).expect("cant get status");
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
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path.clone())?;
    let annotated_commit = repo.find_annotated_commit(branch_data.oid)?;

    match repo.merge_analysis(&[&annotated_commit]) {
        Ok((analysis, _)) if analysis.is_up_to_date() => {
            info!("merge.uptodate");
        }

        Ok((analysis, preference))
            if analysis.is_fast_forward() && !preference.is_no_fast_forward() =>
        {
            info!("merge.fastforward");
            let ob = repo.find_object(branch_data.oid, Some(git2::ObjectType::Commit))?;
            sender
                .send_blocking(crate::Event::LockMonitors(true))
                .expect("Could not send through channel");
            repo.checkout_tree(&ob, Some(git2::build::CheckoutBuilder::new().safe()))?;
            sender
                .send_blocking(crate::Event::LockMonitors(false))
                .expect("Could not send through channel");
            repo.reset(&ob, git2::ResetType::Soft, None)?;
        }
        Ok((analysis, preference)) if analysis.is_normal() && !preference.is_fastforward_only() => {
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
                    defer.replace(DeferRefresh::new(path.clone(), sender.clone(), true, false));
                }
                return Ok(None);
            }
            final_merge_commit(path.clone(), sender.clone())?;
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
    let branch = git2::Branch::wrap(head_ref);
    BranchData::from_branch(&branch, git2::BranchType::Local)
}

pub fn abort(path: PathBuf, sender: Sender<crate::Event>) -> Result<(), git2::Error> {
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
    let git_diff =
        repo.diff_tree_to_index(Some(&current_tree), None, Some(&mut make_diff_options()))?;
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
    debug!("CLEANUP EMPTY CONFLICTED ?????????????");
    sender
        .send_blocking(crate::Event::Conflicted(
            None,
            Some(State::new(repo.state(), "".to_string())),
        ))
        .expect("Could not send through channel");

    Ok(())
}

pub fn choose_conflict_side_of_hunk(
    path: PathBuf,
    file_path: PathBuf,
    hunk: Hunk,
    line: Line,
    sender: Sender<crate::Event>,
) -> Result<()> {
    debug!(
        "choose_conflict_side_of_hunk {:?} Line: {:?}",
        hunk.header,
        line.content(&hunk)
    );
    let repo = git2::Repository::open(path.clone())?;

    let mut index = repo.index()?;
    let conflicts = index.conflicts()?;

    let mut entries: Vec<git2::IndexEntry> = Vec::new();
    let mut conflict_paths: HashSet<PathBuf> = HashSet::new();
    for conflict in conflicts.flatten() {
        if let Some(entry) = conflict.our {
            conflict_paths.insert(PathBuf::from(from_utf8(&entry.path).unwrap()));
            entries.push(entry);
        }
        if let Some(entry) = conflict.their {
            conflict_paths.insert(PathBuf::from(from_utf8(&entry.path).unwrap()));
            entries.push(entry);
        }
        if let Some(entry) = conflict.ancestor {
            conflict_paths.insert(PathBuf::from(from_utf8(&entry.path).unwrap()));
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

    let mut apply_options = git2::ApplyOptions::new();

    let mut bytes: Vec<u8> = Vec::new();
    conflict::choose_conflict_side_of_hunk(
        file_path.as_path(),
        &hunk,
        line.is_our_side_of_conflict(),
        &mut bytes,
    )?;
    let git_diff = match git2::Diff::from_buffer(&bytes) {
        Ok(gd) => gd,
        Err(error) => {
            restore_index(&file_path);
            return Err(error.into());
        }
    };
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
        return Err(error.into());
    }

    try_finalize_conflict(path, sender)?;
    Ok(())
}

pub fn try_finalize_conflict(
    path: PathBuf,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    debug!("cleanup_last_conflict_for_file");
    let repo = git2::Repository::open(path.clone())?;
    //let mut index = repo.index()?;

    // 1 - all conflicts in all files are resolved - update all
    //   - remove all from conflict@index
    // 2 - only this file is resolved, but have other conflicts - update all
    //     - remove this file from conflict@index
    // 3 - conflicts are remaining in all files - just update conflicted
    //     - do not touch conflict@index
    let mut update_status = true;
    let mut cleanup = Vec::new();
    let mut index = repo.index()?;
    if let Some(git_diff) = conflict::get_diff(&repo, &mut Some(&mut cleanup)).unwrap() {
        let diff = make_diff(&git_diff, DiffKind::Conflicted);
        debug!("for sure conflicted IS SOME");
        sender
            .send_blocking(crate::Event::Conflicted(
                Some(diff),
                Some(State::new(repo.state(), "".to_string())),
            ))
            .expect("Could not send through channel");
        update_status = !cleanup.is_empty();
    }
    for path in cleanup {
        index.remove_path(Path::new(&path))?;
        index.add_path(Path::new(&path))?;
        index.write()?;
    }
    if update_status {
        gio::spawn_blocking({
            move || {
                get_current_repo_status(Some(path), sender).expect("cant get status");
            }
        });
    }
    Ok(())
}
