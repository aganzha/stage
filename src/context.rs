use crate::{DiffKind, Diff, Hunk, Line};

#[derive(Debug, Clone)]
pub enum UnderCursor {
    None,
    // btw before also could be implemented!
    Some{diff: Option<Diff>, hunk: Option<Hunk>, line: Option<Line>}
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
    pub screen_width: Option<(i32, i32)>,
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
            }
        }
    }
}
