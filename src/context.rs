use crate::{DiffKind, Diff, Hunk, Line};

#[derive(Debug, Clone)]
pub enum UnderCursor<'ctx>{
    None,
    // btw before also could be implemented!
    Some{diff: Option<&'ctx Diff>, hunk: Option<&'ctx Hunk>, line: Option<&'ctx Line>}
}

#[derive(Debug, Clone)]
pub struct StatusRenderContext<'ctx> {
    pub erase_counter: Option<i32>,
    // diff_kind is used by reconcilation
    // it just passes DiffKind down to hunks
    // and lines
    pub diff_kind: Option<DiffKind>,
    pub max_len: Option<i32>,
    pub under_cursor: UnderCursor<'ctx>,
    pub screen_width: Option<(i32, i32)>,
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
                erase_counter: None,
                diff_kind: None,
                max_len: None,
                under_cursor: UnderCursor::None,
                screen_width: None,
            }
        }
    }

    pub fn update_screen_line_width(&mut self, max_line_len: i32) {        
        if let Some(sw) = self.screen_width {
            if sw.1 < max_line_len {
                self.screen_width.replace((sw.0, max_line_len));
            }
        }        
    }
}
