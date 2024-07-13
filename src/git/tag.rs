// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use git2;
use std::path::{PathBuf};
use log::{info, debug};
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
        if cnt == 0 {
            if let Some(begin_oid) = start_oid {
                if oid != begin_oid {
                    return true;
                }
            }
        }
        if let Ok(tag) =  repo.find_tag(oid) {
            if let Ok(ob) = tag.target() {
                if let Ok(commit) = ob.peel_to_commit() {                    
                    let tag_name = String::from_utf8_lossy(name).to_string();
                    if let Some(look_for) = &search_term {
                        if tag_name.contains(look_for)
                            || tag.message().unwrap_or("").contains(look_for)
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
                        String::from(tag.message().unwrap_or(""))
                    ));
                    cnt += 1;
                    if cnt == TAG_PAGE_SIZE {
                        return false;
                    }
                    return true;
                }
            }
        }
        true
    });
    info!("returning result {:?}", cnt);
    Ok(result)
}
