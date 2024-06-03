use crate::{DiffKind, LineKind};

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
    pub chars: i32,
}

#[derive(Debug, Clone)]
pub struct StatusRenderContext {
    pub erase_counter: Option<i32>,
    // diff_kind is used by reconcilation
    // it just passes DiffKind down to hunks
    // and lines
    pub diff_kind: Option<DiffKind>,
    pub max_len: Option<i32>,
    pub under_cursor: UnderCursor,
    pub screen_width: Option<Rc<RefCell<TextViewWidth>>>,
    pub cursor_pos: Option<CursorPos>,
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
                cursor_pos: None,
            }
        }
    }

    pub fn update_cursor_pos(&mut self, line_no: i32, offset: i32) {
        self.cursor_pos.replace(CursorPos { line_no, offset });
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
