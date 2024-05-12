use gtk4::{gio};
use std::{ffi::OsString, collections::HashSet};
use crate::git::{BranchData, Head, State, commit, get_current_repo_status, STAGE_FLAG, make_diff, Hunk, DiffKind};
use async_channel::Sender;
use log::{debug, info, trace};
use git2;

#[derive(Debug, Clone)]
pub enum MergeError {
    Conflicts,
    General(String)
}

pub fn create_commit(path: OsString, message: Option<String>) {
    let mut repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");
    let tree_oid = repo
        .index()
        .expect("can't get index")
        .write_tree()
        .expect("can't write tree");

    let my_oid = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse")
        .id();

    let mut their_oid: Option<git2::Oid> = None;
    repo.mergehead_foreach(|oid_ref| -> bool {
        their_oid.replace(*oid_ref);
        true
    }).expect("cant get merge heads");

    info!("creating merge commit for {:?} {:?}", my_oid, their_oid);

    let my_commit = repo.find_commit(my_oid).expect("cant get commit");
    let their_commit = repo.find_commit(their_oid.expect("cant get their oid"))
        .expect("cant get commit");

    let message = message.unwrap_or(repo.message().expect("cant get merge message"));
    let tree = repo.find_tree(tree_oid).expect("can't find tree");

    repo.commit(
        Some("HEAD"),
        &me,
        &me,
        &message,
        &tree,
        &[&my_commit, &their_commit]
    ).expect("cant create merge commit");
}

pub fn branch(
    path: OsString,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<BranchData, MergeError> {
    info!("merging {:?}", branch_data.name);
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let annotated_commit = repo
        .find_annotated_commit(branch_data.oid)
        .expect("cant find commit");

    match repo.merge_analysis(&[&annotated_commit]) {
        Ok((analysis, _)) if analysis.is_up_to_date() => {
            info!("merge.uptodate");
        }

        Ok((analysis, preference))
            if analysis.is_fast_forward()
                && !preference.is_no_fast_forward() =>
        {
            info!("merge.fastforward");
            let ob = repo.find_object(branch_data.oid, Some(git2::ObjectType::Commit))
                .expect("cant find ob for oid");
            repo.checkout_tree(&ob, Some(git2::build::CheckoutBuilder::new().safe())).expect("cant checkout tree");
            repo.reset(&ob, git2::ResetType::Soft, None)
                .expect("cant reset to commit");
        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {

            info!("merge.normal");
            if let Err(error) = repo.merge(&[&annotated_commit], None, None) {
                return Err(MergeError::General(String::from(error.message())));
            }
            let index = repo.index().expect("cant get index");
            if index.has_conflicts() {
                gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || {
                        get_current_repo_status(Some(path), sender.clone());
                    }
                });
                return Err(MergeError::Conflicts);
            }
            let head_ref = repo.head().expect("can't get head");
            assert!(head_ref.is_branch());
            let message = format!(
                "merge branch {} into {}",
                branch_data.name,
                git2::Branch::wrap(head_ref).name().unwrap().unwrap()
            );
            create_commit(path, Some(message));
            repo.cleanup_state().expect("cant cleanup state");
        }
        Ok((analysis, preference)) => {
            todo!("not implemented case {:?} {:?}", analysis, preference);
        }
        Err(err) => {
            panic!("error in merge_analysis {:?}", err.message());
        }
    }

    let state = repo.state();
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(git2::ObjectType::Commit)
        .expect("can't get commit from ref!");
    let commit = ob.peel_to_commit().expect("can't get commit from ob!");
    let branch = git2::Branch::wrap(head_ref);
    let new_head = Head::new(&branch, &commit);
    sender
        .send_blocking(crate::Event::State(State::new(state)))
        .expect("Could not send through channel");
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");

    Ok(BranchData::from_branch(branch, git2::BranchType::Local)
        .expect("cant get branch"))
}


pub fn abort(path: OsString, sender: Sender<crate::Event>) {
    info!("git.abort merge");

    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut checkout_builder = git2::build::CheckoutBuilder::new();

    let index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut has_conflicts = false;
    for conflict in conflicts {
        if let Ok(conflict) = conflict {
            if let Some(our) = conflict.our {
                checkout_builder.path(our.path);
                has_conflicts = true;
            }
        }
    }
    if !has_conflicts {
        panic!("no way to abort merge without conflicts");
    }
    let head_ref = repo.head().expect("can't get head");

    let ob = head_ref
        .peel(git2::ObjectType::Commit)
        .expect("can't get commit from ref!");

    repo.reset(&ob, git2::ResetType::Hard, Some(&mut checkout_builder))
        .expect("cant reset hard");

    get_current_repo_status(Some(path), sender);
}

pub fn choose_conflict_side(path: OsString, ours: bool, sender: Sender<crate::Event>) {
    info!("git.choose side");
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut entries: Vec<git2::IndexEntry> = Vec::new();
    for conflict in conflicts {
        if let Ok(conflict) = conflict {
            if ours {
                if let Some(our) = conflict.our {
                    entries.push(our);
                }
            } else {
                if let Some(their) = conflict.their {
                    entries.push(their);
                }
            }
        }
    }
    if entries.is_empty() {
        panic!("nothing to resolve in choose_conflict_side");
    }

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.reverse(true);


    for entry in &mut entries {
        let pth = String::from_utf8(entry.path.clone()).expect("cant get path");
        diff_opts.pathspec(pth.clone());
        index.remove_path(std::path::Path::new(&pth)).expect("cant remove path");
        entry.flags = entry.flags & !STAGE_FLAG;
        index.add(&entry).expect("cant add to index");
    }
    index.write().expect("cant write index");
    let git_diff = repo.diff_index_to_workdir(Some(&index), Some(&mut diff_opts))
        .expect("cant get diff");

    let mut apply_opts = git2::ApplyOptions::new();

    let mut conflicted_headers = HashSet::new();
    let diff = make_diff(&git_diff, DiffKind::Conflicted);

    for f in diff.files {
        for h in f.hunks {
            if h.has_conflicts {
                conflicted_headers.insert(h.header);
            }
        }
    }

    apply_opts.hunk_callback(move |odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            let matched = conflicted_headers.contains(&header);
            debug!("header in callback  {:?} ??= {:?}", header, matched);
            return matched;
        }
        false
    });

    sender.send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");


    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut apply_opts))
        .expect("can't apply patch");

    sender.send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // if their side is choosen, it need to stage all conflicted paths
    // because resolved conflicts will go to staged area, but other changes
    // will be on other side of stage (will be +- same hunks on both sides)
    for entry in &entries {
        let pth = String::from_utf8(entry.path.clone()).expect("cant get path");
        index.add_path(std::path::Path::new(&pth)).expect("cant add path");
    }
    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender);
}
