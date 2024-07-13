// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use git2;
use std::path::{PathBuf};
use log::{info, debug};
use async_channel::Sender;
use crate::git::commit::{CommitLog, CommitRelation};

#[derive(Debug, Clone)]
pub struct Tag {
    pub oid: git2::Oid,
    pub name: String,
    pub commit: CommitLog,
    pub message: String
}

impl Tag {
    pub fn new(oid: git2::Oid, name: String, commit: CommitLog, message: String) -> Tag {
        let mut encoded = String::from("");
        html_escape::encode_safe_to_string(message, &mut encoded);
        let name = name.replace("refs/tags/", "");
        Tag {
            oid,
            name,
            commit,
            message: encoded
        }
    }
}


impl Default for Tag {
    fn default() -> Self {
        Self {
            oid: git2::Oid::zero(),
            name: String::from(""),
            commit: CommitLog::default(),
            message: String::from("")
        }
    }
}

pub const TAG_PAGE_SIZE: usize = 100;

pub fn get_tag_list(path: PathBuf, start_oid: Option<git2::Oid>, search_term: Option<String>) -> Result<Vec<Tag>, git2::Error> {
    info!("get_tag_list {:?} {:?}", start_oid, search_term);
    let repo = git2::Repository::open(path.clone())?;
    let mut result = Vec::new();
    let mut cnt = 0;
    repo.tag_foreach(|oid, name| {
        debug!("--------------------> {:?} {:?}", oid, name);
        if cnt == 0 {
            if let Some(begin_oid) = start_oid {
                if oid != begin_oid {
                    return true;
                }
            }
        }
        let mut message = String::from("");
        let commit = {
            if let Ok(tag) =  repo.find_tag(oid) {
                message = String::from(tag.message().unwrap_or(""));
                let ob = tag.target().unwrap();
                ob.peel_to_commit().unwrap()
            } else {
                message = String::from("");
                repo.find_commit(oid).unwrap()
            }
        };
        let tag_name = String::from_utf8_lossy(name).to_string();
        if let Some(look_for) = &search_term {
            if tag_name.contains(look_for)
                || message.contains(look_for)
                || commit.message().unwrap_or("").contains(look_for)
            {
            } else {
                return true
            }
        }
        let commit_log = CommitLog::from_log(commit, CommitRelation::None);
        result.push(Tag::new(
            oid,
            tag_name,
            commit_log,
            message
        ));
        cnt += 1;
        if cnt == TAG_PAGE_SIZE {
            return false;
        }
        true
    });
    info!("returning result {:?}", cnt);
    Ok(result)
}

pub fn create_tag(path: PathBuf, tag_name: String, target_oid: git2::Oid, sender: Sender<crate::Event>,) -> Result<Option<Tag>, git2::Error> {
    info!("create_tag {:?}", target_oid);
    let repo = git2::Repository::open(path.clone())?;
    let target = repo.find_object(target_oid, Some(git2::ObjectType::Commit))?;
    let created_oid = repo.tag_lightweight(&tag_name, &target, false)?;
    // let created_tag = repo.find_tag(created_oid)?;
    let commit = target.peel_to_commit()?;
    let commit_log = CommitLog::from_log(commit, CommitRelation::None);
    Ok(
        Some(
            Tag::new(
                created_oid,
                tag_name,
                commit_log,
                String::from("")
            )
        )
    )
}

pub fn kill_tag(path: PathBuf, tag_name: String, sender: Sender<crate::Event>,) -> Result<Option<()>, git2::Error> {
    info!("kill_tag {:?}", tag_name);
    let repo = git2::Repository::open(path.clone())?;
    repo.tag_delete(&tag_name)?;
    Ok(Some(()))
}
