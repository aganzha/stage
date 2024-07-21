// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::status_view::view::View;
use core::fmt::{Binary, Formatter, Result};
use gtk4::prelude::*;
use gtk4::{pango, gdk, TextTag};
use libadwaita::prelude::*;
use libadwaita::StyleManager;
use log::debug;
use pango::{Style, ShowFlags};

pub const POINTER: &str = "pointer";
pub const STAGED: &str = "staged";
pub const UNSTAGED: &str = "unstaged";

pub const BOLD: &str = "bold";
pub const ADDED: &str = "added";
pub const ENHANCED_ADDED: &str = "enhancedAdded";
pub const REMOVED: &str = "removed";
pub const ENHANCED_REMOVED: &str = "enhancedRemoved";
pub const CURSOR: &str = "cursor";
pub const REGION: &str = "region";
pub const HUNK: &str = "hunk";
pub const ITALIC: &str = "italic";

pub const SPACES_ADDED: &str = "spacesAdded";
pub const SPACES_REMOVED: &str = "spacesRemoved";
pub const LINE_NO_ADDED: &str = "linenoAdded";
pub const LINE_NO_REMOVED: &str = "linenoRemoved";
pub const LINE_NO_CONTEXT: &str = "linenoContext";
pub const CONTEXT: &str = "context";

pub const CONFLICT_MARKER: &str = "conflictmarker";
pub const OURS: &str = "ours";
pub const THEIRS: &str = "theirs";

pub const TEXT_TAGS: [&str; 18] = [
    BOLD,
    ADDED,
    ENHANCED_ADDED,
    REMOVED,
    ENHANCED_REMOVED,
    // CURSOR,
    // REGION,
    // HUNK,
    ITALIC,
    POINTER,
    STAGED,
    UNSTAGED,
    CONFLICT_MARKER,
    OURS,
    THEIRS,

    SPACES_ADDED,
    SPACES_REMOVED,
    LINE_NO_ADDED,
    LINE_NO_REMOVED,
    LINE_NO_CONTEXT,
    CONTEXT

];

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TagIdx(u32);

impl Default for TagIdx {
    fn default() -> Self {
        Self::new()
    }
}

impl TagIdx {
    pub fn new() -> Self {
        Self(0)
    }
    pub fn from(u: u32) -> Self {
        Self(u)
    }
    /// when tag added to view
    /// view will store index of this tag
    /// from global array as bit mask
    pub fn added(self, tag: &TxtTag) -> Self {
        let mut bit_mask: u32 = 1;
        for name in TEXT_TAGS {
            if tag.name() == name {
                break;
            }
            bit_mask <<= 1;
        }
        Self(self.0 | bit_mask)
    }
    /// when tag removed from view
    /// view will remove index of this tag
    /// in global array from bit mask
    pub fn removed(self, tag: &TxtTag) -> Self {
        let mut bit_mask: u32 = 1;
        for name in TEXT_TAGS {
            if tag.name() == name {
                break;
            }
            bit_mask <<= 1;
        }
        Self(self.0 & !bit_mask)
    }

    pub fn is_added(&self, tag: &TxtTag) -> bool {
        let mut bit_mask: u32 = 1;
        for name in TEXT_TAGS {
            if tag.name() == name {
                break;
            }
            bit_mask <<= 1;
        }
        self.0 & bit_mask != 0
    }
}

impl Binary for TagIdx {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let val = self.0;
        Binary::fmt(&val, f) // delegate to i32's implementation
    }
}

