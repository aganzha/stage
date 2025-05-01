// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(test)]
use crate::git::conflict;
#[cfg(test)]
use crate::git::{make_diff, DiffKind, DiffLineType, LineKind};
#[cfg(test)]
use crate::tests::initialize;
#[cfg(test)]
use log::debug;
#[cfg(test)]
use std::path;

#[cfg(test)]
pub const WORKDIR_CONTENT: &str = "
from warehouse.tools.rs import (
    record_to_rs,
    add_col_with_check,
    has_field_in_rs,
    join_rs,
<<<<<<< Updated upstream
    add_record_value,
    remove_record_value,
=======
    add_record_value
>>>>>>> Stashed changes
)
from warehouse.tools.constants import NomField
";

#[cfg(test)]
pub const GIT_CONTENT: &str = "
from warehouse.tools.rs import (
    record_to_rs,
    add_col_with_check,
    has_field_in_rs,
    join_rs,
    add_record_value
)
from warehouse.tools.constants import NomField
";

#[gtk4::test]
pub fn test_resolution() {
    initialize();
    let path = "src/test.py";
    let mut bytes: Vec<u8> = Vec::new();
    let text_diff = similar::TextDiff::from_lines(GIT_CONTENT, WORKDIR_CONTENT);
    conflict::write_conflict_diff(&mut bytes, path, text_diff);
    let body = String::from_utf8(bytes.clone()).unwrap();
    for line in body.lines() {
        debug!("{}", line);
    }
    let git_diff = git2::Diff::from_buffer(&bytes).unwrap();
    let diff = make_diff(&git_diff, DiffKind::Conflicted);
    let conflict_hunk = diff.files[0].hunks[0].clone();
    let mut ours = Vec::new();
    let mut theirs = Vec::new();
    let mut markers = Vec::new();
    for line in &conflict_hunk.lines {
        let content = line.content(&conflict_hunk);
        match line.kind {
            LineKind::Ours(_) => ours.push(content),
            LineKind::Theirs(_) => theirs.push(content),
            LineKind::ConflictMarker(_) => markers.push(content),
            _ => panic!("stop"),
        }
    }

    let mut bytes: Vec<u8> = Vec::new();
    conflict::choose_conflict_side_of_hunk(
        path::Path::new(path),
        &conflict_hunk,
        false,
        &mut bytes,
    )
    .unwrap();
    let new_body = String::from_utf8(bytes.clone()).unwrap();
    for line in new_body.lines() {
        debug!("{}", line);
    }
    let git_diff = git2::Diff::from_buffer(&bytes).unwrap();
    let diff = make_diff(&git_diff, DiffKind::Conflicted);
    let hunk = diff.files[0].hunks[0].clone();
    for line in &hunk.lines {
        let content = line.content(&hunk);
        debug!(
            "~~~~~~~~~~~~~~~~~~ {:?} {:?} {:?}",
            line.kind,
            line.origin,
            line.content(&hunk)
        );
        match line.origin {
            DiffLineType::Deletion => {
                assert!(markers.contains(&content) || ours.contains(&content))
            }
            DiffLineType::Context => {
                assert!(theirs.contains(&content))
            }
            _ => panic!("stop"),
        }
    }

    let mut bytes: Vec<u8> = Vec::new();
    conflict::choose_conflict_side_of_hunk(path::Path::new(path), &conflict_hunk, true, &mut bytes)
        .unwrap();
    let new_body = String::from_utf8(bytes.clone()).unwrap();
    for line in new_body.lines() {
        debug!("{}", line);
    }
    let git_diff = git2::Diff::from_buffer(&bytes).unwrap();
    let diff = make_diff(&git_diff, DiffKind::Conflicted);
    let hunk = diff.files[0].hunks[0].clone();
    for line in &hunk.lines {
        let content = line.content(&hunk);
        debug!(
            "~~~~~~~~~~~~~~~~~~ {:?} {:?} {:?}",
            line.kind,
            line.origin,
            line.content(&hunk)
        );
        match line.origin {
            DiffLineType::Deletion => {
                assert!(theirs.contains(&content) || markers.contains(&content))
            }
            DiffLineType::Context => {
                assert!(ours.contains(&content))
            }
            _ => panic!("stop"),
        }
    }
}
