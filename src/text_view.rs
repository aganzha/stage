use crate::common_tests::*;
use crate::{stage_via_apply, ApplyFilter, Diff, File, Hunk, Line, View};
use glib::Sender;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, TextBuffer, TextIter, TextTag, TextView};
use std::ffi::OsString;


const CURSOR_HIGHLIGHT: &str = "CursorHighlight";
const CURSOR_HIGHLIGHT_START: &str = "CursorHightlightStart";
const CURSOR_HIGHLIGHT_END: &str = "CursorHightlightEnd";
const CURSOR_COLOR: &str = "#f6fecd";

const REGION_HIGHLIGHT: &str = "RegionHighlight";
const REGION_HIGHLIGHT_START: &str = "RegionHightlightStart";
const REGION_HIGHLIGHT_END: &str = "RegionHightlightEnd";
const REGION_COLOR: &str = "#f2f2f2";

pub fn text_view_factory(sndr: Sender<crate::Event>) -> TextView {
    let txt = TextView::builder().build();
    let buffer = txt.buffer();
    // let signal_id = signal.signal_id();
    let tag = TextTag::new(Some(CURSOR_HIGHLIGHT));
    tag.set_background(Some(CURSOR_COLOR));
    buffer.tag_table().add(&tag);

    let tag = TextTag::new(Some(REGION_HIGHLIGHT));
    tag.set_background(Some(REGION_COLOR));
    buffer.tag_table().add(&tag);

    let event_controller = gtk::EventControllerKey::new();
    event_controller.connect_key_pressed({
        let buffer = buffer.clone();
        let sndr = sndr.clone();
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
        move |view, step, count, _selection| {
            let buffer = view.buffer();
            let pos = buffer.cursor_position();
            let mut start_iter = buffer.iter_at_offset(pos);
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
                    start_iter.forward_chars(loffset);
                }
                gtk::MovementStep::DisplayLineEnds
                | gtk::MovementStep::Paragraphs
                | gtk::MovementStep::ParagraphEnds
                | gtk::MovementStep::Pages
                | gtk::MovementStep::BufferEnds
                | gtk::MovementStep::HorizontalPages => {}
                _ => todo!(),
            }
            sndr.send(crate::Event::Cursor(start_iter.offset(), start_iter.line()))
                .expect("Could not send through channel");
        }
    });

    txt.set_monospace(true);
    txt.set_editable(false);

    buffer.place_cursor(&buffer.iter_at_offset(0));

    let start_iter = buffer.iter_at_offset(0);
    buffer.create_mark(Some(CURSOR_HIGHLIGHT_START), &start_iter, false);
    buffer.create_mark(Some(REGION_HIGHLIGHT_START), &start_iter, false);

    let mut end_iter = buffer.iter_at_offset(0);
    end_iter.forward_to_line_end();
    buffer.create_mark(Some(CURSOR_HIGHLIGHT_END), &end_iter, false);
    buffer.create_mark(Some(REGION_HIGHLIGHT_END), &end_iter, false);

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
    pub fn new() -> Self {
        Label {
            content: String::new(),
            view: View::new(),
        }
    }

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
            tags: Vec::new(),
        }
    }

    fn is_rendered_in(&self, line_no: i32) -> bool {
        self.rendered && self.line_no == line_no && !self.dirty && !self.squashed
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter, content: String) -> &mut Self {
        let line_no = iter.line();

        if self.is_rendered_in(line_no) {
            // skip untouched view
            iter.forward_lines(1);
        } else if !self.rendered {
            // render brand new view
            buffer.insert(iter, &format!("{} {}\n", line_no, content));
            self.line_no = line_no;
            self.rendered = true;
            self.apply_tags(buffer);
        } else if self.dirty {
            // replace view with new content
            assert!(self.line_no == line_no);
            let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
            eol_iter.forward_to_line_end();
            buffer.delete(iter, &mut eol_iter);
            buffer.insert(iter, &format!("{} {}", line_no, content));
            iter.forward_lines(1);
            self.apply_tags(buffer);
            self.rendered = true;
        } else if self.squashed {
            // just delete it
            let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
            nel_iter.forward_lines(1);
            buffer.delete(iter, &mut nel_iter);
            self.rendered = false;
        } else {
            // view was just moved to another line
            // due ti expansion/collapsing
            self.line_no = line_no;
            iter.forward_lines(1);
        }

        self.dirty = false;
        self.squashed = false;
        self
    }

    fn apply_tags(&mut self, buffer: &TextBuffer) {
        let mut start_iter = buffer.iter_at_line(self.line_no).unwrap();
        let mut end_iter = buffer.iter_at_line(self.line_no).unwrap();
        start_iter.set_line_offset(0);
        end_iter.forward_to_line_end();
        if self.current {
            buffer.apply_tag_by_name(CURSOR_HIGHLIGHT, &start_iter, &end_iter);
            self.tags.push(String::from(CURSOR_HIGHLIGHT));
        } else {
            let index = self.tags.iter().position(|t| t == CURSOR_HIGHLIGHT);
            if let Some(ind) = index {
                buffer.remove_tag_by_name(CURSOR_HIGHLIGHT, &start_iter, &end_iter);
                self.tags.remove(ind);
            }
            if self.active {
                buffer.apply_tag_by_name(REGION_HIGHLIGHT, &start_iter, &end_iter);
                self.tags.push(String::from(REGION_HIGHLIGHT));
            } else {
                let index = self.tags.iter().position(|t| t == REGION_HIGHLIGHT);
                if let Some(ind) = index {
                    buffer.remove_tag_by_name(REGION_HIGHLIGHT, &start_iter, &end_iter);
                    self.tags.remove(ind);
                }
            }
        }
    }

    pub fn repr(&self) -> String {
        format!(
            "line_no {:?}, expanded {:?}, rendered: {:?}, active {:?}, current {:?}",
            self.line_no, self.expanded, self.rendered, self.active, self.current
        )
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ViewContainer {
    fn get_kind(&self) -> ViewKind;

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer>;

    fn get_view(&mut self) -> &mut View;

    fn line_from(&self) -> i32;

    fn line_to(&self) -> i32;

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

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter) {
        let content = self.get_content();
        let view = self.get_view().render(buffer, iter, content);
        if view.expanded || view.child_dirty {
            for child in self.get_children() {
                child.render(buffer, iter)
            }
        }
        self.get_view().child_dirty = false;
    }

    fn cursor(&mut self, line_no: i32, parent_active: bool) {
        let view = self.get_view();

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
        }
        for child in self.get_children() {
            child.cursor(line_no, self_active);
        }
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
}

