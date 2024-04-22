use crate::{DiffKind};

#[derive(Debug, Clone)]
pub struct StatusRenderContext {
    pub erase_counter: Option<i32>,
    pub diff_kind: Option<DiffKind>,
    pub max_len: Option<i32>,
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
                screen_width: None,
            }
        }
    }
}
