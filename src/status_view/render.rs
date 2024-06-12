//use crate::status_view::Tag;
use crate::status_view::tags;
use core::fmt::{Binary, Formatter, Result};
use gtk4::prelude::*;
use gtk4::{TextBuffer, TextIter};
use log::{debug, trace};
use std::cell::Cell;
use std::collections::HashSet;

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

pub fn make_tag(name: &str) -> tags::TxtTag {
    tags::TxtTag::from_str(name)
}

pub fn play_with_tags() {
    debug!("play_with_tags--------------------");
}

impl View {
    pub fn new() -> Self {
        View {
            line_no: Cell::new(0),
            // expanded: false,
            // squashed: false,
            // rendered: false,
            // dirty: false,
            // child_dirty: false,
            // active: false,
            // current: false,
            // transfered: false,
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
    pub fn is_rendered_in(&self, line_no: i32) -> bool {
        self.is_rendered()
            && self.line_no.get() == line_no
            && !self.is_dirty()
            && !self.is_squashed()
    }

    fn does_not_match_width(
        &self,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) -> bool {
        if let Some(width) = &context.screen_width {
            let chars = width.borrow().chars;
            if chars > 0 {
                let (start, end) = self.start_end_iters(buffer);
                let len = buffer.slice(&start, &end, true).len() as i32;
                if chars - 1 > len {
                    trace!("rendered content is less then screen width");
                    return true;
                }
            }
        }

        false
    }

    fn replace_dirty_content(
        &self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        content: &str,
        is_markup: bool,
    ) {
        let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
        eol_iter.forward_to_line_end();
        buffer.remove_all_tags(iter, &eol_iter);
        self.cleanup_tags();
        buffer.delete(iter, &mut eol_iter);
        if is_markup {
            buffer.insert_markup(iter, content);
        } else {
            buffer.insert(iter, content);
        }
    }

    fn build_up(
        &self,
        content: &String,
        _line_no: i32,
        context: &mut crate::StatusRenderContext,
    ) -> String {
        let line_content = content.to_string();

        if let Some(width) = &context.screen_width {
            let pixels = width.borrow().pixels;
            let chars = width.borrow().chars;
            trace!(
                "build_up. context width in pixels and chars {:?} {:?}",
                pixels,
                chars
            );
            if chars > 0 {
                trace!(
                    "build_up. line and line length {:?} {:?}",
                    line_content,
                    line_content.len()
                );
                if chars as usize > line_content.len() {
                    let spaces = chars as usize - line_content.len();
                    trace!("build up spaces {:?}", spaces);
                    return format!("{}{}", line_content, " ".repeat(spaces));
                }
            }
        }

        line_content
    }

    // View
    pub fn render_in_textview(
        &self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        content: String,
        is_markup: bool,
        content_tags: Vec<tags::TxtTag>,
        context: &mut crate::StatusRenderContext,
    ) -> &Self {
        // important. self.line_no is assigned only in 2 cases
        // below!!!!

        let line_no = iter.line();
        trace!(
            "======= line {:?} render view {:?} which is at line {:?}. sstate: {:?}",
            line_no,
            content,
            self.line_no,
            self.get_state_for(line_no)
        );
        match self.get_state_for(line_no) {
            ViewState::RenderedInPlace => {
                trace!("..render MATCH rendered_in_line {:?}", line_no);
                iter.forward_lines(1);
            }
            ViewState::Deleted => {
                // nothing todo. calling render on
                // some whuch will be destroyed
                trace!("..render MATCH !rendered squashed {:?}", line_no);
            }
            ViewState::NotYetRendered => {
                trace!("..render MATCH insert {:?}", line_no);
                let content = self.build_up(&content, line_no, context);
                if is_markup {
                    buffer.insert_markup(iter, &format!("{}\n", content));
                } else {
                    buffer.insert(iter, &format!("{}\n", content));
                }
                self.line_no.replace(line_no);
                self.render(true);
                if !content.is_empty() {
                    self.apply_tags(buffer, &content_tags);
                }
            }
            ViewState::TagsModified => {
                trace!("..render MATCH TagsModified {:?}", line_no);
                // this means only tags are changed.
                if self.does_not_match_width(buffer, context) {
                    // here is the case: view is rendered before resize event.
                    // max width is detected by diff max width and then resize
                    // event is come with larger with
                    let content = self.build_up(&content, line_no, context);
                    self.replace_dirty_content(
                        buffer, iter, &content, is_markup,
                    );
                }
                self.apply_tags(buffer, &content_tags);
                if !iter.forward_lines(1) {
                    assert!(iter.offset() == buffer.end_iter().offset());
                }
                self.render(true);
            }
            ViewState::MarkedForDeletion => {
                trace!("..render MATCH squashed {:?}", line_no);
                let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
                nel_iter.forward_lines(1);
                buffer.delete(iter, &mut nel_iter);
                self.render(false);
                self.cleanup_tags();

                if let Some(ec) = context.erase_counter {
                    context.erase_counter.replace(ec + 1);
                } else {
                    context.erase_counter.replace(1);
                }
                trace!(
                    ">>>>>>>>>>>>>>>>>>>> just erased line. context {:?}",
                    context
                );
            }
            ViewState::UpdatedFromGit(l) => {
                trace!(".. render MATCH UpdatedFromGit {:?}", l);
                self.line_no.replace(line_no);
                let content = self.build_up(&content, line_no, context);
                self.replace_dirty_content(buffer, iter, &content, is_markup);
                self.apply_tags(buffer, &content_tags);
                self.force_forward(buffer, iter);
            }
            ViewState::RenderedNotInPlace(l) => {
                // TODO: somehow it is related to transfered!
                trace!(".. render match not in place {:?}", l);
                self.line_no.replace(line_no);
                self.force_forward(buffer, iter);
            }
        }

        self.dirty(false);
        self.squash(false);
        self.transfer(false);
        self
    }

    fn force_forward(&self, buffer: &TextBuffer, iter: &mut TextIter) {
        let current_line = iter.line();
        let moved = iter.forward_lines(1);
        if !moved {
            // happens sometimes when buffer is over
            buffer.insert(iter, "\n");
            if iter.line() - 2 == current_line {
                iter.forward_lines(-1);
            }
            trace!(
                "buffer is over. force 1 line forward. iter now is it line {:?}",
                iter.line()
            );
        }
        assert!(current_line + 1 == iter.line());
    }

    fn start_end_iters(&self, buffer: &TextBuffer) -> (TextIter, TextIter) {
        let mut start_iter = buffer.iter_at_line(self.line_no.get()).unwrap();
        start_iter.set_line_offset(0);
        let mut end_iter = buffer.iter_at_line(self.line_no.get()).unwrap();
        end_iter.forward_to_line_end();
        (start_iter, end_iter)
    }

    fn remove_tag(&self, buffer: &TextBuffer, tag: &tags::TxtTag) {
        if self.tag_is_added(tag) {
            let (start_iter, end_iter) = self.start_end_iters(buffer);
            buffer.remove_tag_by_name(tag.name(), &start_iter, &end_iter);
            self.tag_removed(tag);
        }
    }

    fn add_tag(&self, buffer: &TextBuffer, tag: &tags::TxtTag) {
        if !self.tag_is_added(tag) {
            let (start_iter, end_iter) = self.start_end_iters(buffer);
            buffer.apply_tag_by_name(tag.name(), &start_iter, &end_iter);
            self.tag_added(tag);
        }
    }

    fn apply_tags(
        &self,
        buffer: &TextBuffer,
        content_tags: &Vec<tags::TxtTag>,
    ) {
        let mut fltr: HashSet<&str> = HashSet::new();
        if self.is_current() {
            self.add_tag(buffer, &make_tag(tags::CURSOR));
            // it need to filter background tags
            let hunk = make_tag(tags::HUNK);
            let region = make_tag(tags::REGION);
            self.remove_tag(buffer, &hunk);
            self.remove_tag(buffer, &region);
            fltr.insert(tags::HUNK);
            fltr.insert(tags::REGION);
        } else {
            self.remove_tag(buffer, &make_tag(tags::CURSOR));
        }
        if self.is_active() {
            if !fltr.contains(tags::REGION) {
                self.add_tag(buffer, &make_tag(tags::REGION));
            }
            for t in content_tags {
                if !fltr.contains(t.name()) {
                    self.add_tag(buffer, &t.enhance());
                }
            }
        } else {
            self.remove_tag(buffer, &make_tag(tags::REGION));
            for t in content_tags {
                self.remove_tag(buffer, &t.enhance());
            }
            for t in content_tags {
                if !fltr.contains(t.name()) {
                    self.add_tag(buffer, t);
                }
            }
        }
    }

    fn get_state_for(&self, line_no: i32) -> ViewState {
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
