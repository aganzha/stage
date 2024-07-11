// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use git2;
use std::path::{PathBuf};
use log::{info};

#[derive(Debug, Clone)]
pub struct Tag {
    pub oid: git2::Oid,
    pub name: String,
}

impl Tag {
    pub fn new(oid: git2::Oid, name: String) -> Tag {
        Tag {
            oid,
            name
        }
    }
}

impl Default for Tag {
    fn default() -> Self {
        Self {
            oid: git2::Oid::zero(),
            name: String::from(""),
        }
    }
}

pub const TAG_PAGE_SIZE: usize = 100;

pub fn get_tag_list(path: PathBuf, start_oid: Option<git2::Oid>, search_term: Option<String>) -> Result<Vec<Tag>, git2::Error> {
    info!("get_tag_list {:?}", start_oid);
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
        result.push(Tag::new(oid, String::from_utf8_lossy(name).to_string()));
        cnt += 1;
        if cnt == TAG_PAGE_SIZE {
            return false
        }
        true
    });
    info!("returning result {:?}", cnt);
    Ok(result)
}
