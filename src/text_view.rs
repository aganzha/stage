use crate::common_tests::*;
use crate::{
    commit_staged, get_current_repo_status, stage_via_apply, ApplyFilter, Diff, File, Hunk, Line,
    View,
};
use git2::DiffLineType;
use glib::Sender;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango, TextBuffer, TextIter, TextTag, TextView};
use log::{debug, error, info, log_enabled, trace};
use pango::Style;
use std::cell::RefCell;
use std::ffi::OsString;
use backtrace::Backtrace;

const CURSOR_HIGHLIGHT: &str = "CursorHighlight";
const REGION_HIGHLIGHT: &str = "RegionHighlight";

pub enum Tag {
    Bold,
    Added,
    Removed,
    Cursor,
    Region,
    Italic,
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
            Self::Removed => {
                let tt = self.new_tag();
                tt.set_background(Some("#fbf0f3"));
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
            }
        }
    }
    fn new_tag(&self) -> TextTag {
        TextTag::new(Some(self.name()))
    }
    fn name(&self) -> &str {
        match self {
            Self::Bold => "bold",
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Cursor => CURSOR_HIGHLIGHT,
            Self::Region => REGION_HIGHLIGHT,
            Self::Italic => "italic",
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
    let txt = TextView::builder().build();
    let buffer = txt.buffer();

    // let tag = TextTag::new(Some(CURSOR_HIGHLIGHT));
    // tag.set_background(Some(CURSOR_COLOR));
    // buffer.tag_table().add(&tag);

    // let tag = TextTag::new(Some(REGION_HIGHLIGHT));
    // tag.set_background(Some(REGION_COLOR));
    // buffer.tag_table().add(&tag);

    buffer.tag_table().add(&Tag::Cursor.create());
    buffer.tag_table().add(&Tag::Region.create());
    buffer.tag_table().add(&Tag::Bold.create());
    buffer.tag_table().add(&Tag::Added.create());
    buffer.tag_table().add(&Tag::Removed.create());
    buffer.tag_table().add(&Tag::Italic.create());

    let event_controller = gtk::EventControllerKey::new();
    event_controller.connect_key_pressed({
        let buffer = buffer.clone();
        let sndr = sndr.clone();
        // let txt = txt.clone();
        move |_, key, _, _| {
            match key {
                gdk::Key::Tab => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send(crate::Event::Expand(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                }
                gdk::Key::s => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send(crate::Event::Stage(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                }
                gdk::Key::u => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send(crate::Event::UnStage(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                }
                gdk::Key::c => {
                    sndr.send(crate::Event::CommitRequest)
                        .expect("Could not send through channel");
                    // txt.activate_action("win.commit", None)
                    //     .expect("action does not exists");
                }
                gdk::Key::d => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    println!(
                        "debug ... debug ... {:?} {:?}",
                        iter.line(),
                        iter.line_offset()
                    );
                    sndr.send(crate::Event::Debug)
                        .expect("Could not send through channel");
                }
                _ => (),
            }
            glib::Propagation::Proceed
        }
    });
    txt.add_controller(event_controller);

    let gesture_controller = gtk::GestureClick::new();
    gesture_controller.connect_released({
        let sndr = sndr.clone();
        let txt = txt.clone();
        move |gesture, _some, wx, wy| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let (x, y) =
                txt.window_to_buffer_coords(gtk::TextWindowType::Text, wx as i32, wy as i32);
            if let Some(iter) = txt.iter_at_location(x, y) {
                sndr.send(crate::Event::Cursor(iter.offset(), iter.line()))
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
                gtk::MovementStep::LogicalPositions | gtk::MovementStep::VisualPositions => {
                    start_iter.forward_chars(count);
                }
                gtk::MovementStep::Words => {
                    start_iter.forward_word_end();
                }
                gtk::MovementStep::DisplayLines => {
                    let loffset = start_iter.line_offset();
                    start_iter.forward_lines(count);
                    handle_line_offset(&mut start_iter, loffset, &latest_char_offset);
                }
                gtk::MovementStep::DisplayLineEnds
                | gtk::MovementStep::Paragraphs
                | gtk::MovementStep::ParagraphEnds
                | gtk::MovementStep::Pages
                | gtk::MovementStep::BufferEnds
                | gtk::MovementStep::HorizontalPages => {}
                _ => todo!(),
            }
            let current_line = start_iter.line();
            if line_before != current_line {
                sndr.send(crate::Event::Cursor(start_iter.offset(), current_line))
                    .expect("Could not send through channel");
            } else {
                let mut cnt = latest_char_offset.borrow_mut();
                *cnt = 0;
            }
        }
    });

    txt.set_monospace(true);
    txt.set_editable(false);

    buffer.place_cursor(&buffer.iter_at_offset(0));
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
            view: View::new(),
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
        }
    }

    fn is_rendered_in(&self, line_no: i32) -> bool {
        self.rendered && self.line_no == line_no && !self.dirty && !self.squashed
    }

    fn replace_dirty_content(&mut self, buffer: &TextBuffer, iter: &mut TextIter, content: &str) {
        let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
        eol_iter.forward_to_line_end();
        buffer.remove_all_tags(iter, &mut eol_iter);
        self.tags = Vec::new();
        buffer.delete(iter, &mut eol_iter);
        buffer.insert(iter, content);
    }

    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        content: String,
        content_tags: Vec<Tag>,
    ) -> &mut Self {
        // important. self.line_no is assigned only in 2 cases
        // below!!!!
        let line_no = iter.line();
        trace!(
            "line {:?} render view {:?} which is at line {:?}",
            line_no,
            content,
            self.line_no
        );
        match self.get_state_for(line_no) {
            ViewState::RenderedInLine(l) => {
                debug!("..render MATCH rendered_in_line {:?}", l);
                iter.forward_lines(1);
            }
            ViewState::Deleted => {
                // nothing todo. calling render on
                // some whuch will be destroyed
                debug!("..render MATCH !rendered squashed {:?}", line_no);
            }
            ViewState::NotRendered => {
                debug!("..render MATCH insert {:?}", line_no);
                buffer.insert(iter, &format!("{}\n", content));
                self.line_no = line_no;
                self.rendered = true;
                self.apply_tags(buffer, &content, &content_tags);
            }
            ViewState::RenderedAndMarkedAsDirty => {
                debug!("..render MATCH dirty !transfered {:?}", line_no);
                if !content.is_empty() {
                    self.replace_dirty_content(buffer, iter, &content);
                }
                if !iter.forward_lines(1) {
                    assert!(iter.offset() == buffer.end_iter().offset());
                }
                self.apply_tags(buffer, &content, &content_tags);
                self.rendered = true;
            }
            ViewState::RenderedAndMarkedAsSquashed => {
                debug!("..render MATCH squashed {:?}", line_no);
                let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
                nel_iter.forward_lines(1);
                buffer.delete(iter, &mut nel_iter);
                self.rendered = false;
                self.tags = Vec::new();
            },
            ViewState::RenderedNotInLine(_) => {
                // TODO: somehow it is related to transfered!
                if self.dirty && !content.is_empty() {
                    self.replace_dirty_content(buffer, iter, &content);
                    self.apply_tags(buffer, &content, &content_tags);
                }
                // does not work. until line numbers are there thats for sure
                // let inbuffer = buffer.slice(&iter, &eol_iter, true);
                // if !inbuffer.contains(&content) {
                //     panic!("WHILE MOVE {} != {}", inbuffer, content);
                // }
                self.line_no = line_no;
                let moved = iter.forward_lines(1);
                if !moved {
                    // happens sometimes when buffer is over
                    buffer.insert(iter, "\n");
                    // println!("insert on pass as buffer is over");
                } else {
                    // println!("just pass");
                }
            }
        }

        self.dirty = false;
        self.squashed = false;
        self.transfered = false;
        self
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
        if let Some(_) = index {
        } else {
            let (start_iter, end_iter) = self.start_end_iters(buffer);
            buffer.apply_tag_by_name(tag, &start_iter, &end_iter);
            self.tags.push(String::from(tag));
        }
    }

    fn apply_tags(&mut self, buffer: &TextBuffer, content: &str, content_tags: &Vec<Tag>) {
        if content.is_empty() {
            return;
        }
        if self.current {
            self.add_tag(buffer, CURSOR_HIGHLIGHT);
        } else {
            self.remove_tag(buffer, CURSOR_HIGHLIGHT);
            if self.active {
                self.add_tag(buffer, REGION_HIGHLIGHT);
            } else {
                self.remove_tag(buffer, REGION_HIGHLIGHT);
            }
        }
        for t in content_tags {
            self.add_tag(buffer, t.name());
        }
    }
    fn get_state_for(&self, line_no: i32) -> ViewState {
        if self.is_rendered_in(line_no) {
            return ViewState::RenderedInLine(line_no);
        }
        if !self.rendered && self.squashed {
            return ViewState::Deleted;
        }
        if !self.rendered {
            return ViewState::NotRendered;
        }
        if self.dirty && !self.transfered {
            return ViewState::RenderedAndMarkedAsDirty;
        }
        if self.squashed {
            return ViewState::RenderedAndMarkedAsSquashed;
        }
        ViewState::RenderedNotInLine(line_no)
    }
}

