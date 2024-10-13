// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::tags;
use core::fmt::{Binary, Formatter, Result};
use std::cell::Cell;

#[derive(Debug, Copy, Clone)]
pub enum ViewState {
    RenderedInPlace,
    Deleted,
    NotYetRendered,
    TagsModified,
    MarkedForDeletion,
    UpdatedFromGit(i32),
    RenderedNotInPlace(i32),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RenderFlags(u8);

impl Default for RenderFlags {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderFlags {
    pub fn new() -> Self {
        Self(0)
    }
    pub fn from(i: u8) -> Self {
        Self(i)
    }
    pub const EXPANDED: u8 = 0b00000001;

    pub fn is_expanded(&self) -> bool {
        self.0 & Self::EXPANDED != 0
    }
    pub fn expand(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::EXPANDED)
        } else {
            Self(self.0 & !Self::EXPANDED)
        }
    }

    pub const SQAUASHED: u8 = 0b00000010;

    pub fn is_squashed(&self) -> bool {
        self.0 & Self::SQAUASHED != 0
    }
    pub fn squash(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::SQAUASHED)
        } else {
            Self(self.0 & !Self::SQAUASHED)
        }
    }

    pub const RENDERED: u8 = 0b00000100;

    pub fn is_rendered(&self) -> bool {
        self.0 & Self::RENDERED != 0
    }
    pub fn render(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::RENDERED)
        } else {
            Self(self.0 & !Self::RENDERED)
        }
    }

    pub const DIRTY: u8 = 0b00001000;

    pub fn is_dirty(&self) -> bool {
        self.0 & Self::DIRTY != 0
    }
    pub fn dirty(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::DIRTY)
        } else {
            Self(self.0 & !Self::DIRTY)
        }
    }

    pub const CHILD_DIRTY: u8 = 0b00010000;

    pub fn is_child_dirty(&self) -> bool {
        self.0 & Self::CHILD_DIRTY != 0
    }
    pub fn child_dirty(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::CHILD_DIRTY)
        } else {
            Self(self.0 & !Self::CHILD_DIRTY)
        }
    }

    pub const ACTIVE: u8 = 0b00100000;

    pub fn is_active(&self) -> bool {
        self.0 & Self::ACTIVE != 0
    }
    pub fn activate(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::ACTIVE)
        } else {
            Self(self.0 & !Self::ACTIVE)
        }
    }

    pub const CURRENT: u8 = 0b01000000;

    pub fn is_current(&self) -> bool {
        self.0 & Self::CURRENT != 0
    }
    pub fn make_current(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::CURRENT)
        } else {
            Self(self.0 & !Self::CURRENT)
        }
    }

    pub const TRANSFERED: u8 = 0b10000000;

    pub fn is_transfered(&self) -> bool {
        self.0 & Self::TRANSFERED != 0
    }
    pub fn transfer(&mut self, value: bool) -> Self {
        if value {
            Self(self.0 | Self::TRANSFERED)
        } else {
            Self(self.0 & !Self::TRANSFERED)
        }
    }
}

impl Binary for RenderFlags {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let val = self.0;
        Binary::fmt(&val, f) // delegate to i32's implementation
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub line_no: Cell<i32>,
    pub flags: Cell<RenderFlags>,
    pub tag_indexes: Cell<tags::TagIdx>,
}

impl View {
    pub fn new() -> Self {
        View {
            line_no: Cell::new(0),
            flags: Cell::new(RenderFlags(0)),
            tag_indexes: Cell::new(tags::TagIdx::new()),
        }
    }

    pub fn expand(&self, value: bool) {
        self.flags.replace(self.flags.get().expand(value));
    }
    pub fn squash(&self, value: bool) {
        self.flags.replace(self.flags.get().squash(value));
    }
    pub fn render(&self, value: bool) {
        self.flags.replace(self.flags.get().render(value));
    }
    pub fn dirty(&self, value: bool) {
        self.flags.replace(self.flags.get().dirty(value));
    }
    pub fn child_dirty(&self, value: bool) {
        self.flags.replace(self.flags.get().child_dirty(value));
    }
    pub fn activate(&self, value: bool) {
        self.flags.replace(self.flags.get().activate(value));
    }

    pub fn make_current(&self, value: bool) {
        self.flags.replace(self.flags.get().make_current(value));
    }
    pub fn transfer(&self, value: bool) {
        self.flags.replace(self.flags.get().transfer(value));
    }

    pub fn is_expanded(&self) -> bool {
        self.flags.get().is_expanded()
    }
    pub fn is_squashed(&self) -> bool {
        self.flags.get().is_squashed()
    }
    pub fn is_rendered(&self) -> bool {
        self.flags.get().is_rendered()
    }
    pub fn is_dirty(&self) -> bool {
        self.flags.get().is_dirty()
    }
    pub fn is_child_dirty(&self) -> bool {
        self.flags.get().is_child_dirty()
    }
    pub fn is_active(&self) -> bool {
        self.flags.get().is_active()
    }
    pub fn is_current(&self) -> bool {
        self.flags.get().is_current()
    }
    pub fn is_transfered(&self) -> bool {
        self.flags.get().is_transfered()
    }

    pub fn repr(&self) -> String {
        format!("line_no: {} rendred: {} squashed: {} active: {} current: {} expanded: {} dirty: {} child_dirty: {}, transfered: {}",
                self.line_no.get(),
                self.is_rendered(),
                self.is_squashed(),
                self.is_active(),
                self.is_current(),
                self.is_expanded(),
                self.is_dirty(),
                self.is_child_dirty(),
                self.is_transfered()
        )
    }

    pub fn is_rendered_in(&self, line_no: i32) -> bool {
        self.is_rendered()
            && self.line_no.get() == line_no
            && !self.is_dirty()
            && !self.is_squashed()
    }

    pub fn get_state_for(&self, line_no: i32) -> ViewState {
        if self.is_rendered_in(line_no) {
            return ViewState::RenderedInPlace;
        }
        if !self.is_rendered() && self.is_squashed() {
            return ViewState::Deleted;
        }
        if !self.is_rendered() {
            return ViewState::NotYetRendered;
        }
        if self.is_dirty() && !self.is_transfered() {
            return ViewState::TagsModified;
        }
        if self.is_dirty() && self.is_transfered() {
            // why not in place? it is in place, just transfered!
            // TODO rename this state. and think about it!
            return ViewState::UpdatedFromGit(self.line_no.get());
        }
        if self.is_squashed() {
            return ViewState::MarkedForDeletion;
        }
        ViewState::RenderedNotInPlace(self.line_no.get())
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}
