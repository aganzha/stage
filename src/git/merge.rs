use gtk4::{gio};
use std::ffi::OsString;
use crate::git::{BranchData, Head, State, commit, get_current_repo_status};
use async_channel::Sender;
use log::{debug, info, trace};
use git2;

#[derive(Debug, Clone)]
pub enum MergeError {
    Conflicts,
    Analisys(String)
}

pub fn merge(
    path: OsString,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<BranchData, MergeError> {
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    // i need to store this branch_data.oid somehow,
    // or i can get merging_head?
    let annotated_commit = repo
        .find_annotated_commit(branch_data.oid)
        .expect("cant find commit");
    // let result = repo.merge(&[&annotated_commit], None, None);

    let do_merge = || -> Result<bool, String> {

        let result = repo.merge(&[&annotated_commit], None, None);
        if result.is_err() {
            let git_err = result.unwrap_err();
            return Err(String::from(git_err.message()));
        }

        // all changes are in index now
        let head_ref = repo.head().expect("can't get head");
        assert!(head_ref.is_branch());

        let index = repo.index().expect("cant get index");
        if index.has_conflicts() {
            // just skip commit as it will panic anyways
            return Ok(true);
        }
        // commit(path.clone(), String::from(""), sender.clone());
        let current_branch = git2::Branch::wrap(head_ref);
        let message = format!(
            "merge branch {} into {}",
            branch_data.name,
            current_branch.name().unwrap().unwrap()
        );
        commit(path.clone(), message, sender.clone());
        Ok(false)
    };

    let mut has_conflicts = false;

    let refresh_status = || {
        gio::spawn_blocking({
            let sender = sender.clone();
            let path = path.clone();
            move || {
                get_current_repo_status(Some(path), sender.clone());
            }
        });
    };

    match repo.merge_analysis(&[&annotated_commit]) {
        Ok((analysis, _)) if analysis.is_up_to_date() => {
            info!("merge.uptodate");
        }

        Ok((analysis, preference))
            if analysis.is_fast_forward()
                && !preference.is_no_fast_forward() =>
        {
            trace!("-----------------------------------> {:?}", analysis);
            info!("merge.fastforward");
            match do_merge() {
                Ok(true) => {
                    has_conflicts = true;
                    debug!("retirning after do merge 0");
                    refresh_status();
                    return Err(MergeError::Conflicts)
                },
                Ok(false) => {
                    has_conflicts = false
                }
                Err(message) => return Err(MergeError::Analisys(message))
            }

        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {
            trace!("-----------------------------------> {:?}", analysis);
            info!("merge.normal");
            match do_merge() {
                Ok(true) => {
                    has_conflicts = true;
                    debug!("retirning after do merge 1");
                    refresh_status();
                    return Err(MergeError::Conflicts)
                }
                Ok(false) => {
                    has_conflicts = false;
                }
                Err(message) => return Err(MergeError::Analisys(message))
            }
        }
        Ok((analysis, preference)) => {
            todo!("not implemented case {:?} {:?}", analysis, preference);
        }
        Err(err) => {
            panic!("error in merge_analysis {:?}", err.message());
        }
    }
    if has_conflicts {
        panic!("----------------> whats the case? where is other returns?");
        refresh_status();
        return Err(MergeError::Conflicts);
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