pub enum ViewState {
    RenderedInLine(i32),
    Deleted,
    NotRendered,
    RenderedAndMarkedAsDirty,
    RenderedAndMarkedAsSquashed,
    RenderedNotInLine(i32),
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ViewContainer {
    fn get_kind(&self) -> ViewKind;

    fn child_count(&self) -> usize;

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer>;

    fn get_view(&mut self) -> &mut View;

    fn get_self(&self) -> &dyn ViewContainer;

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

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter) {
        let content = self.get_content();
        let tags = self.tags();
        let view = self.get_view().render(buffer, iter, content, tags);
        if view.expanded || view.child_dirty {
            for child in self.get_children() {
                child.render(buffer, iter)
            }
        }
        self.get_view().child_dirty = false;
    }

    fn cursor(&mut self, line_no: i32, parent_active: bool) -> bool {
        let mut result = false;
        let view = self.get_view();
        // if !view.rendered {
        //   when view is not rendered, it also
        //   could be marked active/inactive
        //   e.g. after expandinf file, all hunks are
        //   expanded and everything inside file is
        //   maked as active
        // }
        let current_before = view.current;
        let active_before = view.active;

        let view_expanded = view.expanded;

        let current = view.is_rendered_in(line_no);
        let active_by_parent = self.is_active_by_parent(parent_active);
        let mut active_by_child = false;

        // todo: make 1 line iter
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
            view.dirty = view.active != active_before || view.current != current_before;
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

    fn expand(&mut self, line_no: i32) -> bool {
        let view = self.get_view();
        let mut found = false;

        if !view.rendered {
            return false;
        }
        if view.line_no == line_no {
            found = true;
            view.expanded = !view.expanded;
            view.dirty = true;
            view.child_dirty = true;
            let expanded = view.expanded;
            self.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let view = vc.get_view();
                if expanded {
                    view.rendered = false;
                } else {
                    view.squashed = true;
                }
            });
        } else if view.expanded {
            // go deeper for self.children
            for child in self.get_children() {
                found = child.expand(line_no);
                if found {
                    break;
                }
            }
        }
        found
    }

    fn erase(&mut self, txt: &TextView) {
        // CAUTION. ATTENTION. IMPORTANT
        // this ONLY rendering
        // the structure is still there. is it ok?
        let view = self.get_view();
        let line_no = view.line_no;
        view.squashed = true;
        view.child_dirty = true;
        self.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let view = vc.get_view();
            view.squashed = true;
            view.child_dirty = true;
        });
        let buffer = txt.buffer();
        let mut iter = buffer
            .iter_at_line(line_no)
            .expect("can't get iter at line");
        self.render(&buffer, &mut iter);
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

    fn get_self(&self) -> &dyn ViewContainer {
        self
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.files
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }

    fn cursor(&mut self, line_no: i32, parent_active: bool) -> bool {
        let mut result = false;
        for file in &mut self.files {
            result = file.cursor(line_no, parent_active) || result;
        }
        result
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter) {
        self.view.line_no = iter.line();
        for file in &mut self.files {
            file.render(buffer, iter);
        }
    }
}

