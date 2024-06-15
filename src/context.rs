use crate::{DiffKind, LineKind};
use crate::status_view::container::ViewKind;

use std::cell::RefCell;
use std::rc::Rc;
use log::{debug, trace};

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
    pub chars: i32,// count of chars in max line on screen  
    pub visible_chars: i32, // count of visible chars on screen. now used only in commit view for line wrapping.
}

#[derive(Debug, Clone)]
pub struct StatusRenderContext {
    pub erase_counter: Option<i32>,
    /// diff_kind is used by reconcilation
    /// it just passes DiffKind down to hunks
    /// and lines
    pub diff_kind: Option<DiffKind>,
    // TODO! kill it!
    pub max_len: Option<i32>,
    pub under_cursor: UnderCursor,
    // TODO! kill it!
    pub screen_width: Option<Rc<RefCell<TextViewWidth>>>,
    pub highlight_lines: Option<(i32, i32)>,
    pub highlight_hunks: Vec<i32>
    // pub cursor_pos: Option<CursorPos>,
}

impl Default for StatusRenderContext {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusRenderContext {
    pub fn new() -> Self {
        {
            Self {
                erase_counter: None,
                diff_kind: None,
                max_len: None,
                under_cursor: UnderCursor::None,
                screen_width: None,
                highlight_lines: None,
                highlight_hunks: Vec::new()
            }
        }
    }

    // pub fn update_cursor_pos(&mut self, line_no: i32, offset: i32) {
    //     self.cursor_pos.replace(CursorPos { line_no, offset });
    // }

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
            Some((from, to)) if from <= line_no && line_no <= to => {
            }
            None => {
                self.highlight_lines.replace((line_no, line_no));
            },
            _ => {
                todo!("whats the case? {:?} {:?}", self.highlight_lines, line_no)
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

    pub fn under_cursor_diff(&mut self, kind: &DiffKind) {
        match &self.under_cursor {
            UnderCursor::None => LineKind::None,
            UnderCursor::Some {
                diff_kind: _,
                line_kind: LineKind::None,
            } => LineKind::None,
            UnderCursor::Some {
                diff_kind: _,
                line_kind: _,
            } => {
                // diff kind is set on top of cursor, when line_kind
                // is empty
                // but if line_kind is not empty - do not change diff_kind!
                return;
            }
        };
        self.under_cursor = UnderCursor::Some {
            diff_kind: kind.clone(),
            line_kind: LineKind::None,
        };
    }

    pub fn under_cursor_line(&mut self, kind: &LineKind) {
        let diff_kind = match &self.under_cursor {
            UnderCursor::Some {
                diff_kind: dk,
                line_kind: _,
            } => dk.clone(),
            UnderCursor::None => {
                panic!("diff kind must be set already");
            }
        };
        self.under_cursor = UnderCursor::Some {
            diff_kind,
            line_kind: kind.clone(),
        };
    }
}
