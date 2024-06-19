// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::get_current_repo_status;
use async_channel::Sender;
use git2;
use gtk4::gio;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct StashData {
    pub num: usize,
    pub title: String,
    pub oid: git2::Oid,
}

impl StashData {
    pub fn new(num: usize, oid: git2::Oid, title: String) -> Self {
        Self { num, oid, title }
    }
}

impl Default for StashData {
    fn default() -> Self {
        Self {
            oid: git2::Oid::zero(),
            title: String::from(""),
            num: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Stashes {
    pub stashes: Vec<StashData>,
}
impl Stashes {
    pub fn new(stashes: Vec<StashData>) -> Self {
        Self { stashes }
    }
}

pub fn list(path: PathBuf, sender: Sender<crate::Event>) -> Stashes {
    let mut repo =
        git2::Repository::open(path.clone()).expect("can't open repo");
    let mut result = Vec::new();
    repo.stash_foreach(|num, title, oid| {
        result.push(StashData::new(num, *oid, title.to_string()));
        true
    })
    .expect("cant get stash");
    let stashes = Stashes::new(result);
    sender
        .send_blocking(crate::Event::Stashes(stashes.clone()))
        .expect("Could not send through channel");
    stashes
}

pub fn stash(
    path: PathBuf,
    stash_message: String,
    stash_staged: bool,
    sender: Sender<crate::Event>,
) -> Stashes {
    let mut repo =
        git2::Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");
    let flags = if stash_staged {
        git2::StashFlags::empty()
    } else {
        git2::StashFlags::KEEP_INDEX
    };
    let _oid = repo
        .stash_save(&me, &stash_message, Some(flags))
        .expect("cant stash");
    gio::spawn_blocking({
        let path = path.clone();
        let sender = sender.clone();
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    list(path, sender)
}

pub fn apply(
    path: PathBuf,
    num: usize,
    file_path: Option<PathBuf>,
    hunk_header: Option<String>,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let mut repo = git2::Repository::open(path.clone())?;
    // let opts = StashApplyOptions::new();
    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    let mut stash_options = git2::StashApplyOptions::new();
    if let Some(file_path) = file_path {
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.path(file_path);
        stash_options.checkout_options(cb);
    };
    let result = repo.stash_apply(num, Some(&mut stash_options));
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");
    if result.is_err() {
        return result;
    }
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    Ok(())
}

pub fn drop(
    path: PathBuf,
    stash_data: StashData,
    sender: Sender<crate::Event>,
) -> Stashes {
    let mut repo =
        git2::Repository::open(path.clone()).expect("can't open repo");
    repo.stash_drop(stash_data.num).expect("cant drop stash");
    list(path, sender)
}