impl ViewContainer for File {
    fn get_kind(&self) -> ViewKind {
        ViewKind::File
    }

    fn child_count(&self) -> usize {
        self.hunks.len()
    }

    fn get_self(&self) -> &dyn ViewContainer {
        self
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

    fn get_self(&self) -> &dyn ViewContainer {
        self
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
}

impl ViewContainer for Line {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Line
    }
    fn child_count(&self) -> usize {
        0
    }
    fn get_self(&self) -> &dyn ViewContainer {
        self
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

    fn expand(&mut self, _line_no: i32) -> bool {
        false
    }

    fn is_active_by_parent(&self, active: bool) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    fn tags(&self) -> Vec<Tag> {
        match self.origin {
            DiffLineType::Addition => vec![Tag::Added],
            DiffLineType::Deletion => vec![Tag::Removed],
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
    fn get_self(&self) -> &dyn ViewContainer {
        self
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

#[derive(Debug, Clone, PartialEq)]
pub enum RenderSource {
    Git,
    Cursor,
    Expand,
    Erase,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub head: Label,
    pub origin: Label,
    pub staged_spacer: Label,
    pub staged_label: Label,
    pub staged: Option<Diff>,
    pub unstaged_spacer: Label,
    pub unstaged_label: Label,
    pub unstaged: Option<Diff>,
    pub rendered: bool,
}

impl Status {
    pub fn new() -> Self {
        Self {
            head: Label::from_string("Head:     common_view refactor cursor"),
            origin: Label::from_string("Origin: common_view refactor cursor"),
            staged_spacer: Label::from_string(""),
            staged_label: Label::from_string("Staged changes"),
            staged: None,
            unstaged_spacer: Label::from_string(""),
            unstaged_label: Label::from_string("Unstaged changes"),
            unstaged: None,
            rendered: false,
        }
    }

    pub fn get_status(&self, sender: Sender<crate::Event>) {
        gio::spawn_blocking({
            move || {
                get_current_repo_status(None, sender);
            }
        });
    }

    pub fn commit_staged(
        &mut self,
        path: &OsString,
        message: String,
        txt: &TextView,
        sender: Sender<crate::Event>,
    ) {
        if let Some(diff) = &mut self.staged {
            // CAUTION. ATTENTION. IMPORTANT
            diff.erase(txt);
            // diff will only erase views visually
            // here we are killing the structure
            // is it ok? git will return new files
            // (there will be no files, actually)
            diff.files = Vec::new();
            gio::spawn_blocking({
                let path = path.clone();
                move || {
                    commit_staged(path, message, sender);
                }
            });
        }
    }
    pub fn update_staged(&mut self, diff: Diff, txt: &TextView) {
        self.staged.replace(diff);
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(&txt, RenderSource::Git);
        }
    }

    pub fn update_unstaged(&mut self, diff: Diff, txt: &TextView) {
        self.unstaged.replace(diff);
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(&txt, RenderSource::Git);
        }
    }

    pub fn cursor(&mut self, txt: &TextView, line_no: i32, offset: i32) {
        let mut changed = false;
        if let Some(unstaged) = &mut self.unstaged {
            changed = changed || unstaged.cursor(line_no, false);
        }
        if let Some(staged) = &mut self.staged {
            changed = changed || staged.cursor(line_no, false);
        }
        if changed {
            self.render(txt, RenderSource::Cursor);
            let buffer = txt.buffer();
            buffer.place_cursor(&buffer.iter_at_offset(offset));
        }
    }

    pub fn expand(&mut self, txt: &TextView, line_no: i32, offset: i32) {
        let mut changed = false;
        if let Some(unstaged) = &mut self.unstaged {
            for file in &mut unstaged.files {
                if file.expand(line_no) {
                    changed = true;
                    break;
                }
            }
        }
        if let Some(staged) = &mut self.staged {
            for file in &mut staged.files {
                if file.expand(line_no) {
                    changed = true;
                    break;
                }
            }
        }
        if changed {
            self.render(txt, RenderSource::Expand);
            // this works only if cursor is on expandable
            // view itself. when it will collapse on line
            // it will not work!
            let buffer = txt.buffer();
            buffer.place_cursor(&buffer.iter_at_offset(offset));
        }
    }
    pub fn render(&mut self, txt: &TextView, source: RenderSource) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);

        self.head.render(&buffer, &mut iter);
        self.origin.render(&buffer, &mut iter);

        self.unstaged_spacer.render(&buffer, &mut iter);
        self.unstaged_label.render(&buffer, &mut iter);
        if let Some(unstaged) = &mut self.unstaged {
            unstaged.render(&buffer, &mut iter);
        }

        self.staged_spacer.render(&buffer, &mut iter);
        self.staged_label.render(&buffer, &mut iter);
        if let Some(staged) = &mut self.staged {
            staged.render(&buffer, &mut iter);
        }

        if source != RenderSource::Cursor {
            // avoid loops on cursor renders
            self.choose_cursor_position(txt, &buffer);
        }
    }

    pub fn stage(
        &mut self,
        txt: &TextView,
        line_no: i32,
        path: &OsString,
        is_staging: bool,
        sender: Sender<crate::Event>,
    ) {
        if is_staging && self.unstaged.is_none() {
            return;
        }
        if !is_staging && self.staged.is_none() {
            return;
        }
        let mut filter = ApplyFilter::default();
        let diff = {
            if is_staging {
                self.unstaged.as_mut().unwrap()
            } else {
                self.staged.as_mut().unwrap()
            }
        };
        let mut file_path_so_stage = String::new();
        diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let content = vc.get_content();
            let kind = vc.get_kind();
            let view = vc.get_view();
            match kind {
                ViewKind::File => {
                    file_path_so_stage = content;
                }
                ViewKind::Hunk => {
                    if !view.active {
                        return;
                    }
                    filter.file_path = file_path_so_stage.clone();
                    filter.hunk_header = content;
                    view.squashed = true;
                }
                ViewKind::Line => {
                    if !view.active {
                        return;
                    }
                    view.squashed = true;
                }
                _ => (),
            }
        });

        if !filter.file_path.is_empty() {
            let buffer = txt.buffer();
            // CAUTION. ATTENTION. IMPORTANT
            // this do both: rendering and changing structure!
            // is it ok?
            diff.files.retain_mut(|f| {
                // it need to remove either whole file
                // or just 1 hunk inside file
                let mut remove_file = false;
                if f.title() == filter.file_path {
                    let hunk_index = f.hunks.iter().position(|h| h.view.squashed).unwrap();
                    if f.hunks.len() == 1 || f.view.current {
                        remove_file = true;
                        f.view.squashed = true;
                    }
                    let mut iter = buffer.iter_at_line(f.view.line_no).unwrap();
                    // CAUTION. ATTENTION. IMPORTANT
                    // rendering just 1 file
                    // but those are used by cursor and expand!
                    f.render(&buffer, &mut iter);

                    f.hunks.remove(hunk_index);
                }
                if remove_file {
                    // kill hunk in filter to stage all hunks
                    filter.hunk_header = String::new();
                    false
                } else {
                    true
                }
            });

            let u = self.unstaged.clone();
            let s = self.staged.clone();
            gio::spawn_blocking({
                let path = path.clone();
                move || {
                    stage_via_apply(u, s, is_staging, path, filter, sender);
                }
            });
        }
    }
    pub fn choose_cursor_position(&mut self, txt: &TextView, buffer: &TextBuffer) {
        if buffer.cursor_position() == buffer.end_iter().offset() {
            // first render. buffer at eof
            if let Some(unstaged) = &self.unstaged {
                if !unstaged.files.is_empty() {
                    let line_no = unstaged.files[0].view.line_no;
                    let iter = buffer.iter_at_line(line_no).unwrap();
                    self.cursor(txt, line_no, iter.offset());
                }
            }
        }
    }
    pub fn has_staged(&self) -> bool {
        if let Some(staged) = &self.staged {
            return !staged.files.is_empty();
        }
        false
    }
}

