pub mod reconciliation;
pub mod tests;

use crate::{
    commit, get_current_repo_status, push, stage_via_apply, ApplyFilter,
    ApplySubject, Diff, DiffKind, File, Head, Hunk, Line, State, View,
};

// use alloc::rc::Rc;
use async_channel::Sender;
use git2::{DiffLineType, RepositoryState};
use glib::clone;
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, AlertDialog, EventControllerKey,
    EventSequenceState, GestureClick, ListBox, MovementStep, SelectionMode,
    TextBuffer, TextIter, TextTag, TextView, TextWindowType,
    Window as Gtk4Window,
};
use std::rc::Rc;

use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, EntryRow, SwitchRow, Window};
use log::{debug, trace};
use pango::Style;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::iter::zip;

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
    fn create(&self) -> TextTag {
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

fn handle_line_offset(
    iter: &mut TextIter,
    prev_line_offset: i32,
    latest_char_offset: &RefCell<i32>,
) {
    // in case of empty line nothing below is required
    if !iter.ends_line() {
        // we are moving by lines mainaining inline (char) offset;
        // if next line has length < current offset, we want to set at that
        // max offset (eol) to not follback to prev line
        iter.forward_to_line_end();
        let eol_offset = iter.line_offset();
        if eol_offset > prev_line_offset {
            // have place to go (backward to same offset)
            iter.set_line_offset(0);
            let mut cnt = latest_char_offset.borrow_mut();
            if *cnt > prev_line_offset {
                // but if it was narrowed before.
                // go to previously stored offset
                if *cnt > eol_offset {
                    // want to flow to last known offset
                    // but line are still to narrow
                    iter.forward_to_line_end();
                } else {
                    iter.forward_chars(*cnt);
                    // and kill stored
                    *cnt = 0;
                }
            } else {
                // just go to the same offset
                iter.forward_chars(prev_line_offset);
                // let mut cnt = latest_char_offset.borrow_mut();
                if prev_line_offset > *cnt {
                    *cnt = prev_line_offset;
                }
            }
        } else {
            // save last known line offset
            let mut cnt = latest_char_offset.borrow_mut();
            if prev_line_offset > *cnt {
                *cnt = prev_line_offset;
            }
        }
    } else {
        let mut cnt = latest_char_offset.borrow_mut();
        if prev_line_offset > *cnt {
            *cnt = prev_line_offset;
        }
    }
}

pub fn text_view_factory(sndr: Sender<crate::Event>) -> TextView {
    let txt = TextView::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();
    let buffer = txt.buffer();

    buffer.tag_table().add(&Tag::Cursor.create());
    buffer.tag_table().add(&Tag::Region.create());
    buffer.tag_table().add(&Tag::Bold.create());
    buffer.tag_table().add(&Tag::Added.create());
    buffer.tag_table().add(&Tag::EnhancedAdded.create());
    buffer.tag_table().add(&Tag::Removed.create());
    buffer.tag_table().add(&Tag::EnhancedRemoved.create());
    buffer.tag_table().add(&Tag::Italic.create());

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let buffer = buffer.clone();
        let sndr = sndr.clone();
        // let txt = txt.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::Tab, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Expand(
                        iter.offset(),
                        iter.line(),
                    ))
                    .expect("Could not send through channel");
                }
                (gdk::Key::s, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Stage(
                        iter.offset(),
                        iter.line(),
                    ))
                    .expect("Could not send through channel");
                }
                (gdk::Key::u, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::UnStage(
                        iter.offset(),
                        iter.line(),
                    ))
                    .expect("Could not send through channel");
                }
                (gdk::Key::c, gdk::ModifierType::CONTROL_MASK) => {
                    // for ctrl-c
                }
                (gdk::Key::c, _) => {
                    sndr.send_blocking(crate::Event::Commit)
                        .expect("Could not send through channel");
                }
                (gdk::Key::p, _) => {
                    sndr.send_blocking(crate::Event::Push)
                        .expect("Could not send through channel");
                }
                (gdk::Key::b, _) => {
                    sndr.send_blocking(crate::Event::Branches)
                        .expect("Could not send through channel");
                }
                (gdk::Key::d, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    println!(
                        "debug ... debug ... {:?} {:?}",
                        iter.line(),
                        iter.line_offset()
                    );
                    sndr.send_blocking(crate::Event::Debug)
                        .expect("Could not send through channel");
                }
                _ => (),
            }
            glib::Propagation::Proceed
        }
    });
    txt.add_controller(event_controller);

    let gesture_controller = GestureClick::new();
    gesture_controller.connect_released({
        let sndr = sndr.clone();
        let txt = txt.clone();
        move |gesture, _some, wx, wy| {
            gesture.set_state(EventSequenceState::Claimed);
            let (x, y) = txt.window_to_buffer_coords(
                TextWindowType::Text,
                wx as i32,
                wy as i32,
            );
            if let Some(iter) = txt.iter_at_location(x, y) {
                sndr.send_blocking(crate::Event::Cursor(
                    iter.offset(),
                    iter.line(),
                ))
                .expect("Could not send through channel");
            }
        }
    });

    txt.add_controller(gesture_controller);

    txt.connect_move_cursor({
        let sndr = sndr.clone();
        let latest_char_offset = RefCell::new(0);
        move |view: &TextView, step, count, _selection| {
            let buffer = view.buffer();
            let pos = buffer.cursor_position();
            let mut start_iter = buffer.iter_at_offset(pos);
            let line_before = start_iter.line();
            // TODO! do not emit event if line is not changed!
            match step {
                MovementStep::LogicalPositions
                | MovementStep::VisualPositions => {
                    start_iter.forward_chars(count);
                }
                MovementStep::Words => {
                    start_iter.forward_word_end();
                }
                MovementStep::DisplayLines => {
                    let loffset = start_iter.line_offset();
                    start_iter.forward_lines(count);
                    handle_line_offset(
                        &mut start_iter,
                        loffset,
                        &latest_char_offset,
                    );
                }
                MovementStep::DisplayLineEnds
                | MovementStep::Paragraphs
                | MovementStep::ParagraphEnds
                | MovementStep::Pages
                | MovementStep::BufferEnds
                | MovementStep::HorizontalPages => {}
                _ => todo!(),
            }
            let current_line = start_iter.line();
            if line_before != current_line {
                sndr.send_blocking(crate::Event::Cursor(
                    start_iter.offset(),
                    current_line,
                ))
                .expect("Could not send through channel");
            } else {
                let mut cnt = latest_char_offset.borrow_mut();
                *cnt = 0;
            }
        }
    });
    txt.add_css_class("stage");
    txt.set_monospace(true);
    txt.set_editable(false);
    // let sett = txt.settings();
    // sett.set_gtk_cursor_blink(true);
    // sett.set_gtk_cursor_blink_time(3000);
    // sett.set_gtk_cursor_aspect_ratio(0.05);
    txt
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewKind {
    Diff,
    File,
    Hunk,
    Line,
    Label,
}

