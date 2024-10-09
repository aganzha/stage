// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::{Diff, DiffKind, File, Hunk, Line};


// #[derive(Debug, Clone)]
// pub enum UserOpLine {
//     Diff(i32),
//     File(i32),
//     Hunk(i32),
//     Line(i32),
// }
// #[derive(Debug, Clone)]
// pub struct UserOp {
//     diff_kind: DiffKind,
//     line: UserOpLine,
// }

#[derive(Debug, Clone)]
pub struct StatusRenderContext<'a> {
    pub erase_counter: i32,
    /// diff_kind is used by reconcilation
    /// it just passes DiffKind down to hunks
    /// and lines
    pub diff_kind: Option<DiffKind>,
    // TODO! kill it!
    pub max_len: Option<i32>,
    // TODO! kill it!
    pub cursor: i32,
    pub highlight_lines: Option<(i32, i32)>,
    pub highlight_hunks: Vec<i32>,

    // rename to current as view: active-current etc!
    pub cursor_diff: Option<&'a Diff>,
    pub cursor_file: Option<&'a File>,
    pub cursor_hunk: Option<&'a Hunk>,
    pub cursor_line: Option<&'a Line>,

    // this is sliding values during render.
    // at the end of render they will
    // show last visited structures!
    pub sliding_diff: Option<&'a Diff>,
    pub sliding_file: Option<&'a File>,
    pub sliding_hunk: Option<&'a Hunk>,
    pub sliding_line: Option<&'a Line>,
}

impl Default for StatusRenderContext<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusRenderContext<'_> {
    pub fn new() -> Self {
        {
            Self {
                erase_counter: 0,
                diff_kind: None,
                max_len: None,
                cursor: 0,
                highlight_lines: None,
                highlight_hunks: Vec::new(),

                cursor_diff: None,
                cursor_file: None,
                cursor_hunk: None,
                cursor_line: None,

                sliding_diff: None,
                sliding_file: None,
                sliding_hunk: None,
                // it is useless. sliding_x is sliding variable during render
                // and there is nothing to render after line
                sliding_line: None,
            }
        }
    }

    pub fn collect_hunk_highlights(&mut self, line_no: i32) {
        self.highlight_hunks.push(line_no);
    }

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
                todo!(
                    "whats the case? {:?} {:?}",
                    self.highlight_lines,
                    line_no
                )
            }
        }
    }
}
