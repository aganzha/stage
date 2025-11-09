// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::git::DeferRefresh;
use async_channel::Sender;
use git2;

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct StashNum(usize);

impl StashNum {
    pub fn new(num: usize) -> Self {
        Self(num)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
    pub fn as_i32(&self) -> i32 {
        self.0 as i32
    }
}

#[derive(Debug, Clone)]
pub struct StashData {
    pub num: StashNum,
    pub title: String,
    pub oid: git2::Oid,
}

impl StashData {
    pub fn new(num: usize, oid: git2::Oid, title: String) -> Self {
        Self {
            num: StashNum(num),
            oid,
            title,
        }
    }
}

impl Default for StashData {
    fn default() -> Self {
        Self {
            oid: git2::Oid::zero(),
            title: String::from(""),
            num: StashNum(0),
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
    let mut repo = git2::Repository::open(path.clone()).expect("can't open repo");
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
    file_path: Option<PathBuf>,
    sender: Sender<crate::Event>,
) -> Result<Option<Stashes>, git2::Error> {
    let _defer = DeferRefresh::new(path.clone(), sender.clone(), true, false);
    let mut repo = git2::Repository::open(path.clone())?;
    let me = repo.signature()?;
    let flags = if stash_staged {
        git2::StashFlags::empty()
    } else {
        git2::StashFlags::KEEP_INDEX
    };

    if let Some(path) = file_path {
        let mut options = git2::StashSaveOptions::new(me);
        options.flags(Some(flags));
        options.pathspec(path);
        repo.stash_save_ext(Some(&mut options))?;
    } else {
        repo.stash_save(&me, &stash_message, Some(flags))?;
    }
    Ok(Some(list(path, sender)))
}

pub fn apply(
    path: PathBuf,
    num: StashNum,
    file_path: Option<PathBuf>,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let _defer = DeferRefresh::new(path.clone(), sender.clone(), true, true);

    let mut repo = git2::Repository::open(path.clone())?;
    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    let mut stash_options = git2::StashApplyOptions::new();
    if let Some(file_path) = file_path {
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.path(file_path);
        stash_options.checkout_options(cb);
    };
    repo.stash_apply(num.as_usize(), Some(&mut stash_options))?;
    Ok(())
}

pub fn drop(path: PathBuf, stash_data: StashData, sender: Sender<crate::Event>) -> Stashes {
    let mut repo = git2::Repository::open(path.clone()).expect("can't open repo");
    repo.stash_drop(stash_data.num.as_usize())
        .expect("cant drop stash");
    list(path, sender)
}
