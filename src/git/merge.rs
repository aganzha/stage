use crate::git::{
    get_conflicted_v1, get_current_repo_status, make_diff, BranchData,
    DiffKind, Head, Hunk, Line, LineKind, State,
};
use async_channel::Sender;
use git2;
use gtk4::gio;
use log::{debug, info};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::from_utf8,
};

pub const STAGE_FLAG: u16 = 0x3000;

#[derive(Debug, Clone)]
pub enum MergeError {
    Conflicts,
    General(String),
}

pub fn commit(path: PathBuf) {
    let mut repo =
        git2::Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");

    let my_oid = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse")
        .id();

    let mut their_oid: Option<git2::Oid> = None;
    repo.mergehead_foreach(|oid_ref| -> bool {
        their_oid.replace(*oid_ref);
        true
    })
    .expect("cant get merge heads");

    let their_oid = their_oid.unwrap();
    info!("creating merge commit for {:?} {:?}", my_oid, their_oid);

    let my_commit = repo.find_commit(my_oid).expect("cant get commit");
    let their_commit = repo.find_commit(their_oid).expect("cant get commit");

    // let message = message.unwrap_or(repo.message().expect("cant get merge message"));

    let mut their_branch: Option<git2::Branch> = None;
    let refs = repo.references().expect("no refs");
    for r in refs.into_iter() {
        if let Ok(r) = r {
            if let Some(oid) = r.target() {
                if oid == their_oid {
                    their_branch.replace(git2::Branch::wrap(r));
                }
            }
        }
    }
    let their_branch = their_branch.unwrap();

    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let my_branch = git2::Branch::wrap(head_ref);
    let message = format!(
        "merge branch {} into {}",
        their_branch.name().unwrap().unwrap(),
        my_branch.name().unwrap().unwrap()
    );

    let tree_oid = repo
        .index()
        .expect("can't get index")
        .write_tree()
        .expect("can't write tree");
    let tree = repo.find_tree(tree_oid).expect("can't find tree");

    repo.commit(
        Some("HEAD"),
        &me,
        &me,
        &message,
        &tree,
        &[&my_commit, &their_commit],
    )
    .expect("cant create merge commit");
    repo.cleanup_state().expect("cant cleanup state");
}

pub fn branch(
    path: PathBuf,
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
            let ob = repo
                .find_object(branch_data.oid, Some(git2::ObjectType::Commit))
                .expect("cant find ob for oid");
            repo.checkout_tree(
                &ob,
                Some(git2::build::CheckoutBuilder::new().safe()),
            )
            .expect("cant checkout tree");
            repo.reset(&ob, git2::ResetType::Soft, None)
                .expect("cant reset to commit");
        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {
            info!("merge.normal");
            if let Err(error) = repo.merge(&[&annotated_commit], None, None) {
                return Err(MergeError::General(String::from(
                    error.message(),
                )));
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
            commit(path);
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

pub fn abort(path: PathBuf, sender: Sender<crate::Event>) {
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

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_index(Some(&current_tree), None, None)
        .expect("can't get diff tree to index");
    git_diff
        .foreach(
            &mut |d: git2::DiffDelta, _| {
                let path = d.new_file().path().expect("cant get path");
                checkout_builder.path(path);
                true
            },
            None,
            None,
            None,
        )
        .expect("cant foreach on diff");

    let head_ref = repo.head().expect("can't get head");

    let ob = head_ref
        .peel(git2::ObjectType::Commit)
        .expect("can't get commit from ref!");

    repo.reset(&ob, git2::ResetType::Hard, Some(&mut checkout_builder))
        .expect("cant reset hard");

    get_current_repo_status(Some(path), sender);
}

pub fn choose_conflict_side(
    path: PathBuf,
    ours: bool,
    sender: Sender<crate::Event>,
) {
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
            } else if let Some(their) = conflict.their {
                entries.push(their);
            }
        }
    }
    if entries.is_empty() {
        panic!("nothing to resolve in choose_conflict_side");
    }

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.reverse(true);

    for entry in &mut entries {
        let pth =
            String::from_utf8(entry.path.clone()).expect("cant get path");
        diff_opts.pathspec(pth.clone());
        index
            .remove_path(Path::new(&pth))
            .expect("cant remove path");
        entry.flags &= !STAGE_FLAG;
        index.add(entry).expect("cant add to index");
    }
    index.write().expect("cant write index");
    let git_diff = repo
        .diff_index_to_workdir(Some(&index), Some(&mut diff_opts))
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

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.apply(
        &git_diff,
        git2::ApplyLocation::WorkDir,
        Some(&mut apply_opts),
    )
    .expect("can't apply patch");

    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // if their side is choosen, it need to stage all conflicted paths
    // because resolved conflicts will go to staged area, but other changes
    // will be on other side of stage (will be +- same hunks on both sides)
    for entry in &entries {
        let pth =
            String::from_utf8(entry.path.clone()).expect("cant get path");
        index.add_path(Path::new(&pth)).expect("cant add path");
    }
    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender);
}

trait PathHolder {
    fn get_path(&self) -> PathBuf;
}

impl PathHolder for git2::IndexConflict {
    fn get_path(&self) -> PathBuf {
        PathBuf::from(
            from_utf8(match (&self.our, &self.their) {
                (Some(o), _) => &o.path[..],
                (_, Some(t)) => &t.path[..],
                _ => panic!("no path"),
            })
            .unwrap(),
        )
    }
}

