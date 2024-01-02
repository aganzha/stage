use glib::Sender;
use gtk::prelude::*;
use gtk::{gdk, glib, TextBuffer, TextIter, TextTag, TextView};

use crate::{Diff, File, Hunk, Line, View, DiffView, Status, StatusView};

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
                    println!("TAAAAAAAAAAAAAAAAAAB {:?}", iter.line());
                    sndr.send(crate::Event::Expand(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                }
                gdk::Key::s => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send(crate::Event::Stage(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                    // let start_mark = buffer.mark(CURSOR_HIGHLIGHT_START).unwrap();
                    // let end_mark = buffer.mark(CURSOR_HIGHLIGHT_END).unwrap();
                    // let start_iter = buffer.iter_at_mark(&start_mark);
                    // let end_iter = buffer.iter_at_mark(&end_mark);
                    // let text = String::from(buffer.text(&start_iter, &end_iter, true).as_str());
                    // sndr.send(crate::Event::Stage(text))
                    //     .expect("Could not send through channel");
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
            let (x, y) = txt.window_to_buffer_coords(gtk::TextWindowType::Text, wx as i32, wy as i32);
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
    File,
    Hunk,
    Line,
}

impl View {
    pub fn new() -> Self {
        View {
            line_no: 0,
            expanded: false,
            squashed: false,
            rendered: false,
            dirty: false,
            active: false,
            current: false,
            tags: Vec::new(),
        }
    }

    fn is_rendered_in(&self, line_no: i32) -> bool {
        self.rendered && self.line_no == line_no && !self.dirty
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter, content: String) -> &mut Self {
        if self.is_rendered_in(iter.line()) {
            iter.forward_lines(1);
        } else {
            let line_no = iter.line();
            println!("is this same or new line???? {:?} rendered - {:?}, dirty - {:?}", line_no, self.rendered, self.dirty);
            if !self.rendered {
                println!("just insert new line {:?}", line_no);
                buffer.insert(iter, &format!("{} {}\n", line_no, content));
            } else {
                println!("the view is already rendered, but it need to rerender it at {:?}", line_no);
                dbg!(self.clone());
                // if view is dirty - render it on this line
                // but it need to assert if it is on same line
                if self.dirty {
                    println!("dirty view");
                    assert!(self.line_no == line_no);
                    let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
                    eol_iter.forward_to_line_end();
                    buffer.delete(iter, &mut eol_iter);
                    buffer.insert(iter, &format!("{} {}", line_no, content));
                } else if self.squashed && self.line_no > line_no {
                    // squashing view must be moved.
                    // either upward or downward.
                    // render comes from top to bottom.
                    // so, there will be only ONE move during render:
                    // either upward (delete lines) or downward

                    // HOW TO DETECT THAT IT NEED TO KILL LINES????
                    // ???????????????????
                    // it need to do it only once!
                    // only when this is next view to collapsed one!
                    // how do i know that it is next?
                    // collapsed view is that one, on which the cursor is placed
                    // but because of static views, it could be several lines apart
                    // ...
                    println!("SQUASHING VIEW. this line {:?}, view line {:?}",
                             line_no,
                             self.line_no
                    );
                    let mut my_iter = buffer.iter_at_line(self.line_no).unwrap();
                    buffer.delete(iter, &mut my_iter);

                } else {
                    println!("just skip this view. new line will be instaled on it");
                }
                iter.forward_lines(1);
            }
            self.line_no = line_no;
            self.apply_tags(buffer);
        }
        self.rendered = true;
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
        format!("line_no {:?}, expanded {:?}, rendered: {:?}, active {:?}, current {:?}",
             self.line_no, self.expanded, self.rendered, self.active, self.current)
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffView {
    pub fn new() -> Self {
        Self {
            line_from: 0,
            line_to: 0,
            text: String::new()
        }
    }
    pub fn region(&self) -> (i32, i32) {
        (self.line_from, self.line_to)
    }
}

impl Default for DiffView {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusView {
    pub fn new() -> Self {
        Self {
            user_cursor: 0,
            current_line: 0,
            current_offset: 0
        }
    }

    pub fn position(&self) -> (i32, i32) {
        (self.current_offset, self.current_line)
    }

    pub fn save_position(&mut self, iter: &TextIter) {
        self.current_line = iter.line();
        self.current_offset = iter.offset();
    }
}

impl Default for StatusView {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ViewContainer {
    fn get_kind(&self) -> ViewKind;

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer>;

    fn get_view(&mut self) -> &mut View;

    // TODO - return bool and stop iteration when false
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
        if view.expanded {
            for child in self.get_children() {
                child.render(buffer, iter)
            }
        }
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

    fn expand(&mut self, line_no: i32) -> (bool, bool) {
        let view = self.get_view();
        let mut found = false;
        let mut squash_found = false;
        if !view.rendered {
            return (false, false);
        }
        if view.line_no == line_no {
            // mark this view expanded/collapsed
            // squashed view wiould be marked in the loop
            // above this
            found = true;
            view.expanded = !view.expanded;
            // need to rerender
            view.dirty = true;
            // kill all children
            self.walk_down(&mut |rvc: &mut dyn ViewContainer| {
                let view = rvc.get_view();
                view.rendered = false;
            });
        } else if view.expanded {
            // go deeper for self.children
            for child in self.get_children() {
                if found {
                    // found is previous child
                    // this child in loop is squashed
                    child.get_view().squashed = true;
                    squash_found = true;
                    break;
                }
                (found, squash_found) = child.expand(line_no);
                // mark expanded and squashed deeper
                if found && squash_found {
                    break
                }
            }
        }
        (found, squash_found)
    }
}

impl ViewContainer for File {
    fn get_kind(&self) -> ViewKind {
        ViewKind::File
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.path.to_str().unwrap().to_string()
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

    fn get_content(&self) -> String {
        self.header.to_string()
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

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.content.to_string()
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn expand(&mut self, _line_no: i32) -> (bool, bool) {
        (false, false)
    }

    fn is_active_by_parent(&self, active: bool) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
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
    let mut squashed = false;
    let mut delta: i32 = 0;
    let mut last_line: i32 = 0;
    // 1 view will be marked expanded/collapsed
    // next view will be marked squashed, to delete preceeding lines
    // on render
    for diff in [&mut status.unstaged, &mut status.staged] {
        println!("LOOOOOOOOOOOP FOR DIFF expand squahswed {:?} {:?}", expanded, squashed);
        for file in &mut diff.files {
            if expanded && !squashed {
                // mark next file for squashing
                // it could be in another diff!
                file.get_view().squashed = true;
                squashed = true;
                break;
            }
            if !expanded {
                // try to expand only once
                (expanded, squashed) = file.expand(line_no);
            }
            if expanded && squashed {
                // all set
                break;
            }
        }
        if expanded {
            // render expanded and every other diff
            let from = diff.view.line_from;
            let to = diff.view.line_to;
            render_diff(txt, diff, from + delta);
            last_line = diff.view.line_to;
            delta =  last_line - to;
        }
    }
    let buffer = txt.buffer();
    let mut iter = buffer.iter_at_line(last_line).unwrap();
    buffer.delete(&mut iter, &mut buffer.end_iter());
    iter.set_offset(offset);
    buffer.place_cursor(&iter);
}

pub fn cursor(
    txt: &TextView,
    status: &mut Status,
    offset: i32,
    line_no: i32,
    _sndr: Sender<crate::Event>,
) {
    status.view.user_cursor = offset;

    for diff in [&mut status.unstaged, &mut status.staged] {
        for file in &mut diff.files {
            file.cursor(line_no, false);
        }
        // are you sure it must be diff.view.line_from ??????
        let mut iter = render_diff(txt, diff, diff.view.line_from);
        // TODO: this cursor is set in loop! fix it!
        iter.set_offset(offset);
        iter.buffer().place_cursor(&iter);
    }
}

pub fn render_diff(txt: &TextView, diff: &mut Diff, line_no: i32) -> TextIter {
    let buffer = txt.buffer();
    diff.view.line_from = line_no;
    println!("rendering diff for line {:?}", line_no);
    let mut iter = buffer.iter_at_line(line_no).unwrap();

    for file in &mut diff.files {
        file.render(&buffer, &mut iter)
    }
    diff.view.line_to = iter.line();
    iter

}

pub fn render_status(txt: &TextView, status: &mut Status, _sndr: Sender<crate::Event>) {
    let buffer = txt.buffer();
    let mut iter = buffer.iter_at_offset(0);
    buffer.insert(&mut iter, "Unstaged changes:\n");
    // TODO? why the fuck i need save_position at all???

    // render first diff
    status.view.save_position(&iter);
    iter.forward_lines(1);

    iter = render_diff(txt, &mut status.unstaged, iter.line());
    status.view.save_position(&iter);

    // render second diff
    iter.forward_lines(1);
    iter.set_line_offset(0);
    buffer.insert(&mut iter, "Staged changes:\n");

    status.view.save_position(&iter);
    iter.forward_lines(1);
    iter = render_diff(txt, &mut status.staged, iter.line());
    status.view.save_position(&iter);
    buffer.delete(&mut iter, &mut buffer.end_iter());
}


#[cfg(test)]
mod tests {
    use super::*;
            fn create_line(prefix: i32) -> Line {
            let mut line = Line::new();
            line.content = format!("line {}", prefix);
            line.kind = crate::LineKind::Regular;
            line
        }

        fn create_hunk(prefix: i32) -> Hunk {
            let mut hunk = Hunk::new();
            hunk.header = format!("hunk {}", prefix);
            for i in 0..3 {
                hunk.lines.push(create_line(i))
            }
            hunk
        }

        fn create_file(prefix: i32) -> File {
            let mut file = File::new();
            file.path = format!("file{}.rs", prefix).into();
            for i in 0..3 {
                file.hunks.push(create_hunk(i))
            }
            file
        }

        fn create_diff() -> Diff {

            let mut diff = Diff::new();
            for i in 0..3 {
                diff.files.push(create_file(i));
            }
            diff
        }

    mod single_diff {
        use super::*;

        pub fn render_view(vc: &mut dyn ViewContainer, mut line_no: i32) -> i32 {
            let view = vc.get_view();
            view.line_no = line_no;
            view.rendered = true;
            view.dirty = false;
            line_no += 1;
            if view.expanded {
                for child in vc.get_children() {
                    line_no = render_view(child, line_no)
                }
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
        pub fn test() {

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
        }
    }
}