impl View {
    pub fn tag_added(&self, tag: &TxtTag) {
        self.tag_indexes.replace(self.tag_indexes.get().added(tag));
    }
    pub fn tag_removed(&self, tag: &TxtTag) {
        self.tag_indexes
            .replace(self.tag_indexes.get().removed(tag));
    }
    pub fn tag_is_added(&self, tag: &TxtTag) -> bool {
        self.tag_indexes.get().is_added(tag)
    }
    pub fn cleanup_tags(&self) {
        self.tag_indexes.replace(TagIdx::new());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TxtTag(String);

impl TxtTag {
    pub fn new(s: String) -> Self {
        if !TEXT_TAGS.contains(&&s[..]) {
            panic!("undeclared tag {}", s);
        }
        Self(s)
    }

    pub fn unknown_tag(s: String) -> Self {
        Self(s)
    }

    pub fn from_str(s: &str) -> Self {
        if !TEXT_TAGS.contains(&s) {
            panic!("undeclared tag {}", s);
        }
        Self(s.to_string())
    }

    pub fn str(&self) -> &str {
        &self.0[..]
    }

    pub fn name(&self) -> &str {
        &self.0[..]
    }

    pub fn enhance(&self) -> Self {
        let new_name = match self.name() {
            ADDED => ENHANCED_ADDED,
            REMOVED => ENHANCED_REMOVED,
            other => other,
        };
        Self::from_str(new_name)
    }

    pub fn fill_text_tag(&self, tag: &TextTag, is_dark: bool) {
        match &self.0[..] {
            SPACES_ADDED => {
                if is_dark {
                    tag.set_background(Some("#4a8e09"));
                } else {
                    tag.set_background(Some("#9bebc6"));
                }
            }
            SPACES_REMOVED => {
                if is_dark {
                    tag.set_background(Some("#a51d2d"));
                } else {
                    tag.set_background(Some("#e4999e"));
                }
            }
            LINE_NO_ADDED => {
                if is_dark {
                    tag.set_foreground_rgba(Some(&gdk::RGBA::parse("#4a8e0999").unwrap()));
                } else {
                    tag.set_foreground_rgba(Some(&gdk::RGBA::parse("#2ec27e55").unwrap()));
                }
            }
            LINE_NO_REMOVED => {
                if is_dark {
                    tag.set_foreground_rgba(Some(&gdk::RGBA::parse("#a51d2d99").unwrap()));
                } else {
                    tag.set_foreground_rgba(Some(&gdk::RGBA::parse("#c01c2855").unwrap()));
                }
            }
            LINE_NO_CONTEXT => {
                // tag.set_priority(100);
                if is_dark {
                    tag.set_foreground_rgba(Some(&gdk::RGBA::new(1.0, 1.0, 1.0, 0.2)));
                } else {

                    tag.set_foreground_rgba(Some(&gdk::RGBA::new(0.0, 0.0, 0.0, 0.2)));
                    // from removed
                    // tag.set_foreground_rgba(Some(&gdk::RGBA::parse("#c01c2855").unwrap()));
                }
            }
            CONTEXT => {
                // tag.set_priority(1);
                // for some reason it affects LINE_NO_CONTEXT.
                // cant find reason...
                if is_dark {
                    // tag.set_foreground_rgba(Some(&gdk::RGBA::new(1.0, 1.0, 1.0, 0.6)));
                } else {
                    // from removed
                    // tag.set_foreground(Some("#c01c28"));
                    // tag.set_foreground_rgba(Some(&gdk::RGBA::new(0.0, 0.0, 0.0, 0.6)));

                }
            }
            BOLD => {
                tag.set_weight(700);
            }
            ADDED => {
                if is_dark {
                    tag.set_foreground(Some("#4a8e09"));
                } else {
                    tag.set_foreground(Some("#2ec27e"));
                }
            }
            ENHANCED_ADDED => {
                if is_dark {
                    tag.set_foreground(Some("#3fb907"));
                } else {
                    tag.set_foreground(Some("#26a269"));
                }
            }
            REMOVED => {
                if is_dark {
                    tag.set_foreground(Some("#a51d2d"));
                } else {
                    tag.set_foreground(Some("#c01c28"));
                }
            }
            ENHANCED_REMOVED => {
                if is_dark {
                    tag.set_foreground(Some("#cd0e1c"));
                } else {
                    tag.set_foreground(Some("#a51d2d"));
                }
            }
            CURSOR => {
                if is_dark {
                    tag.set_background(Some("#23374f"));
                } else {
                    // tag.set_background(Some("#f6fecd")); // original yellow
                    tag.set_background(Some("#cce0f8")); // default blue
                }
            }
            REGION => {
                if is_dark {
                    tag.set_background(Some("#494949"));
                } else {
                    tag.set_background(Some("#f6f5f4"));
                }
            }
            HUNK => {
                if is_dark {
                    tag.set_background(Some("#383838"));
                } else {
                    tag.set_background(Some("#deddda"));
                }
            }
            ITALIC => {
                tag.set_style(Style::Italic);
            }
            POINTER => {}
            STAGED => {}
            UNSTAGED => {}
            CONFLICT_MARKER => {
                if is_dark {
                } else {
                    tag.set_foreground(Some("#ff0000"));
                }
            }
            OURS => {
                if is_dark {
                } else {
                    tag.set_foreground(Some("#2ec27e"));
                }
            }
            THEIRS => {
                if is_dark {
                } else {
                    tag.set_foreground(Some("#2ec27e"));
                }
            }
            unknown => {
                debug!("unknown tag {}", unknown);
            }
        }
    }

    pub fn create(&self) -> TextTag {
        let tag = TextTag::new(Some(&self.0));
        let manager = StyleManager::default();
        let is_dark = manager.is_dark();
        self.fill_text_tag(&tag, is_dark);
        tag
    }
}
