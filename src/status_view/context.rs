// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::render::ViewContainer;
use crate::status_view::view::State;
use crate::status_view::StageView;
use crate::{git::LineKind, Diff, File, Hunk, Line};
use git2::DiffLineType;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SnapshotData {
    pub line_no: i32,
    pub is_expanded: bool,
    pub added: usize,
    pub removed: usize,
    pub state: State,
}

impl Hunk {
    pub fn snapshot(&self) -> Option<SnapshotData> {
        if let Some(line_no) = self.get_line_no() {
            return Some(SnapshotData {
                line_no,
                is_expanded: self.is_expanded(),
                added: self
                    .lines
                    .iter()
                    .filter(|l| matches!(l.origin, DiffLineType::Addition))
                    .count(),
                removed: self
                    .lines
                    .iter()
                    .filter(|l| matches!(l.origin, DiffLineType::Deletion))
                    .count(),
                state: self.view.state.get(),
            });
        }
        None
    }
}

impl File {
    pub fn snapshot(&self) -> Option<SnapshotData> {
        if let Some(line_no) = self.get_line_no() {
            return Some(SnapshotData {
                line_no,
                is_expanded: self.is_expanded(),
                added: self
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| matches!(l.origin, DiffLineType::Addition))
                    .count(),
                removed: self
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| matches!(l.origin, DiffLineType::Deletion))
                    .count(),
                state: self.view.state.get(),
            });
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct StatusRenderContext<'a> {
    pub stage: &'a StageView,

    pub erase_counter: i32,

    /// same for hunks and line ranges
    pub highlight_lines: Option<(i32, i32)>,
    pub highlight_hunks: Vec<SnapshotData>,

    pub linenos: HashMap<i32, (String, DiffLineType, LineKind)>,

    // rename to current as view: active-current etc!
    pub selected_diff: Option<&'a Diff>,
    pub selected_file: Option<(&'a File, usize)>,
    pub selected_hunk: Option<(&'a Hunk, usize)>,
    pub selected_line: Option<(&'a Line, usize)>,

    // this is sliding values during render/cursor.
    // at the end of render they will
    // show last visited structures!
    pub current_diff: Option<&'a Diff>,
    pub current_file: Option<&'a File>,
    pub current_hunk: Option<&'a Hunk>,
    pub current_line: Option<&'a Line>,

    pub previous_line: Option<&'a Line>,

    // used in fn cursor to check if view is changed during fn cursor
    pub was_current: bool,
    pub search_matched_lines: Vec<i32>,
}

impl<'a> StatusRenderContext<'a> {
    pub fn new(stage: &'a StageView) -> Self {
        {
            Self {
                stage,
                erase_counter: 0,

                highlight_lines: None,
                highlight_hunks: Vec::new(),

                linenos: HashMap::new(),

                selected_diff: None,
                selected_file: None,
                selected_hunk: None,
                selected_line: None,

                current_diff: None,
                current_file: None,
                current_hunk: None,

                current_line: None,
                previous_line: None,
                was_current: false,
                search_matched_lines: Vec::new(),
            }
        }
    }

    // pub fn collect_hunk_highlights(&mut self, line_no: i32) {
    //     self.highlight_hunks.push(line_no);
    // }

    pub fn collect_line_highlights(&mut self, line_no: i32) {
        match self.highlight_lines {
            Some((from, to)) if line_no < from => {
                self.highlight_lines.replace((line_no, to));
            }
            Some((from, to)) if line_no > to => {
                self.highlight_lines.replace((from, line_no));
            }
            Some((from, to)) if from <= line_no && line_no <= to => {}
            None => {
                self.highlight_lines.replace((line_no, line_no));
            }
            _ => {
                todo!("whats the case? {:?} {:?}", self.highlight_lines, line_no)
            }
        }
    }
    pub fn cursor_is_on_diff(&self) -> bool {
        self.selected_diff.is_some() && self.selected_file.is_none()
    }
    pub fn has_selected(&self) -> bool {
        self.selected_diff.is_some()
            || self.selected_file.is_some()
            || self.selected_hunk.is_some()
            || self.selected_line.is_none()
    }
}
