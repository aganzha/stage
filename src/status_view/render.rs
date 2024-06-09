//use crate::status_view::Tag;
use crate::status_view::tags;
use gtk4::prelude::*;
use gtk4::{TextBuffer, TextIter, TextTag, pango};
use log::{debug, trace};
use std::cell::Cell;
use std::collections::HashSet;
use std::fmt;
use pango::Style;

#[derive(Debug, Clone)]
pub enum ViewState {
    RenderedInPlace,
    Deleted,
    NotRendered,
    RenderedDirtyInPlace,
    RenderedAndMarkedAsSquashed,
    RenderedDirtyNotInPlace(i32),
    RenderedNotInPlace(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub line_no: i32,
    pub expanded: bool,
    pub squashed: bool,
    pub rendered: bool,
    pub dirty: bool,
    pub child_dirty: bool,
    pub active: bool,
    pub current: bool,
    pub transfered: bool,
    pub tags: Vec<String>,
    pub markup: bool,
    pub tag_indexes: tags::TagIdx,
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
            line_no: 0,
            expanded: false,
            squashed: false,
            rendered: false,
            dirty: false,
            child_dirty: false,
            active: false,
            current: false,
            transfered: false,
            tags: Vec::new(),
            markup: false,
            tag_indexes: tags::TagIdx::new(),
        }
    }
    pub fn new_markup() -> Self {
        let mut view = Self::new();
        view.markup = true;
        view
    }

    pub fn is_rendered_in(&self, line_no: i32) -> bool {
        self.rendered
            && self.line_no == line_no
            && !self.dirty
            && !self.squashed
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
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        content: &str,
    ) {
        let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
        eol_iter.forward_to_line_end();
        buffer.remove_all_tags(iter, &eol_iter);
        self.tags = Vec::new();
        buffer.delete(iter, &mut eol_iter);
        if self.markup {
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
    pub fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        content: String,
        content_tags: Vec<tags::TxtTag>,
        context: &mut crate::StatusRenderContext,
    ) -> &mut Self {
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
            ViewState::NotRendered => {
                trace!("..render MATCH insert {:?}", line_no);
                let content = self.build_up(&content, line_no, context);
                if self.markup {
                    buffer.insert_markup(iter, &format!("{}\n", content));
                } else {
                    buffer.insert(iter, &format!("{}\n", content));
                }
                self.line_no = line_no;
                self.rendered = true;
                if !content.is_empty() {
                    self.apply_tags(buffer, &content_tags);
                }
            }
            ViewState::RenderedDirtyInPlace => {
                trace!("..render MATCH RenderedDirtyInPlace {:?}", line_no);
                // this means only tags are changed.
                if self.does_not_match_width(buffer, context) {
                    // here is the case: view is rendered before resize event.
                    // max width is detected by diff max width and then resize
                    // event is come with larger with
                    let content = self.build_up(&content, line_no, context);
                    self.replace_dirty_content(buffer, iter, &content);
                }
                self.apply_tags(buffer, &content_tags);
                if !iter.forward_lines(1) {
                    assert!(iter.offset() == buffer.end_iter().offset());
                }
                self.rendered = true;
            }
            ViewState::RenderedAndMarkedAsSquashed => {
                trace!("..render MATCH squashed {:?}", line_no);
                let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
                nel_iter.forward_lines(1);
                buffer.delete(iter, &mut nel_iter);
                self.rendered = false;
                self.tags = Vec::new();

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
            ViewState::RenderedDirtyNotInPlace(l) => {
                trace!(".. render MATCH RenderedDirtyNotInPlace {:?}", l);
                self.line_no = line_no;
                let content = self.build_up(&content, line_no, context);
                self.replace_dirty_content(buffer, iter, &content);
                self.apply_tags(buffer, &content_tags);
                self.force_forward(buffer, iter);
            }
            ViewState::RenderedNotInPlace(l) => {
                // TODO: somehow it is related to transfered!
                trace!(".. render match not in place {:?}", l);
                self.line_no = line_no;
                self.force_forward(buffer, iter);
            }
        }

        self.dirty = false;
        self.squashed = false;
        self.transfered = false;
        self
    }

    fn force_forward(&self, buffer: &TextBuffer, iter: &mut TextIter) {
        let current_line = iter.line();
        trace!("force forward at line {:?}", current_line);
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
        let mut start_iter = buffer.iter_at_line(self.line_no).unwrap();
        start_iter.set_line_offset(0);
        let mut end_iter = buffer.iter_at_line(self.line_no).unwrap();
        end_iter.forward_to_line_end();
        (start_iter, end_iter)
    }

    fn remove_tag(&mut self, buffer: &TextBuffer, tag: &tags::TxtTag) {
        if self.tag_is_added(tag) {
            let (start_iter, end_iter) = self.start_end_iters(buffer);
            buffer.remove_tag_by_name(tag.name(), &start_iter, &end_iter);
            self.tag_removed(tag);
        }
    }

    fn add_tag(&mut self, buffer: &TextBuffer, tag: &tags::TxtTag) {        
        if !self.tag_is_added(tag) {
            let (start_iter, end_iter) = self.start_end_iters(buffer);
            if tag.name() == tags::CURSOR {
                trace!("JJJJJJJJJJJJJJJJJJJUST adding cursor {:?}", start_iter.line());
            }
            buffer.apply_tag_by_name(tag.name(), &start_iter, &end_iter);
            self.tag_added(tag);
        }
    }

    fn apply_tags(&mut self, buffer: &TextBuffer, content_tags: &Vec<tags::TxtTag>) {
        trace!("apply_tags {} {:?}", &self.line_no, &content_tags);
        let mut fltr: HashSet<&str> = HashSet::new();
        if self.current {
            trace!("ADDED CURSOR at line {}", self.line_no);
            self.add_tag(buffer, &make_tag(tags::CURSOR));
            // it need to filter background tags
            let hunk = make_tag(tags::HUNK);
            let region = make_tag(tags::REGION);
            self.remove_tag(buffer, &hunk);
            self.remove_tag(buffer, &region);
            fltr.insert(tags::HUNK);
            fltr.insert(tags::REGION);
        } else {
            trace!("rrrrrrrrremoving CURSOR at line {:?}", self.line_no);
            self.remove_tag(buffer, &make_tag(tags::CURSOR));
        }
        if self.active {
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
        if !self.rendered && self.squashed {
            return ViewState::Deleted;
        }
        if !self.rendered {
            return ViewState::NotRendered;
        }
        if self.dirty && !self.transfered {
            return ViewState::RenderedDirtyInPlace;
        }
        if self.dirty && self.transfered {
            // why not in place? it is in place, just transfered!
            // TODO rename this state. and think about it!
            return ViewState::RenderedDirtyNotInPlace(self.line_no);
        }
        if self.squashed {
            return ViewState::RenderedAndMarkedAsSquashed;
        }
        ViewState::RenderedNotInPlace(self.line_no)
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}
