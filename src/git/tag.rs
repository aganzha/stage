// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use git2;
use std::path::{PathBuf};

#[derive(Debug, Clone)]
pub struct Tag {
    pub oid: git2::Oid,
    pub name: String,
}

impl Default for Tag {
    fn default() -> Self {
        Self {
            oid: git2::Oid::zero(),
            name: String::from(""),
        }
    }
}


pub fn get_tag_list(path: PathBuf, search_term: Option<String>) -> Result<Vec<Tag>, git2::Error> {
    Ok(Vec::new())
}
