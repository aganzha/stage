// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::view::View;
use core::fmt::{Binary, Formatter, Result};
use gtk4::prelude::*;
use gtk4::{pango, TextTag};
use libadwaita::StyleManager;
use log::debug;
use pango::Underline;

pub const POINTER: &str = "pointer";
pub const STAGED: &str = "staged";
pub const UNSTAGED: &str = "unstaged";

pub const DIFF: &str = "diff";

pub const BOLD: &str = "bold";
pub const ADDED: &str = "added";
pub const ENHANCED_ADDED: &str = "enhancedAdded";
pub const SYNTAX_ADDED: &str = "syntaxAdded";

pub const REMOVED: &str = "removed";
pub const ENHANCED_REMOVED: &str = "enhancedRemoved";
pub const SYNTAX_REMOVED: &str = "syntaxRemoved";

pub const CURSOR: &str = "cursor";
pub const REGION: &str = "region";

pub const HUNK: &str = "hunk";
pub const FILE: &str = "file";
pub const OID: &str = "oid";

pub const UNDERLINE: &str = "italic";

pub const SPACES_ADDED: &str = "spacesAdded";
pub const SPACES_REMOVED: &str = "spacesRemoved";

pub const CONFLICT_MARKER: &str = "conflictmarker";
pub const OURS: &str = "ours";
pub const THEIRS: &str = "theirs";

pub const CONTEXT: &str = "context";
pub const SYNTAX: &str = "syntax";

pub const TEXT_TAGS: [&str; 19] = [
    BOLD,
    ADDED,
    ENHANCED_ADDED,
    REMOVED,
    ENHANCED_REMOVED,
    // CURSOR,
    // REGION,
    // HUNK,
    DIFF,
    HUNK,
    FILE,
    OID,
    UNDERLINE,
    POINTER,
    STAGED,
    UNSTAGED,
    CONFLICT_MARKER,
    OURS,
    THEIRS,
    SPACES_ADDED,
    SPACES_REMOVED,
    CONTEXT,
];

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Color(pub (String, String));

impl Color {
    pub fn for_light_scheme(&self) -> String {
        self.0 .0.clone()
    }
    pub fn for_dark_scheme(&self) -> String {
        self.0 .1.clone()
    }
    pub fn darken_color(hex: &str, factor: Option<f32>) -> String {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap();

        // Default factor to 0.1 (10%) if not provided
        let factor = factor.unwrap_or(0.1);

        // Darken the color
        let darken = |c: u8| -> u8 { (c as f32 * (1.0 - factor)).round() as u8 };

        let new_r = darken(r);
        let new_g = darken(g);
        let new_b = darken(b);

        // Format the new color back to hex
        format!("#{:02x}{:02x}{:02x}", new_r, new_g, new_b)
    }
    pub fn darken(&self, factor: Option<f32>) -> Self {
        let fg = Self::darken_color(&self.0 .0, factor);
        let bg = Self::darken_color(&self.0 .1, factor);
        Self((fg, bg))
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct ColorTag(pub (&'static str, Color));

impl ColorTag {
    pub fn create(&self, is_dark: bool) -> TextTag {
        let tag = TextTag::new(Some(self.0 .0));
        self.toggle(&tag, is_dark);
        tag
    }
    pub fn toggle(&self, tag: &TextTag, is_dark: bool) {
        if is_dark {
            tag.set_foreground(Some(&self.0 .1 .0 .0));
        } else {
            tag.set_foreground(Some(&self.0 .1 .0 .1));
        }
    }
}

//pub const T_ADDED: ColorTag = ColorTag("ADDED");

#[derive(Debug, Clone, PartialEq)]
pub struct TxtTag(String);

impl TxtTag {
    pub fn new(s: String) -> Self {
        if !TEXT_TAGS.contains(&&s[..]) {
            panic!("undeclared tag {}", s);
        }
        Self(s)
    }

    pub fn fg_bg_color(&self) -> (Option<&str>, Option<&str>) {
        match &self.0[..] {
            ADDED => (Some("#4a8e09"), None),
            _ => (None, None),
        }
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
            BOLD => {
                tag.set_weight(700);
            }
            // ADDED => {
            //     if is_dark {
            //         tag.set_foreground(Some("#4a8e09"));
            //     } else {
            //         tag.set_foreground(Some("#2ec27e"));
            //     }
            // }
            // ENHANCED_ADDED => {
            //     if is_dark {
            //         tag.set_foreground(Some("#3fb907"));
            //     } else {
            //         tag.set_foreground(Some("#26a269"));
            //     }
            // }
            // REMOVED => {
            //     if is_dark {
            //         tag.set_foreground(Some("#a51d2d"));
            //     } else {
            //         tag.set_foreground(Some("#c01c28"));
            //     }
            // }
            // ENHANCED_REMOVED => {
            //     if is_dark {
            //         tag.set_foreground(Some("#cd0e1c"));
            //     } else {
            //         tag.set_foreground(Some("#a51d2d"));
            //     }
            // }
            // CURSOR => {
            //     if is_dark {
            //         tag.set_background(Some("#23374f"));
            //     } else {
            //         // tag.set_background(Some("#f6fecd")); // original yellow
            //         tag.set_background(Some("#cce0f8")); // default blue
            //     }
            // }
            // REGION => {
            //     if is_dark {
            //         tag.set_background(Some("#494949"));
            //     } else {
            //         tag.set_background(Some("#f6f5f4"));
            //     }
            // }
            // HUNK => {
            //     // if is_dark {
            //     //     tag.set_background(Some("#383838"));
            //     // } else {
            //     //     tag.set_background(Some("#deddda"));
            //     // }
            // }
            UNDERLINE => {
                tag.set_underline(Underline::Single);
            }
            // POINTER => {}
            // STAGED | UNSTAGED => {}
            DIFF => {
                // TODO! get it from line_yrange!
                tag.set_weight(700);
                tag.set_pixels_above_lines(32);
                if is_dark {
                    tag.set_foreground(Some("#a78a44"));
                } else {
                    tag.set_foreground(Some("#8b6508"));
                }
            }
            CONFLICT_MARKER => {
                tag.set_foreground(Some("#ff0000"));
            }
            // OURS => {}
            // THEIRS => {}
            unknown => {
                debug!("skip tag ...... {}", unknown);
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

    pub fn added_tags(&self) -> Vec<TxtTag> {
        let mut bit_mask: u32 = 1;
        let mut result = Vec::new();
        for name in TEXT_TAGS {
            if self.0 & bit_mask != 0 {
                result.push(TxtTag::from_str(name));
            }
            bit_mask <<= 1;
        }
        result
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
        // hack for SYNTAX
        if tag.0 == BOLD {
            return;
        }
        self.tag_indexes.replace(self.tag_indexes.get().added(tag));
    }
    pub fn tag_removed(&self, tag: &TxtTag) {
        self.tag_indexes
            .replace(self.tag_indexes.get().removed(tag));
    }
    pub fn tag_is_added(&self, tag: &TxtTag) -> bool {
        // haack, for SYNTAX
        if tag.0 == BOLD {
            return false;
        }
        self.tag_indexes.get().is_added(tag)
    }
    pub fn cleanup_tags(&self) {
        self.tag_indexes.replace(TagIdx::new());
    }
    pub fn added_tags(&self) -> Vec<TxtTag> {
        self.tag_indexes.get().added_tags()
    }
}