#[derive(Debug, Clone, Default)]
pub struct Label {
    content: String,
    view: View,
}

impl Label {
    pub fn from_string(content: &str) -> Self {
        Label {
            content: String::from(content),
            view: View::new_markup(),
        }
    }
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
            hidden: false,
        }
    }
    pub fn new_markup() -> Self {
        let mut view = Self::new();
        view.markup = true;
        view
    }

    fn is_rendered_in(&self, line_no: i32) -> bool {
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
            buffer.insert_markup(iter, &content);
        } else {
            buffer.insert(iter, content);
        }
    }

    fn build_up(
        &self,
        content: &String,
        context: &mut Option<StatusRenderContext>,
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
                    " ".repeat(spaces).to_string()
                );
            }
        }
        line_content
    }

    // View
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        content: String,
        content_tags: Vec<Tag>,
        context: &mut Option<StatusRenderContext>,
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
        let mut line_len: Option<i32> = None;
        // dbg!(&self);
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
                line_len = Some(content.len() as i32);
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
                    line_len = Some(content.len() as i32);
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
                    line_len = Some(content.len() as i32);
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

impl Default for View {
    fn default() -> Self {
        Self::new()
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

pub trait ViewContainer {
    fn get_kind(&self) -> ViewKind;

    fn child_count(&self) -> usize;

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer>;

    fn get_view(&mut self) -> &mut View;

    // TODO - return bool and stop iteration when false
    // visitor takes child as first arg and parent as second arg
    fn walk_down(&mut self, visitor: &mut dyn FnMut(&mut dyn ViewContainer)) {
        for child in self.get_children() {
            visitor(child);
            child.walk_down(visitor);
        }
    }

    fn get_content(&self) -> String;

    fn tags(&self) -> Vec<Tag> {
        Vec::new()
    }

    fn fill_context(&self, _: &mut Option<StatusRenderContext>) {}

    // ViewContainer
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut Option<StatusRenderContext>,
    ) {
        self.fill_context(context);
        let content = self.get_content();
        let tags = self.tags();
        let view =
            self.get_view().render(buffer, iter, content, tags, context);
        if view.expanded || view.child_dirty {
            for child in self.get_children() {
                child.render(buffer, iter, context);
            }
        }
        self.get_view().child_dirty = false;
    }

    // ViewContainer
    fn cursor(&mut self, line_no: i32, parent_active: bool) -> bool {
        let mut result = false;
        let view = self.get_view();

        let current_before = view.current;
        let active_before = view.active;

        let view_expanded = view.expanded;
        let current = view.is_rendered_in(line_no);
        let active_by_parent = self.is_active_by_parent(parent_active);
        let mut active_by_child = false;

        if view_expanded {
            for child in self.get_children() {
                active_by_child = child.get_view().is_rendered_in(line_no);
                if active_by_child {
                    break;
                }
            }
        }
        active_by_child = self.is_active_by_child(active_by_child);

        let self_active = active_by_parent || current || active_by_child;

        let view = self.get_view();
        view.active = self_active;
        view.current = current;

        if view.rendered {
            // repaint if highlight is changed
            view.dirty =
                view.active != active_before || view.current != current_before;
            result = view.dirty;
        }
        for child in self.get_children() {
            result = child.cursor(line_no, self_active) || result;
        }
        result
    }

    fn is_active_by_child(&self, _child_active: bool) -> bool {
        false
    }

    fn is_active_by_parent(&self, _parent_active: bool) -> bool {
        false
    }

    // ViewContainer
    fn expand(&mut self, line_no: i32) -> Option<i32> {
        let mut found_line: Option<i32> = None;
        if self.get_view().is_rendered_in(line_no) {
            let view = self.get_view();
            found_line = Some(line_no);
            view.expanded = !view.expanded;
            view.child_dirty = true;
            let expanded = view.expanded;
            self.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let view = vc.get_view();
                if expanded {
                    view.squashed = false;
                    view.rendered = false;
                } else {
                    view.squashed = true;
                }
            });
        } else if {
            let view = self.get_view();
            view.expanded && view.rendered
        } {
            // go deeper for self.children
            for child in self.get_children() {
                found_line = child.expand(line_no);
                if found_line.is_some() {
                    break;
                }
            }
            if found_line.is_some() {
                if self.is_expandable_by_child() {
                    let my_line = self.get_view().line_no;
                    return self.expand(my_line);
                }
            }
        }
        found_line
    }

    fn is_expandable_by_child(&self) -> bool {
        false
    }

    // ViewContainer
    fn erase(
        &mut self,
        txt: &TextView,
        context: &mut Option<StatusRenderContext>,
    ) {
        // CAUTION. ATTENTION. IMPORTANT
        // this ONLY rendering. the data remains
        // unchaged. means it used to be called just
        // before replacing data in status struct.
        // CAUTION. ATTENTION. IMPORTANT
        // if 1 view is rendered - it is ok.
        // next render on Status struct will shift all views.
        // But when erease multiple view in loop, all rest views
        // in loop must be shifted manually!
        // @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
        // during render the cursor moves down or become
        // unchanged in case of erasing (RenderedAndMarkedAsSquashed)
        // here we have only erase loop. cursor will be always
        // on same place, means it need to decrement view.line_no
        // by amount of deleted lines. thats why i need erase_counter.
        // but how to count, if erasing is recursive? it need to pass
        // it to render itself! means each render must receives context.
        // hm. how to avoid it? lets not avoid it. lets try to pass it,
        // and also put there prev_line length!
        let view = self.get_view();
        let mut line_no = view.line_no;
        debug!("EEEEERRRRAISING! BEFORE {:?} {:?}", line_no, context);
        if let Some(ctx) = context {
            if let Some(ec) = ctx.erase_counter {
                line_no -= ec;
            }
        }
        view.squashed = true;
        view.child_dirty = true;
        // debug!("erasing ......{:?}", &view);
        self.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let view = vc.get_view();
            view.squashed = true;
            view.child_dirty = true;
        });
        let buffer = txt.buffer();
        let mut iter = buffer
            .iter_at_line(line_no)
            .expect("can't get iter at line");
        // debug!("erase one signgle view at line > {:?}", iter.line());
        self.render(&buffer, &mut iter, context);
        // debug!("erase iter line after erase_____ > {:?}", iter.line());

        debug!("EEEEERRRRAISING! AFTER {:?} {:?}", line_no, context);
        debug!("");
        debug!("");
    }
}