impl ViewContainer for Diff {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Diff
    }

    fn line_from(&self) -> i32 {
        if self.files.is_empty() {
            return 0;
        }
        self.files[0].view.line_no
    }

    fn line_to(&self) -> i32 {
        // on diff line_no
        // stores LAST rendered line
        self.view.line_no
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

    fn cursor(&mut self, line_no: i32, parent_active: bool) {
        for file in &mut self.files {
            file.cursor(line_no, parent_active);
        }
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter) {
        for file in &mut self.files {
            file.render(buffer, iter);
        }
        self.view.line_no = iter.line();
    }
}

impl ViewContainer for File {
    fn get_kind(&self) -> ViewKind {
        ViewKind::File
    }

    fn get_self(&self) -> &dyn ViewContainer {
        self
    }

    fn line_from(&self) -> i32 {
        self.view.line_no
    }

    fn line_to(&self) -> i32 {
        todo!();
        self.view.line_no
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
}

impl ViewContainer for Hunk {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Hunk
    }

    fn get_self(&self) -> &dyn ViewContainer {
        self
    }
    fn line_from(&self) -> i32 {
        self.view.line_no
    }

    fn line_to(&self) -> i32 {
        todo!();
        self.view.line_no
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
            .filter(|l| !matches!(l.kind, crate::LineKind::File | crate::LineKind::Hunk))
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
}

impl ViewContainer for Line {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Line
    }
    fn get_self(&self) -> &dyn ViewContainer {
        self
    }
    fn line_from(&self) -> i32 {
        self.view.line_no
    }

    fn line_to(&self) -> i32 {
        self.view.line_no
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
}

impl ViewContainer for Label {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
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

    fn line_to(&self) -> i32 {
        self.view.line_no
    }

