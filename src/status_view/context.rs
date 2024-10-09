// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::{Diff, DiffKind, File, Hunk, Line};

#[derive(Debug, Clone)]
pub enum CursorPosition<'a> {
    CursorDiff(&'a Diff),
    CursorFile(&'a File),
    CursorHunk(&'a Hunk),
    CursorLine(&'a Line),
    None
}

#[derive(Debug, Clone)]
pub struct StatusRenderContext<'a> {

    pub erase_counter: i32,
    
    /// is used to highlight cursor line
    pub cursor_lineno: i32,
    /// same for hunks and line ranges
    pub highlight_lines: Option<(i32, i32)>,
    pub highlight_hunks: Vec<i32>,

    /// introduce
    pub cursor_position: CursorPosition<'a>,

    
    // rename to current as view: active-current etc!
    pub selected_diff: Option<&'a Diff>,
    pub selected_file: Option<&'a File>,
    pub selected_hunk: Option<&'a Hunk>,
    pub selected_line: Option<&'a Line>,

    // this is sliding values during render.
    // at the end of render they will
    // show last visited structures!
    pub rendering_diff: Option<&'a Diff>,
    pub rendering_file: Option<&'a File>,
    pub rendering_hunk: Option<&'a Hunk>,
    pub rendering_line: Option<&'a Line>,
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

                cursor_lineno: 0,
                highlight_lines: None,
                highlight_hunks: Vec::new(),

                cursor_position: CursorPosition::None,
                selected_diff: None,
                selected_file: None,
                selected_hunk: None,
                selected_line: None,

                rendering_diff: None,
                rendering_file: None,
                rendering_hunk: None,
                // it is useless. rendering_x is sliding variable during render
                // and there is nothing to render after line
                rendering_line: None,
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
