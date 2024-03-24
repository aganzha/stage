use gtk4::prelude::*;
use gtk4::{pango, TextBuffer, TextIter, TextTag};
use log::{trace};
use pango::Style;
use std::collections::HashSet;

const CURSOR_TAG: &str = "CursorTag";

#[derive(Eq, Hash, PartialEq)]
pub enum Tag {
    Bold,
    Added,
    EnhancedAdded,
    Removed,
    EnhancedRemoved,
    Cursor,
    Region,
    Italic,
    // Link
}
impl Tag {
    pub fn create(&self) -> TextTag {
        match self {
            Self::Bold => {
                let tt = self.new_tag();
                tt.set_weight(700);
                tt
            }
            Self::Added => {
                let tt = self.new_tag();
                tt.set_background(Some("#ebfcf1"));
                tt
            }
            Self::EnhancedAdded => {
                let tt = self.new_tag();
                tt.set_background(Some("#d3fae1"));
                tt
            }
            Self::Removed => {
                let tt = self.new_tag();
                tt.set_background(Some("#fbf0f3"));
                tt
            }
            Self::EnhancedRemoved => {
                let tt = self.new_tag();
                tt.set_background(Some("#f4c3d0"));
                tt
            }
            Self::Cursor => {
                let tt = self.new_tag();
                tt.set_background(Some("#f6fecd"));
                tt
            }
            Self::Region => {
                let tt = self.new_tag();
                tt.set_background(Some("#f2f2f2"));
                tt
            }
            Self::Italic => {
                let tt = self.new_tag();
                tt.set_style(Style::Italic);
                tt
            } // Self::Link => {
              //     let tt = self.new_tag();
              //     tt.set_background(Some("0000ff"));
              //     tt.set_style(Style::Underlined);
              //     tt
              // }
        }
    }
    fn new_tag(&self) -> TextTag {
        TextTag::new(Some(self.name()))
    }
    fn name(&self) -> &str {
        match self {
            Self::Bold => "bold",
            Self::Added => "added",
            Self::EnhancedAdded => "enhancedAdded",
            Self::Removed => "removed",
            Self::EnhancedRemoved => "enhancedRemoved",
            Self::Cursor => CURSOR_TAG,
            Self::Region => "region",
            Self::Italic => "italic",
        }
    }
    fn enhance(&self) -> &Self {
        match self {
            Self::Added => &Self::EnhancedAdded,
            Self::Removed => &Self::EnhancedRemoved,
            other => other,
        }
    }
}

pub enum ViewState {
    Hidden,
    RenderedInPlace,
    Deleted,
    NotRendered,
    RenderedDirtyInPlace,
    RenderedAndMarkedAsSquashed,
    RenderedDirtyNotInPlace(i32),
    RenderedNotInPlace(i32),
}

impl crate::View {
    pub fn new() -> Self {
        crate::View {
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
            hidden: false,
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
            // let mut encoded = String::new();
            // html_escape::encode_safe_to_string(&content, &mut encoded);
            buffer.insert_markup(iter, content);
        } else {
            buffer.insert(iter, content);
        }
    }

    fn build_up(
        &self,
        content: &String,
        context: &mut Option<crate::StatusRenderContext>,
    ) -> String {
        let line_content = content.to_string();
        if let Some(ctx) = context {
            if let Some(max) = ctx.max_hunk_len {
                trace!(
                    "build_up .............. {:?} {:?} ======= {:?}",
                    max,
                    line_content.len(),
                    line_content
                );
                let spaces = max as usize - line_content.len();
                return format!(
                    "{}{}",
                    line_content,
                    " ".repeat(spaces)
                );
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
        content_tags: Vec<Tag>,
        context: &mut Option<crate::StatusRenderContext>,
    ) -> &mut Self {
        // important. self.line_no is assigned only in 2 cases
        // below!!!!
        let line_no = iter.line();
        trace!(
            "======= line {:?} render view {:?} which is at line {:?}",
            line_no,
            content,
            self.line_no
        );
        match self.get_state_for(line_no) {
            ViewState::Hidden => {
                trace!("skip hidden view");
                return self;
            }
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
                let content = self.build_up(&content, context);
                if self.markup {
                    // let mut encoded = String::new();
                    // html_escape::encode_safe_to_string(&content, &mut encoded);
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
                if !content.is_empty() {
                    let content = self.build_up(&content, context);
                    self.replace_dirty_content(buffer, iter, &content);
                    self.apply_tags(buffer, &content_tags);
                } else {
                    self.apply_tags(buffer, &content_tags);
                }
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
                if let Some(ctx) = context {
                    let mut inc = 1;
                    if let Some(ec) = ctx.erase_counter {
                        inc += ec;
                    }
                    ctx.erase_counter.replace(inc);
                    trace!(
                        ">>>>>>>>>>>>>>>>>>>> just erased line. context {:?}",
                        ctx
                    );
                }
            }
            ViewState::RenderedDirtyNotInPlace(l) => {
                trace!(".. render MATCH RenderedDirtyNotInPlace {:?}", l);
                self.line_no = line_no;
                if !content.is_empty() {
                    let content = self.build_up(&content, context);
                    self.replace_dirty_content(buffer, iter, &content);
                    self.apply_tags(buffer, &content_tags);
                } else if self.tags.contains(&String::from(CURSOR_TAG)) {
                    // special case for cleanup cursor highlight
                    self.apply_tags(buffer, &content_tags);
                }
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

    fn remove_tag(&mut self, buffer: &TextBuffer, tag: &str) {
        let index = self.tags.iter().position(|t| t == tag);
        if let Some(ind) = index {
            let (start_iter, end_iter) = self.start_end_iters(buffer);
            buffer.remove_tag_by_name(tag, &start_iter, &end_iter);
            self.tags.remove(ind);
        }
    }

    fn add_tag(&mut self, buffer: &TextBuffer, tag: &str) {
        let index = self.tags.iter().position(|t| t == tag);
        if index.is_none() {
            let (start_iter, end_iter) = self.start_end_iters(buffer);
            buffer.apply_tag_by_name(tag, &start_iter, &end_iter);
            self.tags.push(String::from(tag));
        }
    }

    fn apply_tags(&mut self, buffer: &TextBuffer, content_tags: &Vec<Tag>) {
        let mut fltr: HashSet<Tag> = HashSet::new();
        if self.current {
            self.add_tag(buffer, Tag::Cursor.name());
            fltr.insert(Tag::Added);
            fltr.insert(Tag::Removed);
            fltr.insert(Tag::Region);
            // it need to filter background tags
        } else {
            self.remove_tag(buffer, Tag::Cursor.name());
        }
        if self.active {
            if !fltr.contains(&Tag::Region) {
                self.add_tag(buffer, Tag::Region.name());
            }
            for t in content_tags {
                if !fltr.contains(t) {
                    self.add_tag(buffer, t.enhance().name());
                }
            }
        } else {
            self.remove_tag(buffer, Tag::Region.name());
            for t in content_tags {
                self.remove_tag(buffer, t.enhance().name());
            }
            for t in content_tags {
                if !fltr.contains(t) {
                    self.add_tag(buffer, t.name());
                }
            }
        }
    }

    fn get_state_for(&self, line_no: i32) -> ViewState {
        if self.hidden {
            return ViewState::Hidden;
        }
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
            return ViewState::RenderedDirtyNotInPlace(self.line_no);
        }
        if self.squashed {
            return ViewState::RenderedAndMarkedAsSquashed;
        }
        ViewState::RenderedNotInPlace(self.line_no)
    }
}

impl Default for crate::View {
    fn default() -> Self {
        Self::new()
    }
}