    fn line_from(&self) -> i32 {
        self.view.line_no
    }
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub head: Label,
    pub origin: Label,
    pub staged_label: Label,
    pub staged: Option<Diff>,
    pub unstaged_label: Label,
    pub unstaged: Option<Diff>,
    pub rendered: bool,
}

impl Status {
    pub fn new() -> Self {
        Self {
            head: Label::new(),
            origin: Label::new(),
            staged_label: Label::new(),
            staged: None,
            unstaged_label: Label::new(),
            unstaged: None,
            rendered: false,
        }
    }
}

impl Status {
    fn try_stage(
        &mut self,
        txt: &TextView,
        line_no: i32,
        path: &OsString,
        sender: Sender<crate::Event>,
    ) {
        let mut filter = ApplyFilter::default();
        let mut parent_hunk = String::from("");
        let mut parent_file = String::from("");
        let mut found = false;
        let unstaged = self.unstaged.as_mut().unwrap();
        unstaged.walk_down(&mut |vc: &mut dyn ViewContainer| {
            if vc.get_kind() == ViewKind::File {
                parent_file = vc.get_content();
            }
            if vc.get_kind() == ViewKind::Hunk {
                parent_hunk = vc.get_content();
            }

            let v = vc.get_view();
            if v.line_no == line_no {
                found = true;
                match vc.get_kind() {
                    ViewKind::File => {
                        filter.file_path = vc.get_content();
                    }
                    ViewKind::Hunk => {
                        filter.file_path = parent_file.clone();
                        filter.hunk_header = vc.get_content();
                    }
                    ViewKind::Line => {
                        filter.file_path = parent_file.clone();
                        filter.hunk_header = parent_hunk.clone();
                    }
                    _ => {}
                };
            }
        });
        if !filter.file_path.is_empty() {
            for file in &mut unstaged.files {
                if file.title() == filter.file_path {
                    if !filter.hunk_header.is_empty() {
                        // staging hunk. all hunk headers
                        // will be changed. it need to kill everything
                        // inside file (probably only AFTER this hunk
                        // including it, cause prev hunks are not changed!!)
                        // lets just try kill everything
                        file.walk_down(&mut |vc: &mut dyn ViewContainer| {
                            let view = vc.get_view();
                            view.squashed = true;
                        });
                        file.get_view().child_dirty = true;
                    } else {
                        file.get_view().squashed = true;
                    }

                    let buffer = txt.buffer();
                    let mut iter = buffer.iter_at_line(file.view.line_no).unwrap();
                    file.render(&buffer, &mut iter);
                    break;
                }
            }

            let u = self.unstaged.clone();
            let s = self.staged.clone();
            gio::spawn_blocking({
                let path = path.clone();
                move || {
                    stage_via_apply(u.unwrap(), s, path, filter, sender);
                }
            });
        }
    }
}

pub fn expand(
    txt: &TextView,
    status: &mut Status,
    offset: i32,
    line_no: i32,
    sndr: Sender<crate::Event>,
) {
    let mut expanded = false;

    // lines changed during expand/collapse
    // e.g. +10 or -10
    // will be applied to the view next to expanded/collapsed
    let mut delta: i32 = 0;
    let mut last_line: i32 = 0;

    // 1 view will be marked expanded/collapsed
    // next view will be marked squashed, to delete preceeding lines
    // on render. squashed view could be on another diff!
    let mut diffs: Vec<&mut Diff> = Vec::new();
    if status.unstaged.is_some() {
        diffs.push(status.unstaged.as_mut().unwrap());
    }
    if status.staged.is_some() {
        diffs.push(status.staged.as_mut().unwrap());
    }
    for diff in diffs {
        for file in &mut diff.files {
            if !expanded {
                expanded = file.expand(line_no);
            }
            if expanded {
                break;
            }
        }

        if expanded {
            // render expanded and every other diff
            let from = diff.line_from();
            let to = diff.line_to();

            let buffer = txt.buffer();
            let mut iter = buffer.iter_at_line(from + delta).unwrap();
            diff.render(&buffer, &mut iter);

            last_line = diff.line_to();
            delta = last_line - to;
        }
    }

    if expanded {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_line(last_line).unwrap();
        buffer.delete(&mut iter, &mut buffer.end_iter());
        iter.set_offset(offset);
        buffer.place_cursor(&iter);
    }
}

pub fn stage(
    txt: &TextView,
    status: &mut Status,
    offset: i32,
    line_no: i32,
    repo_path: &OsString,
    sndr: Sender<crate::Event>,
) {
    if status.unstaged.is_some() {
        status.try_stage(txt, line_no, repo_path, sndr);
    }
}

pub fn cursor(
    txt: &TextView,
    status: &mut Status,
    offset: i32,
    line_no: i32,
    _sndr: Sender<crate::Event>,
) {
    let mut diffs: Vec<&mut Diff> = Vec::new();
    if status.unstaged.is_some() {
        diffs.push(status.unstaged.as_mut().unwrap());
    }
    if status.staged.is_some() {
        diffs.push(status.staged.as_mut().unwrap());
    }
    for diff in diffs {
        diff.cursor(line_no, false);
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_line(diff.line_from()).unwrap();
        diff.render(&buffer, &mut iter);

        // TODO: this cursor is set in loop! fix it!
        iter.set_offset(offset);
        iter.buffer().place_cursor(&iter);
    }
}

pub fn render_status(txt: &TextView, status: &mut Status, _sndr: Sender<crate::Event>) {
    let buffer = txt.buffer();
    let mut iter = buffer.iter_at_offset(0);

    if status.rendered {
        iter.forward_lines(1);
    } else {
        status.head = Label::from_string("Head:     common_view refactor cursor");
        status.head.render(&buffer, &mut iter);
    }

    if status.rendered {
        iter.forward_lines(1);
    } else {
        status.origin = Label::from_string("Origin: common_view refactor cursor");
        status.origin.render(&buffer, &mut iter);
    }

    if status.unstaged.is_some() {
        if status.rendered {
            iter.forward_lines(1);
        } else {
            status.unstaged_label = Label::from_string("Unstaged changes");
            status.unstaged_label.render(&buffer, &mut iter);
        }
        println!(
            "rendering unstaged at line ------------------- {:?}",
            iter.line()
        );
        dbg!(&status.unstaged.as_ref().unwrap().files[0].view);
        status.unstaged.as_mut().unwrap().render(&buffer, &mut iter);
    }

    if status.staged.is_some() {
        if status.rendered {
            iter.forward_lines(1);
        } else {
            status.staged_label = Label::from_string("Staged changes");
            status.staged_label.render(&buffer, &mut iter);
        }
        status.staged.as_mut().unwrap().render(&buffer, &mut iter);
    }

    // buffer.delete(&mut iter, &mut buffer.end_iter());
    status.rendered = true;
}

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