impl ViewContainer for Diff {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Diff
    }

    fn child_count(&self) -> usize {
        self.files.len()
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        String::from("")
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.files
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }

    // diff
    fn cursor(&mut self, line_no: i32, parent_active: bool) -> bool {
        let mut result = false;
        for file in &mut self.files {
            result = file.cursor(line_no, parent_active) || result;
        }
        result
    }

    // Diff
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut Option<StatusRenderContext>,
    ) {
        self.view.line_no = iter.line();
        let mut prev_line_len: Option<i32> = None;
        for file in &mut self.files {
            file.render(buffer, iter, context);
        }
    }
    // Diff
    fn expand(&mut self, line_no: i32) -> Option<i32> {
        todo!("no one calls expand on diff");
        None
    }
}

impl ViewContainer for File {
    fn get_kind(&self) -> ViewKind {
        ViewKind::File
    }

    fn child_count(&self) -> usize {
        self.hunks.len()
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.title()
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.hunks
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }
    fn tags(&self) -> Vec<Tag> {
        vec![Tag::Bold]
    }
}

impl ViewContainer for Hunk {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Hunk
    }

    fn child_count(&self) -> usize {
        self.lines.len()
    }

    fn get_content(&self) -> String {
        self.title()
    }

    fn get_view(&mut self) -> &mut View {
        if self.view.line_no == 0 && !self.view.expanded {
            // hunks are expanded by default
            self.view.expanded = true
        }
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.lines
            .iter_mut()
            .filter(|l| {
                !matches!(
                    l.origin,
                    DiffLineType::FileHeader | DiffLineType::HunkHeader
                )
            })
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }

    fn is_active_by_parent(&self, active: bool) -> bool {
        // if file is active (cursor on it)
        // whole hunk is active
        active
    }

    fn is_active_by_child(&self, active: bool) -> bool {
        // if line is active (cursor on it)
        // whole hunk is active
        active
    }
    fn tags(&self) -> Vec<Tag> {
        vec![Tag::Italic]
    }

    fn is_expandable_by_child(&self) -> bool {
        true
    }

    fn fill_context(&self, context: &mut Option<StatusRenderContext>) {
        if let Some(ctx) = context {
            ctx.max_hunk_len.replace(self.max_line_len as i32);
        }
    }
}

