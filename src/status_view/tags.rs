// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::view::View;
use core::fmt::{Binary, Formatter, Result};
use gtk4::prelude::*;
use gtk4::{TextTag, TextTagTable};

pub const POINTER: &str = "pointer";
pub const STAGED: &str = "staged";
pub const UNSTAGED: &str = "unstaged";

pub const DIFF: &str = "diff";

pub const BOLD: &str = "bold";
pub const ADDED: &str = "added";
pub const ENHANCED_ADDED: &str = "enhancedAdded";
pub const SYNTAX_ADDED: &str = "syntaxAdded";
pub const SYNTAX_1_ADDED: &str = "syntax1Added";
pub const ENHANCED_SYNTAX_ADDED: &str = "enhancedSyntaxAdded";
pub const ENHANCED_SYNTAX_1_ADDED: &str = "enhancedSyntax1Added";

pub const REMOVED: &str = "removed";
pub const ENHANCED_REMOVED: &str = "enhancedRemoved";
pub const SYNTAX_REMOVED: &str = "syntaxRemoved";
pub const SYNTAX_1_REMOVED: &str = "syntax1Removed";
pub const ENHANCED_SYNTAX_REMOVED: &str = "enhancedSyntaxRemoved";
pub const ENHANCED_SYNTAX_1_REMOVED: &str = "enhancedSyntax1Removed";

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
pub const ENHANCED_CONTEXT: &str = "enhancedContext";

pub const SYNTAX: &str = "syntax";
pub const SYNTAX_1: &str = "syntax1";
pub const ENHANCED_SYNTAX: &str = "enhancedSyntax";
pub const ENHANCED_SYNTAX_1: &str = "enhancedSyntax";

// THE ORDER HERE IS IMPORTANT!
// if swap context and syntax, then syntax tags will not be visible in context lines!
pub const TEXT_TAGS: [&str; 32] = [
    BOLD,
    ADDED,
    ENHANCED_ADDED,
    REMOVED,
    ENHANCED_REMOVED,
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
    ENHANCED_CONTEXT,
    SYNTAX,
    SYNTAX_ADDED,
    SYNTAX_REMOVED,
    ENHANCED_SYNTAX,
    ENHANCED_SYNTAX_ADDED,
    ENHANCED_SYNTAX_REMOVED,
    SYNTAX_1,
    SYNTAX_1_ADDED,
    SYNTAX_1_REMOVED,
    ENHANCED_SYNTAX_1,
    ENHANCED_SYNTAX_1_ADDED,
    ENHANCED_SYNTAX_1_REMOVED,
];

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Color(pub (String, String));

impl Color {
    pub fn adjust_color(hex: &str, factor: f32) -> String {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap();

        // Adjust the color
        let adjust = |c: u8| -> u8 {
            // Clamp the value between 0 and 255
            (c as f32 * (1.0 + factor)).round() as u8
        };

        let new_r = adjust(r);
        let new_g = adjust(g);
        let new_b = adjust(b);

        // Format the new color back to hex
        format!("#{:02x}{:02x}{:02x}", new_r, new_g, new_b)
    }

    pub fn darken(&self, factor: Option<f32>) -> Self {
        let f = factor.unwrap_or(0.1);
        let fg = Self::adjust_color(&self.0 .0, f);
        let bg = Self::adjust_color(&self.0 .1, 0.0 - f);
        Self((fg, bg))
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Tag(pub &'static str);

impl Tag {
    pub fn create(&self, table: &TextTagTable) -> TextTag {
        let tag = TextTag::new(Some(self.0));
        table.add(&tag);
        tag
    }

    pub fn enhance(&self) -> Self {
        match self.0 {
            "ADDED" => Self("ENHANCED_ADDED"),
            "REMOVED" => Self("ENHANCED_REMOVED"),
            "CONTEXT" => Self("ENHANCED_CONTEXT"),
            "SYNTAX" => Self("ENHANCED_SYNTAX"),
            "SYNTAX_ADDED" => Self("ENHANCED_SYNTAX_ADDED"),
            "SYNTAX_REMOVED" => Self("ENHANCED_SYNTAX_REMOVED"),
            "SYNTAX_1" => Self("ENHANCED_SYNTAX_1"),
            "SYNTAX_1_ADDED" => Self("ENHANCED_SYNTAX_1_ADDED"),
            "SYNTAX_1_REMOVED" => Self("ENHANCED_SYNTAX_1_REMOVED"),
            name => Self(name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct ColorTag(pub (&'static str, Color));

impl ColorTag {
    pub fn create(&self, table: &TextTagTable, is_dark: bool) -> TextTag {
        let tag = TextTag::new(Some(self.0 .0));
        self.toggle(&tag, is_dark);
        table.add(&tag);
        tag
    }
    pub fn toggle(&self, tag: &TextTag, is_dark: bool) {
        if is_dark {
            tag.set_foreground(Some(&self.0 .1 .0 .0));
            if self.0 .0 == SPACES_ADDED || self.0 .0 == SPACES_REMOVED {
                tag.set_background(Some(&self.0 .1 .0 .0));
            }
        } else {
            tag.set_foreground(Some(&self.0 .1 .0 .1));
            if self.0 .0 == SPACES_ADDED || self.0 .0 == SPACES_REMOVED {
                tag.set_background(Some(&self.0 .1 .0 .0));
            }
        }
    }
}

pub trait ITag {
    fn name(&self) -> &'static str;
}

impl ITag for Tag {
    fn name(&self) -> &'static str {
        self.0
    }
}

impl ITag for ColorTag {
    fn name(&self) -> &'static str {
        self.0 .0
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
    pub fn added(self, tag: &'static str) -> Self {
        let mut bit_mask: u32 = 1;
        for name in TEXT_TAGS {
            if tag == name {
                break;
            }
            bit_mask <<= 1;
        }
        Self(self.0 | bit_mask)
    }
    /// when tag removed from view
    /// view will remove index of this tag
    /// in global array from bit mask
    pub fn removed(self, tag: &'static str) -> Self {
        let mut bit_mask: u32 = 1;
        for name in TEXT_TAGS {
            if tag == name {
                break;
            }
            bit_mask <<= 1;
        }
        Self(self.0 & !bit_mask)
    }

    pub fn is_added(&self, tag: &'static str) -> bool {
        let mut bit_mask: u32 = 1;
        for name in TEXT_TAGS {
            if tag == name {
                break;
            }
            bit_mask <<= 1;
        }
        self.0 & bit_mask != 0
    }

    pub fn added_tags(&self) -> Vec<&'static str> {
        let mut bit_mask: u32 = 1;
        let mut result = Vec::new();
        for name in TEXT_TAGS {
            if self.0 & bit_mask != 0 {
                result.push(name);
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
    pub fn tag_added(&self, tag: &'static str) {
        self.tag_indexes.replace(self.tag_indexes.get().added(tag));
    }
    pub fn tag_removed(&self, tag: &'static str) {
        self.tag_indexes
            .replace(self.tag_indexes.get().removed(tag));
    }
    pub fn tag_is_added(&self, tag: &'static str) -> bool {
        self.tag_indexes.get().is_added(tag)
    }
    pub fn cleanup_tags(&self) {
        self.tag_indexes.replace(TagIdx::new());
    }
    pub fn added_tags(&self) -> Vec<&'static str> {
        self.tag_indexes.get().added_tags()
    }
}