pub fn debug(_txt: &TextView, _status: &mut Status) {}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn render_view(vc: &mut dyn ViewContainer, mut line_no: i32) -> i32 {
        let view = vc.get_view();
        view.line_no = line_no;
        view.rendered = true;
        view.dirty = false;
        line_no += 1;
        if view.expanded || view.child_dirty {
            for child in vc.get_children() {
                line_no = render_view(child, line_no)
            }
            vc.get_view().child_dirty = false;
        }
        line_no
    }

    pub fn render(diff: &mut Diff) -> i32 {
        let mut line_no: i32 = 0;
        for file in &mut diff.files {
            line_no = render_view(file, line_no);
        }
        line_no
    }

    pub fn cursor(diff: &mut Diff, line_no: i32) {
        for (_, file) in diff.files.iter_mut().enumerate() {
            file.cursor(line_no, false);
        }
        // some views will be rerenderred cause highlight changes
        render(diff);
    }

    #[test]
    pub fn test_single_diff() {
        let mut diff = create_diff();

        render(&mut diff);

        for cursor_line in 0..3 {
            cursor(&mut diff, cursor_line);

            for (i, file) in diff.files.iter_mut().enumerate() {
                let view = file.get_view();
                if i as i32 == cursor_line {
                    assert!(view.active);
                    assert!(view.current);
                } else {
                    assert!(!view.active);
                    assert!(!view.current);
                }
                assert!(!view.expanded);
            }
        }
        // last line from prev loop
        // the cursor is on it
        let mut cursor_line = 2;
        for file in &mut diff.files {
            if file.expand(cursor_line) {
                assert!(file.get_view().child_dirty);
                break;
            }
        }

        render(&mut diff);

        for (i, file) in diff.files.iter_mut().enumerate() {
            let view = file.get_view();
            if i as i32 == cursor_line {
                assert!(view.rendered);
                assert!(view.current);
                assert!(view.active);
                assert!(view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(view.rendered);
                    assert!(view.active);
                    assert!(!view.current);
                });
            } else {
                assert!(!view.current);
                assert!(!view.active);
                assert!(!view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(!view.rendered);
                });
            }
        }

        // go 1 line backward
        // end expand it
        cursor_line = 1;
        cursor(&mut diff, cursor_line);

        for file in &mut diff.files {
            if file.expand(cursor_line) {
                break;
            }
        }

        render(&mut diff);
        for (i, file) in diff.files.iter_mut().enumerate() {
            let view = file.get_view();
            let j = i as i32;
            if j < cursor_line {
                // all are inactive
                assert!(!view.current);
                assert!(!view.active);
                assert!(!view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(!view.rendered);
                });
            } else if j == cursor_line {
                // all are active
                assert!(view.rendered);
                assert!(view.current);
                assert!(view.active);
                assert!(view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(view.rendered);
                    assert!(view.active);
                    assert!(!view.current);
                });
            } else if j > cursor_line {
                // all are expanded but inactive
                assert!(view.rendered);
                assert!(!view.current);
                assert!(!view.active);
                assert!(view.expanded);
                file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                    let view = vc.get_view();
                    assert!(view.rendered);
                    assert!(!view.active);
                    assert!(!view.current);
                });
            }
        }

        // go to first hunk of second file
        cursor_line = 2;
        cursor(&mut diff, cursor_line);
        for file in &mut diff.files {
            if file.expand(cursor_line) {
                for child in file.get_children() {
                    let view = child.get_view();
                    if view.line_no == cursor_line {
                        // hunks were expanded by default.
                        // now they are collapsed!
                        assert!(!view.expanded);
                        assert!(view.child_dirty);
                        for line in child.get_children() {
                            assert!(line.get_view().squashed);
                        }
                    }
                }
                break;
            }
        }
    }
}