impl ViewContainer for Line {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Line
    }
    fn child_count(&self) -> usize {
        0
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.content.to_string()
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    // line
    fn expand(&mut self, line_no: i32) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no == line_no {
            return Some(line_no);
        }
        None
    }

    fn is_active_by_parent(&self, active: bool) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    fn tags(&self) -> Vec<Tag> {
        match self.origin {
            DiffLineType::Addition => {
                vec![Tag::Added]
            }
            DiffLineType::Deletion => {
                vec![Tag::Removed]
            }
            _ => Vec::new(),
        }
    }
}

impl ViewContainer for Label {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }
    fn child_count(&self) -> usize {
        0
    }
    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn get_content(&self) -> String {
        self.content.to_string()
    }
}

impl ViewContainer for Head {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }
    fn child_count(&self) -> usize {
        0
    }
    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn get_content(&self) -> String {
        format!(
            "{}<span color=\"#4a708b\">{}</span> {}",
            if !self.remote {
                "Head:     "
            } else {
                "Upstream: "
            },
            &self.branch,
            self.commit
        )
    }
}

impl ViewContainer for State {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }
    fn child_count(&self) -> usize {
        0
    }
    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn get_content(&self) -> String {
        let state = match self.state {
            RepositoryState::Clean => "Clean",
            RepositoryState::Merge => "<span color=\"#ff0000\">Merge</span>",
            RepositoryState::Revert => "<span color=\"#ff0000\">Revert</span>",
            RepositoryState::RevertSequence => {
                "<span color=\"#ff0000\">RevertSequence</span>"
            }
            RepositoryState::CherryPick => {
                "<span color=\"#ff0000\">CherryPick</span>"
            }
            RepositoryState::CherryPickSequence => {
                "<span color=\"#ff0000\">CherryPickSequence</span>"
            }
            RepositoryState::Bisect => "<span color=\"#ff0000\">Bisect</span>",
            RepositoryState::Rebase => "<span color=\"#ff0000\">Rebase</span>",
            RepositoryState::RebaseInteractive => {
                "<span color=\"#ff0000\">RebaseInteractive</span>"
            }
            RepositoryState::RebaseMerge => {
                "<span color=\"#ff0000\">RebaseMerge</span>"
            }
            RepositoryState::ApplyMailbox => {
                "<span color=\"#ff0000\">ApplyMailbox</span>"
            }
            RepositoryState::ApplyMailboxOrRebase => {
                "<span color=\"#ff0000\">ApplyMailboxOrRebase</span>"
            }
        };
        format!("State:    {}", state)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RenderSource {
    Git,
    Cursor(i32),
    Expand(i32),
    Erase,
}

#[derive(Debug, Clone)]
pub struct StatusRenderContext {
    pub erase_counter: Option<i32>,
    pub diff_kind: Option<DiffKind>,
    pub max_hunk_len: Option<i32>,
}

impl StatusRenderContext {
    pub fn new() -> Self {
        return {
            Self {
                erase_counter: None,
                diff_kind: None,
                max_hunk_len: None,
            }
        };
    }
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub head: Option<Head>,
    pub upstream: Option<Head>,
    pub state: Option<State>,
    pub staged_spacer: Label,
    pub staged_label: Label,
    pub staged: Option<Diff>,
    pub unstaged_spacer: Label,
    pub unstaged_label: Label,
    pub unstaged: Option<Diff>,
    pub rendered: bool, // what it is for ????
    pub context: Option<StatusRenderContext>,
}

impl Status {
    pub fn new() -> Self {
        Self {
            head: None,
            upstream: None,
            state: None,
            staged_spacer: Label::from_string(""),
            staged_label: Label::from_string(
                "<span weight=\"bold\" color=\"#8b6508\">Staged changes</span>",
            ),
            staged: None,
            unstaged_spacer: Label::from_string(""),
            unstaged_label: Label::from_string(
                "<span weight=\"bold\" color=\"#8b6508\">Unstaged changes</span>",
            ),
            unstaged: None,
            rendered: false,
            context: None::<StatusRenderContext>
        }
    }

    pub fn get_status(
        &self,
        path: Option<OsString>,
        sender: Sender<crate::Event>,
    ) {
        gio::spawn_blocking({
            move || {
                get_current_repo_status(path, sender);
            }
        });
    }

    pub fn push(
        &mut self,
        path: &OsString,
        window: &impl IsA<Gtk4Window>,
        sender: Sender<crate::Event>,
    ) {
        let lb = ListBox::builder()
            .selection_mode(SelectionMode::None)
            .css_classes(vec![String::from("boxed-list")])
            .build();
        let upstream = SwitchRow::builder()
            .title("Set upstream")
            .css_classes(vec!["input_field"])
            .active(true)
            .build();

        let input = EntryRow::builder()
            .title("Remote branch name:")
            .css_classes(vec!["input_field"])
            .text(self.choose_remote())
            .build();
        lb.append(&input);
        lb.append(&upstream);

        crate::make_confirm_dialog(
            window,
            Some(&lb),
            "Push to remote/origin", // TODO here is harcode
            "Push",
        )
        .choose(None::<&gio::Cancellable>, {
            let path = path.clone();
            let sender = sender.clone();
            move |result| {
                if result == "confirm" {
                    trace!(
                        "confirm push dialog {:?} {:?}",
                        input.text(),
                        upstream.is_active()
                    );
                    let remote_branch_name = format!("{}", input.text());
                    let track_remote = upstream.is_active();
                    gio::spawn_blocking({
                        let path = path.clone();
                        move || {
                            push(
                                path,
                                remote_branch_name,
                                track_remote,
                                sender,
                            );
                        }
                    });
                }
            }
        });
    }

    pub fn choose_remote(&self) -> String {
        if let Some(upstream) = &self.upstream {
            debug!(
                "-------------------> upstream branch {:?}",
                upstream.branch.clone()
            );
            return upstream.branch.clone();
        }
        if let Some(head) = &self.head {
            debug!("-------------------> head branch");
            return head.branch.clone();
        }
        debug!("-------------------> Default");
        String::from("origin/master")
    }

    pub fn commit(
        &mut self,
        path: &OsString,
        txt: &TextView,
        window: &impl IsA<Gtk4Window>,
        sender: Sender<crate::Event>,
    ) {
        if let Some(_) = &mut self.staged {
            let lb = ListBox::builder()
                .selection_mode(SelectionMode::None)
                .css_classes(vec![String::from("boxed-list")])
                .build();
            let input = EntryRow::builder()
                .title("Commit message:")
                .css_classes(vec!["input_field"])
                .build();
            lb.append(&input);
            // let me = Rc::new(RefCell::new(self));
            crate::make_confirm_dialog(window, Some(&lb), "Commit", "Commit")
                .choose(None::<&gio::Cancellable>, {
                    let path = path.clone();
                    let sender = sender.clone();
                    // let me = Rc::clone(&me);
                    move |result| {
                        if result == "confirm" {
                            trace!("confirm commit dialog {:?}", input.text());
                            let message = format!("{}", input.text());
                            gio::spawn_blocking({
                                let path = path.clone();
                                move || {
                                    commit(path, message, sender);
                                }
                            });
                        }
                    }
                });
        }
    }

    pub fn update_head(&mut self, mut head: Head, txt: &TextView) {
        // refactor.enrich
        if let Some(current_head) = &self.head {
            head.enrich_view(&current_head);
        }
        self.head.replace(head);
        self.render(txt, RenderSource::Git);
    }

    pub fn update_upstream(
        &mut self,
        mut upstream: Option<Head>,
        txt: &TextView,
    ) {
        // refactor.enrich
        match (&self.upstream, upstream.as_mut()) {
            (Some(current), Some(new)) => new.enrich_view(&current),
            _ => {}
        }
        self.upstream = upstream;
        self.render(txt, RenderSource::Git);
    }

    pub fn update_state(&mut self, mut state: State, txt: &TextView) {
        if let Some(current_state) = &self.state {
            state.enrich_view(current_state)
        }
        self.state.replace(state);
        self.render(txt, RenderSource::Git);
    }

    pub fn update_staged(&mut self, mut diff: Diff, txt: &TextView) {
        if let Some(s) = &mut self.staged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(s, txt, &mut self.context);
        }
        self.staged.replace(diff);
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git);
        }
    }

    pub fn update_unstaged(&mut self, mut diff: Diff, txt: &TextView) {
        if let Some(u) = &mut self.unstaged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(u, txt, &mut self.context);
        }
        self.unstaged.replace(diff);
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git);
        }
    }
    // status
    pub fn cursor(&mut self, txt: &TextView, line_no: i32, offset: i32) {
        let mut changed = false;
        if let Some(unstaged) = &mut self.unstaged {
            changed = changed || unstaged.cursor(line_no, false);
        }
        if let Some(staged) = &mut self.staged {
            changed = changed || staged.cursor(line_no, false);
        }
        if changed {
            self.render(txt, RenderSource::Cursor(line_no));
            let buffer = txt.buffer();
            trace!("put cursor on line {:?} in CURSOR", line_no);
            buffer.place_cursor(&buffer.iter_at_offset(offset));
        }
    }

    // Status
    pub fn expand(&mut self, txt: &TextView, line_no: i32, offset: i32) {
        // let mut changed = false;
        if let Some(unstaged) = &mut self.unstaged {
            for file in &mut unstaged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(txt, RenderSource::Expand(expanded_line));
                    return;
                }
            }
        }
        if let Some(staged) = &mut self.staged {
            for file in &mut staged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(txt, RenderSource::Expand(expanded_line));
                    return;
                }
            }
        }
    }

    // Status
    pub fn render(&mut self, txt: &TextView, source: RenderSource) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);

        if let Some(head) = &mut self.head {
            head.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(upstream) = &mut self.upstream {
            upstream.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(state) = &mut self.state {
            state.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(unstaged) = &mut self.unstaged {
            if unstaged.files.is_empty() {
                self.unstaged_spacer.view.squashed = true;
                self.unstaged_label.view.squashed = true;
            }
            self.unstaged_spacer
                .render(&buffer, &mut iter, &mut self.context);
            self.unstaged_label
                .render(&buffer, &mut iter, &mut self.context);
            unstaged.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(staged) = &mut self.staged {
            if staged.files.is_empty() {
                self.staged_spacer.view.squashed = true;
                self.staged_label.view.squashed = true;
            }
            self.staged_spacer
                .render(&buffer, &mut iter, &mut self.context);
            self.staged_label
                .render(&buffer, &mut iter, &mut self.context);
            staged.render(&buffer, &mut iter, &mut self.context);
        }
        trace!("render source {:?}", source);
        match source {
            RenderSource::Cursor(_) => {
                // avoid loops on cursor renders
                trace!("avoid cursor position on cursor");
            }
            RenderSource::Expand(line_no) => {
                // avoid loops on cursor renders
                self.choose_cursor_position(txt, &buffer, Some(line_no));
            }
            RenderSource::Git => {
                // avoid loops on cursor renders
                self.choose_cursor_position(txt, &buffer, None);
            }
            src => {}
        };
    }

    pub fn stage(
        &mut self,
        txt: &TextView,
        line_no: i32,
        path: &OsString,
        subject: ApplySubject,
        sender: Sender<crate::Event>,
    ) {
        // hm. this is very weird code
        match subject {
            ApplySubject::Stage => {
                if self.unstaged.is_none() {
                    return;
                }
            }
            ApplySubject::Unstage => {
                if self.staged.is_none() {
                    return;
                }
            }
        }

        let diff = {
            match subject {
                ApplySubject::Stage => self.unstaged.as_mut().unwrap(),
                ApplySubject::Unstage => self.staged.as_mut().unwrap(),
            }
        };
        let mut filter = ApplyFilter::new(subject);
        let mut file_path_so_stage = String::new();
        let mut hunks_staged = 0;
        // there could be either file with all hunks
        // or just 1 hunk
        diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let content = vc.get_content();
            let kind = vc.get_kind();
            let view = vc.get_view();
            match kind {
                ViewKind::File => {
                    // just store current file_path
                    // in this loop. temporary variable
                    file_path_so_stage = content;
                }
                ViewKind::Hunk => {
                    if !view.active {
                        return;
                    }
                    // store active hunk in filter
                    // if the cursor is on file, all
                    // hunks under it will be active
                    filter.file_path = file_path_so_stage.clone();
                    filter.hunk_header.replace(content);
                    hunks_staged += 1;
                }
                _ => (),
            }
        });
        debug!("stage. apply filter {:?}", filter);
        if !filter.file_path.is_empty() {
            if hunks_staged > 1 {
                // stage all hunks in file
                filter.hunk_header = None;
            }
            debug!("stage via apply {:?}", filter);
            gio::spawn_blocking({
                let path = path.clone();
                move || {
                    stage_via_apply(path, filter, sender);
                }
            });
        }
    }

    pub fn choose_cursor_position(
        &mut self,
        txt: &TextView,
        buffer: &TextBuffer,
        line_no: Option<i32>,
    ) {
        trace!("choose_cursor_position. optional line {:?}", line_no);
        let offset = buffer.cursor_position();
        if offset == buffer.end_iter().offset() {
            // first render. buffer at eof
            if let Some(unstaged) = &self.unstaged {
                if !unstaged.files.is_empty() {
                    let line_no = unstaged.files[0].view.line_no;
                    let iter = buffer.iter_at_line(line_no).unwrap();
                    trace!(
                        "choose cursor at first unstaged file {:?}",
                        line_no
                    );
                    self.cursor(txt, line_no, iter.offset());
                    return;
                }
            }
        }
        let iter = buffer.iter_at_offset(offset);
        // after git op view could be shifted.
        // cursor is on place and it is visually current,
        // but view under it is not current, cause line_no differs
        trace!("choose cursor when NOT on eof {:?}", iter.line());
        self.cursor(txt, iter.line(), offset);
    }

    pub fn has_staged(&self) -> bool {
        if let Some(staged) = &self.staged {
            return !staged.files.is_empty();
        }
        false
    }
    pub fn debug(&mut self, txt: &TextView) {
        let buffer = txt.buffer();
        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let current_line = iter.line();
        println!("debug at line {:?}", current_line);
        if let Some(diff) = &mut self.staged {
            for f in &diff.files {
                dbg!(&f);
            }
            diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let content = vc.get_content();
                let view = vc.get_view();
                if view.line_no == current_line {
                    println!("found view {:?}", content);
                    dbg!(view);
                }
            });
        }
        if let Some(diff) = &mut self.unstaged {
            for f in &diff.files {
                dbg!(&f);
            }
            diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let content = vc.get_content();
                let view = vc.get_view();
                if view.line_no == current_line {
                    println!("found view {:?}", content);
                    dbg!(view);
                }
            });
        }
    }
}
