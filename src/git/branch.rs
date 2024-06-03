use crate::commit::CommitRepr;
use crate::get_current_repo_status;
use crate::git::remote::set_remote_callbacks;
use async_channel::Sender;
use chrono::{DateTime, FixedOffset};
use git2;
use gtk4::gio;
use log::{info};
use std::cmp::Ordering;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BranchData {
    pub name: String,
    pub refname: String,
    pub branch_type: git2::BranchType,
    pub oid: git2::Oid,
    pub commit_string: String,
    pub is_head: bool,
    pub upstream_name: Option<String>,
    pub commit_dt: DateTime<FixedOffset>,
}

impl Default for BranchData {
    fn default() -> Self {
        BranchData {
            name: String::from(""),
            refname: String::from(""),
            branch_type: git2::BranchType::Local,
            oid: git2::Oid::zero(),
            commit_string: String::from(""),
            is_head: false,
            upstream_name: None,
            commit_dt: DateTime::<FixedOffset>::MIN_UTC.into(),
        }
    }
}

impl BranchData {
    pub fn from_branch(
        branch: git2::Branch,
        branch_type: git2::BranchType,
    ) -> Result<Option<Self>, git2::Error> {
        let name = branch.name().unwrap().unwrap().to_string();
        let mut upstream_name: Option<String> = None;
        if let Ok(upstream) = branch.upstream() {
            upstream_name =
                Some(upstream.name().unwrap().unwrap().to_string());
        }
        let is_head = branch.is_head();
        let bref = branch.get();
        let refname = bref.name().unwrap().to_string();
        let ob = bref.peel(git2::ObjectType::Commit)?;
        let commit = ob.peel_to_commit()?;
        let commit_string = commit.log_message();
        let commit_dt = commit.dt();
        if let Some(oid) = branch.get().target() {
            Ok(Some(BranchData {
                name,
                refname,
                branch_type,
                oid,
                commit_string,
                is_head,
                upstream_name,
                commit_dt,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn local_name(&self) -> String {
        self.name.replace("origin/", "")
    }
    pub fn remote_name(&self) -> String {
        format!("origin/{}", self.name.replace("origin/", ""))
    }
}

pub fn get_branches(path: PathBuf) -> Result<Vec<BranchData>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let mut result = Vec::new();
    let branches = repo.branches(None)?;
    branches.for_each(|item| {
        let (branch, branch_type) = item.unwrap();
        if let Ok(Some(branch_data)) =
            BranchData::from_branch(branch, branch_type)
        {
            result.push(branch_data);
        }
    });
    result.sort_by(|a, b| {
        // let head be always on top
        if a.is_head {
            return Ordering::Less;
        }
        if b.is_head {
            return Ordering::Greater;
        }

        if a.branch_type == git2::BranchType::Local
            && b.branch_type != git2::BranchType::Local
        {
            return Ordering::Less;
        }
        if b.branch_type == git2::BranchType::Local
            && a.branch_type != git2::BranchType::Local
        {
            return Ordering::Greater;
        }
        b.commit_dt.cmp(&a.commit_dt)
    });
    Ok(result)
}

pub fn checkout_branch(
    path: PathBuf,
    mut branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<Option<BranchData>, git2::Error> {
    info!("checkout branch");
    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(branch_data.oid)?;

    // weird....................
    // if branch_data.branch_type == git2::BranchType::Remote {
    //     // handle case when checkout remote branch and local branch
    //     // is ahead of remote
    //     let head_ref = repo.head()?;
    //     assert!(head_ref.is_branch());

    //     // wtf? when checkout 24.2155 and head is 24.3100
    //     // SO. here it need to check branch names!!!!!!
    //     // it just skips altogether and returns head. why???
    //     let ob = head_ref.peel(git2::ObjectType::Commit)?;
    //     let commit = ob.peel_to_commit()?;

    //     if repo.graph_descendant_of(commit.id(), branch_data.oid)? {
    //         panic!("skip checkout ancestor tree");
    //         let branch = git2::Branch::wrap(head_ref);
    //         return BranchData::from_branch(branch, git2::BranchType::Local);
    //     }
    // }
    let mut builder = git2::build::CheckoutBuilder::new();
    let opts = builder.safe();

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");

    repo.checkout_tree(commit.as_object(), Some(opts))?;
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

    match branch_data.branch_type {
        git2::BranchType::Local => {}
        git2::BranchType::Remote => {
            let created =
                repo.branch(&branch_data.local_name(), &commit, false);
            let mut branch = match created {
                Ok(branch) => branch,
                Err(_) => repo.find_branch(
                    &branch_data.local_name(),
                    git2::BranchType::Local,
                )?,
            };
            branch.set_upstream(Some(&branch_data.remote_name()))?;
            if let Some(new_branch_data) =
                BranchData::from_branch(branch, git2::BranchType::Local)?
            {
                branch_data = new_branch_data;
            }
        }
    }
    repo.set_head(&branch_data.refname)?;
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    branch_data.is_head = true;
    Ok(Some(branch_data))
}

pub fn create_branch(
    path: PathBuf,
    new_branch_name: String,
    need_checkout: bool,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<Option<BranchData>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(branch_data.oid)?;
    let branch = repo.branch(&new_branch_name, &commit, false)?;
    if let Some(new_branch_data) =
        BranchData::from_branch(branch, git2::BranchType::Local)?
    {
        if need_checkout {
            return checkout_branch(path, new_branch_data, sender);
        } else {
            return Ok(Some(new_branch_data));
        }
    }
    Ok(None)
}

pub fn kill_branch(
    path: PathBuf,
    branch_data: BranchData,
    _sender: Sender<crate::Event>,
) -> Result<Option<()>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let name = &branch_data.name;
    let kind = branch_data.branch_type;
    let mut branch = repo.find_branch(name, kind)?;
    if kind == git2::BranchType::Remote {
        gio::spawn_blocking({
            let path = path.clone();
            let name = name.clone();
            move || {
                let repo = git2::Repository::open(path.clone())
                    .expect("can't open repo");
                let mut remote = repo
                    .find_remote("origin") // TODO here is hardcode
                    .expect("no remote");
                let mut opts = git2::PushOptions::new();
                let mut callbacks = git2::RemoteCallbacks::new();
                set_remote_callbacks(&mut callbacks, &None);
                opts.remote_callbacks(callbacks);

                let refspec =
                    format!(":refs/heads/{}", name.replace("origin/", ""),);
                remote
                    .push(&[refspec], Some(&mut opts))
                    .expect("cant push to remote");
            }
        });
    }
    branch.delete()?;
    Ok(Some(()))
}
