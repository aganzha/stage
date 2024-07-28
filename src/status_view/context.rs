// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::{Diff, DiffKind, File, Hunk, Line, LineKind};

use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum UnderCursor {
    None,
    Some {
        diff_kind: DiffKind,
        line_kind: LineKind,
    },
}

#[derive(Debug, Clone)]
pub struct CursorPos {
    pub line_no: i32,
    pub offset: i32,
}

#[derive(Debug, Clone, Default)]
pub struct TextViewWidth {
    pub pixels: i32,
    pub chars: i32, // count of chars in max line on screen
    pub visible_chars: i32, // count of visible chars on screen. now used only in commit view for line wrapping.
}

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
    pub screen_width: Option<Rc<RefCell<TextViewWidth>>>,
    pub cursor: i32,
    pub highlight_lines: Option<(i32, i32)>,
    pub highlight_hunks: Vec<i32>,

    pub cursor_diff: Option<&'a Diff>,
    pub cursor_file: Option<&'a File>,
    pub cursor_hunk: Option<&'a Hunk>,
    pub cursor_line: Option<&'a Line>,

    pub current_diff: Option<&'a Diff>,
    pub current_file: Option<&'a File>,
    pub current_hunk: Option<&'a Hunk>,
    pub current_line: Option<&'a Line>,
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
                // under_cursor: UnderCursor::None,
                screen_width: None,
                cursor: 0,
                highlight_lines: None,
                highlight_hunks: Vec::new(),

                cursor_diff: None,
                cursor_file: None,
                cursor_hunk: None,
                cursor_line: None,

                current_diff: None,
                current_file: None,
                current_hunk: None,
                // it is useless. current_x is sliding variable during render
                // and there is nothing to render after line
                current_line: None,
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

    pub fn update_screen_line_width(&mut self, max_line_len: i32) {
        if let Some(sw) = &self.screen_width {
            if sw.borrow().chars < max_line_len {
                sw.borrow_mut().chars = max_line_len;
            }
        }
    }

    pub fn under_cursor_hunk(&mut self, _hunk: &Hunk) {}
}
