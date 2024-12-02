// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::commit::CommitRepr;
use crate::git::{remote::set_remote_callbacks, DeferRefresh};
use async_channel::Sender;
use chrono::{DateTime, FixedOffset};
use git2;
use gtk4::gio;
use log::info;
use std::cmp::Ordering;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct BranchName(String);

impl BranchName {
    pub fn to_str(&self) -> &str {
        &self.0
    }
    pub fn to_local(&self) -> String {
        return self.0.split("/").last().unwrap().to_string();
    }
    pub fn remote_name(&self) -> String {
        return self.0.split("/").next().unwrap().to_string();
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&git2::Branch<'_>> for BranchName {
    fn from(branch: &git2::Branch) -> BranchName {
        let bname = branch.name().unwrap().unwrap().to_string();
        BranchName(bname)
    }
}

#[derive(Debug, Clone)]
pub struct BranchData {
    pub name: BranchName,
    pub refname: String,
    pub branch_type: git2::BranchType,
    pub oid: git2::Oid,
    pub log_message: String,
    pub is_head: bool,
    pub commit_dt: DateTime<FixedOffset>,
    pub remote_name: Option<String>,
}

impl Default for BranchData {
    fn default() -> Self {
        BranchData {
            name: BranchName("".to_string()),
            refname: String::from(""),
            branch_type: git2::BranchType::Local,
            oid: git2::Oid::zero(),
            log_message: String::from(""),
            is_head: false,
            commit_dt: DateTime::<FixedOffset>::MIN_UTC.into(),
            remote_name: None,
        }
    }
}

impl BranchData {
    pub fn from_branch(
        branch: git2::Branch,
        branch_type: git2::BranchType,
    ) -> Result<Option<Self>, git2::Error> {
        let name: BranchName = (&branch).into();
        let is_head = branch.is_head();
        let bref = branch.get();
        let refname = bref.name().unwrap().to_string();
        let ob = bref.peel(git2::ObjectType::Commit)?;
        let commit = ob.peel_to_commit()?;
        let log_message = commit.log_message();
        let commit_dt = commit.dt();
        let remote_name = match branch_type {
            git2::BranchType::Local => {
                if let Ok(ref upstream) = branch.upstream() {
                    Some(BranchName::from(upstream).remote_name())
                } else {
                    None
                }
            }
            git2::BranchType::Remote => Some(name.remote_name()),
        };

        if let Some(oid) = branch.get().target() {
            Ok(Some(BranchData {
                name,
                refname,
                branch_type,
                oid,
                log_message,
                is_head,
                commit_dt,
                remote_name,
            }))
        } else {
            Ok(None)
        }
    }

    // pub fn set_remote_name(&mut self, repo: &git2::Repository) {
    //     match self.branch_type {
    //         git2::BranchType::Local => {
    //             if let Ok(buf) = repo.branch_upstream_remote(&self.refname) {
    //                 self.remote_name = buf.as_str().map(|b| b.to_string());
    //             }
    //         }
    //         git2::BranchType::Remote => {
    //             let mut parts = self.refname.split("/");
    //             assert!(parts.next().unwrap() == "refs");
    //             assert!(parts.next().unwrap() == "remotes");
    //             self.remote_name = parts.next().map(|p| p.to_string())
    //         }
    //     }
    // }
}

pub fn get_branches(path: PathBuf) -> Result<Vec<BranchData>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let mut result = Vec::new();
    let branches = repo.branches(None)?;
    branches.for_each(|item| {
        let (branch, branch_type) = item.unwrap();
        if let Ok(Some(branch_data)) = BranchData::from_branch(branch, branch_type) {
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

        if a.branch_type == git2::BranchType::Local && b.branch_type != git2::BranchType::Local {
            return Ordering::Less;
        }
        if b.branch_type == git2::BranchType::Local && a.branch_type != git2::BranchType::Local {
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
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(branch_data.oid)?;

    let mut builder = git2::build::CheckoutBuilder::new();
    let opts = builder.safe();

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");

    let checkout_error = repo.checkout_tree(commit.as_object(), Some(opts)).err();

    if let Some(checkout_error) = checkout_error {
        return Err(checkout_error);
    }
    match branch_data.branch_type {
        git2::BranchType::Local => {}
        git2::BranchType::Remote => {
            let created = repo.branch(&branch_data.name.to_local(), &commit, false);
            let mut branch = match created {
                Ok(branch) => branch,
                Err(_) => {
                    repo.find_branch(&branch_data.name.to_local(), git2::BranchType::Local)?
                }
            };
            branch.set_upstream(Some(&branch_data.name.to_string()))?;
            if let Some(new_branch_data) = BranchData::from_branch(branch, git2::BranchType::Local)?
            {
                branch_data = new_branch_data;
            }
        }
    }
    repo.set_head(&branch_data.refname)?;

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
    if let Some(new_branch_data) = BranchData::from_branch(branch, git2::BranchType::Local)? {
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
    sender: Sender<crate::Event>,
) -> Result<Option<()>, git2::Error> {
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path.clone())?;
    let name = &branch_data.name;
    let kind = branch_data.branch_type;
    let mut branch = repo.find_branch(name.to_str(), kind)?;
    if kind == git2::BranchType::Remote {
        gio::spawn_blocking({
            let path = path.clone();
            let name = name.clone();
            move || {
                let repo = git2::Repository::open(path.clone()).expect("can't open repo");
                let mut remote = repo
                    .find_remote("origin") // TODO here is hardcode
                    .expect("no remote");
                let mut opts = git2::PushOptions::new();
                let mut callbacks = git2::RemoteCallbacks::new();
                set_remote_callbacks(&mut callbacks, &None);
                opts.remote_callbacks(callbacks);

                let refspec = format!(":refs/heads/{}", name);
                remote
                    .push(&[refspec], Some(&mut opts))
                    .expect("cant push to remote");
            }
        });
    }
    branch.delete()?;
    Ok(Some(()))
}