pub fn choose_conflict_side_of_hunk(
    path: PathBuf,
    file_path: PathBuf,
    hunk: Hunk,
    line: Line,
    sender: Sender<crate::Event>,
) {
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");

    // let mut current_conflict: git2::IndexConflict;

    let chosen_path = PathBuf::from(&file_path);

    let mut current_conflict = conflicts
        .filter(|c| c.as_ref().unwrap().get_path() == chosen_path)
        .next()
        .unwrap()
        .unwrap();

    index
        .remove_path(chosen_path.as_path())
        .expect("cant remove path");

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");

    let mut opts = git2::DiffOptions::new();
    let mut opts = opts.pathspec(&file_path).reverse(true);
    let mut git_diff = repo
        .diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
        .expect("cant get diff");

    let reversed_header = Hunk::reverse_header(hunk.header);

    let mut options = git2::ApplyOptions::new();

    let file_path_clone = file_path.clone();

    if line.kind == LineKind::Ours {
        // just kill all hunk from diff.
        // our tree is all that required
        options.hunk_callback(|odh| -> bool {
            if let Some(dh) = odh {
                let header = Hunk::get_header_from(&dh);
                return header == reversed_header;
            }
            false
        });
        options.delta_callback(|odd| -> bool {
            if let Some(dd) = odd {
                let path: PathBuf = dd.new_file().path().unwrap().into();
                return file_path == path;
            }
            todo!("diff without delta");
        });
    } else {
        let their_entry = current_conflict.their.as_mut().unwrap();
        let their_original_flags = their_entry.flags;

        their_entry.flags &= !STAGE_FLAG;

        index.add(their_entry).expect("cant add entry");

        let mut opts = git2::DiffOptions::new();
        opts.pathspec(file_path.clone());
        // reverse means index will be NEW side cause we are adding hunk to workdir
        opts.reverse(true);

        // ANOTHER DIFF!
        git_diff = repo
            .diff_index_to_workdir(Some(&index), Some(&mut opts))
            .expect("cant get diff");

        // restore stage flag to conflict again
        their_entry.flags = their_original_flags;

        // passed hunk is from diff_tree_to_workdir. workdir is NEW side
        // for this hunks NEW side is workdir
        // so it need to compare NEW side of passed hunk with OLD side of this diff
        // (cause new side is index side, where hunk headers will differ a lot)
        options.hunk_callback(|odh| -> bool {
            if let Some(dh) = odh {
                return hunk.new_start == dh.old_start();
            }
            false
        });
        options.delta_callback(|odd| -> bool {
            if let Some(dd) = odd {
                let path: PathBuf = dd.new_file().path().unwrap().into();
                return file_path == path;
            }
            todo!("diff without delta");
        });
    }

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut options))
        .expect("can't apply patch");

    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // remove from index again to restore conflict
    // and also to clear from other side tree
    index
        .remove_path(Path::new(&file_path.clone()))
        .expect("cant remove path");

    if let Some(entry) = &current_conflict.ancestor {
        index.add(entry).expect("cant add ancestor");
        debug!("ancestor restored!");
    }
    if let Some(entry) = &current_conflict.our {
        debug!("our restored!");
        index.add(entry).expect("cant add our");
    }
    if let Some(entry) = &current_conflict.their {
        debug!("their restored!");
        index.add(entry).expect("cant add their");
    }
    index.write().expect("cant write index");

    cleanup_last_conflict_for_file(path, file_path_clone, sender);

    // let diff = get_conflicted_v1(path.clone());
    // // why where is [0]!!!!!!!!!!!!!!!!!!
    // // i have 1 file, but diff could have many of them!
    // let has_conflicts = diff.files[0].hunks.iter().fold(false, |a, h| {
    //     a || h.has_conflicts
    // });

    // // TODO! what about multiple files?
    // // this code asumes that this is only 1 file has conflicts!
    // // but, possibly, there will be multiple files!
    // if has_conflicts {
    //     sender
    //         .send_blocking(crate::Event::Conflicted(diff))
    //         .expect("Could not send through channel");
    //     return;
    // }

    // // cleanup conflicts and show banner
    // index.remove_path(Path::new(&file_path)).expect("cant remove path");
    // index.add_path(Path::new(&file_path)).expect("cant add path");

    // index.write().expect("cant write index");
    // get_current_repo_status(Some(path), sender);
}

pub fn cleanup_last_conflict_for_file(
    path: PathBuf,
    file_path: PathBuf,
    sender: Sender<crate::Event>,
) {
    let diff = get_conflicted_v1(path.clone());
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");

    let has_conflicts = diff
        .files
        .iter()
        .flat_map(|f| &f.hunks)
        .any(|h| h.has_conflicts);
    if !has_conflicts {
        // file is clear now
        // stage it!
        index
            .remove_path(Path::new(&file_path))
            .expect("cant remove path");
        index
            .add_path(Path::new(&file_path))
            .expect("cant add path");
        index.write().expect("cant write index");
        // perhaps it will be another files with conflicts
        // perhaps not
        // it need to rerender everything
        get_current_repo_status(Some(path), sender);
        return;
    }
    sender
        .send_blocking(crate::Event::Conflicted(diff))
        .expect("Could not send through channel");
}
