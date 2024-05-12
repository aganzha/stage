use gtk4::{gio};
use std::ffi::OsString;
use crate::git::{BranchData, Head, State, commit, get_current_repo_status};
use async_channel::Sender;
use log::{debug, info, trace};
use git2;

#[derive(Debug, Clone)]
pub enum MergeError {
    Conflicts,
    General(String)
}

pub fn merge_commit(path: OsString) {
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

    let message = repo.message().expect("cant get merge message");
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

pub fn merge(
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
            repo.reset(&ob, git2::ResetType::Soft, None)
                .expect("cant reset to commit");
            // if let Err(error) = repo.merge(&[&annotated_commit], None, None) {
            //     return Err(MergeError::General(String::from(error.message())));
            // }
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
            merge_commit(path);
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
