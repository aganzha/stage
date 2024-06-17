// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use std::path::{Path, PathBuf};

pub trait StrPath {
    fn as_str(&self) -> &str;
}

impl StrPath for PathBuf {
    fn as_str(&self) -> &str {
        self.to_str().unwrap()
    }
}

// impl StrPath for Option<PathBuf> {
//     fn as_str(&self) -> &str {
//         let path = self.unwrap();
//         path.as_str()
//     }
// }
