// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::tags;
use std::cell::Cell;
use std::fmt;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Display {
    Settled(i32, bool), //my_line_no, tags_are_ok
    Pending,
    Trashed,
    Hidden,
    None,
}
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum State {
    Active,
    Current,
    None,
}
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Switch {
    Expanded,
    PendingExpansion,
    Collapsed,
    PendingCollapsion,
}
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum RenderOp {
    Insert,
    Delete,
    Rewrite,
    UpdateTags,
    Skip,
    None,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ChildExpandOp {
    ForceShow,
    ForceHide,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ExpandOp {
    Children(ChildExpandOp),
    Parent(Switch),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub tag_indexes: Cell<tags::TagIdx>,
    pub display: Cell<Display>,
    pub switch: Cell<Switch>,
    pub state: Cell<State>,
}

impl View {
    pub fn snapshot(&self, line_no: i32) -> RenderOp {
        // there are only insert/rewrite/delete and move!
        // apply tahs was only added for search!
        match self.display.get() {
            Display::None => {
                self.display.replace(Display::Settled(line_no, true));
                RenderOp::Insert
            }
            Display::Settled(_my_line_no, tags_are_ok) => {
                // line is changed here!
                self.display.replace(Display::Settled(line_no, true));
                if tags_are_ok {
                    RenderOp::Skip
                } else {
                    RenderOp::UpdateTags
                }
            }
            Display::Pending => {
                // here i can catch moving. but why?
                self.display.replace(Display::Settled(line_no, true));
                RenderOp::Rewrite
            }
            Display::Trashed => {
                self.display.replace(Display::Hidden);
                RenderOp::Delete
            }
            Display::Hidden => RenderOp::None,
        }
    }
    pub fn needs_children_snapshot(&self) -> bool {
        match self.switch.get() {
            Switch::Expanded => true,
            Switch::PendingExpansion => {
                self.switch.replace(Switch::Expanded);
                true
            }
            Switch::PendingCollapsion => {
                self.switch.replace(Switch::Collapsed);
                true
            }
            Switch::Collapsed => false,
        }
    }
    pub fn apply_child_expand_op(&self, child_expand_op: ChildExpandOp) {
        match child_expand_op {
            ChildExpandOp::ForceShow => {
                self.switch.replace(Switch::PendingExpansion);
                match self.display.get() {
                    Display::Settled(_, _) | Display::Pending | Display::None => {
                        // on next render i will be visible anyways
                        // do nothing
                    }
                    Display::Trashed | Display::Hidden => {
                        // insert me into view
                        self.display.replace(Display::None);
                    }
                }
            }
            ChildExpandOp::ForceHide => {
                self.switch.replace(Switch::PendingCollapsion);
                match self.display.get() {
                    Display::Settled(_, _) | Display::Pending => {
                        // on next render i should be erased!
                        // do nothing
                        self.display.replace(Display::Trashed);
                    }
                    Display::Trashed | Display::Hidden | Display::None => {
                        // i am already invisible or will be erased
                        // do nothing
                    }
                }
            }
        }
    }
    pub fn update_tags(&self) {
        if let Display::Settled(line_no, _) = self.display.get() {
            self.display.replace(Display::Settled(line_no, false));
        }
    }

    pub fn set_switch(&self, value: bool) {
        // this is only for setting switch from code!
        // e.g. display first file expanded.
        // make hunks expanded by default etc.
        if value {
            self.switch.replace(Switch::PendingExpansion);
        } else {
            self.switch.replace(Switch::PendingCollapsion);
        }
    }

    pub fn toggle(&self) -> ChildExpandOp {
        // this is only for calling from UI!
        // when render line by line, current line is passed here as argument
        // if let Display::Settled(my_line_no, _) = self.display.get() {
        //     if line_no == my_line_no {
        match self.switch.get() {
            Switch::Expanded => {
                self.switch.replace(Switch::PendingCollapsion);
                ChildExpandOp::ForceHide
            }
            Switch::Collapsed => {
                self.switch.replace(Switch::PendingExpansion);
                ChildExpandOp::ForceShow
            }
            _ => panic!(
                "🏁 whats the case for toggle? {:?} {:?}",
                self.display, self.switch
            ),
        }
        //     }
        // };
    }

    pub fn is_expanded(&self) -> bool {
        matches!(self.switch.get(), Switch::Expanded)
    }

    pub fn is_rendered(&self) -> bool {
        matches!(self.display.get(), Display::Settled(_, _))
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state.get(), State::Active)
    }
    pub fn is_current(&self) -> bool {
        matches!(self.state.get(), State::Current)
    }
    pub fn is_transfered(&self) -> bool {
        !matches!(self.display.get(), Display::None)
    }

    pub fn is_rendered_in(&self, line_no: i32) -> bool {
        if let Display::Settled(my_line_no, _) = self.display.get() {
            return my_line_no == line_no;
        }
        false
    }
}

impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "display: {:?} switch: {:?} state: {:?}",
            self.display, self.switch, self.state
        )
    }
}

impl Default for View {
    fn default() -> Self {
        View {
            tag_indexes: Cell::new(tags::TagIdx::new()),
            display: Cell::new(Display::None),
            switch: Cell::new(Switch::Collapsed),
            state: Cell::new(State::None),
        }
    }
}
