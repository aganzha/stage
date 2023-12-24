use std::ffi;
use gtk::prelude::*;
use gtk::{glib, gdk, pango, TextView, TextBuffer, TextTag, TextIter, TextMark};
use glib::{Sender, subclass::Signal, subclass::signal::SignalId, value::Value};

use crate::{View, Diff, File, Hunk, Line};

const CURSOR_HIGHLIGHT: &str = "CursorHighlight";
const CURSOR_HIGHLIGHT_START: &str  = "CursorHightlightStart";
const CURSOR_HIGHLIGHT_END: &str = "CursorHightlightEnd";
const CURSOR_COLOR: &str = "#f6fecd";

const REGION_HIGHLIGHT: &str = "RegionHighlight";
const REGION_HIGHLIGHT_START: &str  = "RegionHightlightStart";
const REGION_HIGHLIGHT_END: &str = "RegionHightlightEnd";
const REGION_COLOR: &str = "#f2f2f2";

pub fn text_view_factory(sndr: Sender<crate::Event>) ->  TextView {
    let txt = TextView::builder()
        .build();
    let buffer = txt.buffer();
    // let signal_id = signal.signal_id();
    let tag = TextTag::new(Some(CURSOR_HIGHLIGHT));
    tag.set_background(Some(CURSOR_COLOR));
    buffer.tag_table().add(&tag);

    let tag = TextTag::new(Some(REGION_HIGHLIGHT));
    tag.set_background(Some(REGION_COLOR));
    // tag.set_underline(pango::Underline::Single);
    buffer.tag_table().add(&tag);

    let event_controller = gtk::EventControllerKey::new();
    event_controller.connect_key_pressed({
        let buffer = buffer.clone();
        let sndr = sndr.clone();
        move |_, key, _, _| {
            match key {
                gdk::Key::Tab => {
                    println!("taaaaaaaaaaaaaaaaaaaaaaaaaaaaaab!");
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send(crate::Event::Expand(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                },
                gdk::Key::s => {
                    let start_mark = buffer.mark(CURSOR_HIGHLIGHT_START).unwrap();
                    let end_mark = buffer.mark(CURSOR_HIGHLIGHT_END).unwrap();
                    let start_iter = buffer.iter_at_mark(&start_mark);
                    let end_iter = buffer.iter_at_mark(&end_mark);
                    let text = String::from(buffer.text(&start_iter, &end_iter, true).as_str());
                    sndr.send(crate::Event::Stage(text))
                        .expect("Could not send through channel");
                },
                _ => (),
            }
            glib::Propagation::Proceed
        }
        });
    txt.add_controller(event_controller);

    let gesture = gtk::GestureClick::new();
    gesture.connect_released({
        // let txt = txt.clone();
        // let sndr = sndr.clone();
        move |gesture, _some, _wx, _wy| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            // let (x, y) = txt.window_to_buffer_coords(gtk::TextWindowType::Text, wx as i32, wy as i32);
            // let maybe_iter = txt.iter_at_location(x, y);
            // if maybe_iter.is_none() {
            //     return;
            // }
            // let mut iter = maybe_iter.unwrap();
            // highlight_cursor(&txt, iter);
            // sndr.send(crate::Event::HighlightRegion(iter.line()))
            //             .expect("Could not send through channel");
            // let alloc = txt.allocation();
            // println!("Box pressed! {:?} {:?} {:?} {:?} == {:?}", wx, wy, x, y, alloc);
        }
    });

    txt.add_controller(gesture);

    txt.connect_move_cursor({
        let sndr = sndr.clone();
        move |view, step, count, _selection| {
            let buffer = view.buffer();
            let pos = buffer.cursor_position();
            let mut start_iter = buffer.iter_at_offset(pos);
            match step {
                gtk::MovementStep::LogicalPositions |
                gtk::MovementStep::VisualPositions => {
                    start_iter.forward_chars(count);
                },
                gtk::MovementStep::Words => {
                    start_iter.forward_word_end();
                },
                gtk::MovementStep::DisplayLines |
                gtk::MovementStep::DisplayLineEnds |
                gtk::MovementStep::Paragraphs |
                gtk::MovementStep::ParagraphEnds => {
                    start_iter.forward_lines(count);
                },
                gtk::MovementStep::Pages |
                gtk::MovementStep::BufferEnds |
                gtk::MovementStep::HorizontalPages => {
                },
                _ => todo!()
            }
            // highlight_cursor(view, start_iter);
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

    // highlight_cursor(&txt, start_iter);
    // sndr.send(crate::Event::HighlightRegion(start_iter.line()))
    //     .expect("Could not send through channel");
    txt
}

#[derive(Debug, Clone)]
pub struct Region {
    pub kind: ViewKind,
    pub line_from: i32,
    pub line_to: i32,
}

impl Region {
    pub fn new(kind: ViewKind, line_from: i32, line_to: i32) -> Self {
        return Self {
            kind,
            line_from,
            line_to,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.line_to == self.line_from
    }
}



#[derive(Debug, Clone, PartialEq)]
pub enum ViewKind {
    File,
    Hunk,
    Line,
    None
}

impl View {
    pub fn new() -> Self {
        return View {
            line_no: 0,
            expanded: false,
            rendered: false,
            active: false,
            current: false,
            tags: Vec::new()
        }
    }

    fn is_rendered_in_its_place(&self, line_no: i32) -> bool {
        self.rendered && self.line_no == line_no
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter, content: String) -> &mut Self {
        if self.is_rendered_in_its_place(iter.line()) {
            iter.forward_lines(1);  
        } else {
            self.line_no = iter.line();
            let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
            eol_iter.forward_to_line_end();
            let new_line = iter.offset() == eol_iter.offset();

            if new_line {
                buffer.insert(iter, &format!("{} {}\n", iter.line(), content));
            } else {
                buffer.delete(iter, &mut eol_iter);
                buffer.insert(iter, &format!("{} {}", iter.line(), content));
                iter.forward_lines(1);
            }
            self.apply_tags(buffer);
        }
        self.rendered = true;
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
            if index.is_some() {
                buffer.remove_tag_by_name(
                    CURSOR_HIGHLIGHT,
                    &start_iter,
                    &end_iter
                );
                self.tags.remove(index.unwrap());
            }
            if self.active {
                buffer.apply_tag_by_name(REGION_HIGHLIGHT, &start_iter, &end_iter);
                self.tags.push(String::from(REGION_HIGHLIGHT));
            } else {
                let index = self.tags.iter().position(|t| t == REGION_HIGHLIGHT);
                if index.is_some() {
                    buffer.remove_tag_by_name(
                        REGION_HIGHLIGHT,
                        &start_iter,
                        &end_iter
                    );
                    self.tags.remove(index.unwrap());
                }
            }
        }
    }

}


pub trait RecursiveViewContainer {

    fn get_kind(&self) -> ViewKind;

    fn get_children(&mut self) -> Vec<&mut dyn RecursiveViewContainer>;

    fn get_view(&mut self) -> &mut View;

    // TODO - return bool and stop iteration when false
    fn walk_down(&mut self, visitor: &mut dyn FnMut(&mut dyn RecursiveViewContainer) -> ()) {
        for child in self.get_children() {
            visitor(child);
            child.walk_down(visitor);
        }
    }

    fn get_content(&self) -> String;

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter) {

        let content = self.get_content();
        let view = self.get_view().render(&buffer, iter, content);
        if view.expanded {
            for child in self.get_children() {
                child.render(buffer, iter)
            }
        }
    }

    fn cursor(&mut self, line_no: i32, parent_active: bool) -> Option<bool> {        
        let kind = self.get_kind();
        println!("cursor in interface {:?}", kind);
        let view = self.get_view();
        if !view.rendered {
            return None;
        }

        let mut active: bool = false;
        let mut current = false;
        let expanded = view.expanded;
        
        if view.line_no == line_no {
            current = true;
            active = true;
        }
        active = active || self.is_active_by_parent(parent_active);
        println!("cursor in {:?} view {:?} current line {:?} active {:?} parent_active {:?} expanded {:?}",
                 kind,
                 self.get_view().line_no,
                 line_no,
                 active,
                 parent_active,
                 expanded);
        let mut child_active: bool = false;
        if expanded {
            self.walk_down(&mut |rvc: &mut dyn RecursiveViewContainer| {
                println!("will call cursor in interface recursive. i am expanded {:?}", expanded);
                let ca = rvc.cursor(line_no, active);
                if ca.is_some() {
                    child_active = child_active || ca.unwrap();
                }
            });
        }
        active = active || self.is_active_by_child(child_active);
        let view = self.get_view();
        let changed = view.current != current || view.active != active;
        view.current = current;
        view.active = active;
        view.rendered = view.rendered && !changed;
        Some(active)
    }

    fn is_active_by_child(&self, _child_active: bool) -> bool {
        false
    }

    fn is_active_by_parent(&self, _parent_active: bool) -> bool {
        false
    }

    fn expand(&mut self, line_no: i32) {
        let view = self.get_view();
        if !view.rendered {
            return
        }
        if view.line_no == line_no {

            view.expanded = !view.expanded;
            view.rendered = false;
            println!("expand collapse view at {:?} {:?} {:?}", line_no, view.expanded, view.rendered);
            if !view.expanded {
                self.walk_down(&mut |rvc: &mut dyn RecursiveViewContainer| {
                    rvc.get_view().rendered = false;
                })
            }
        } else {
            self.walk_down(&mut |rvc: &mut dyn RecursiveViewContainer| {
                rvc.expand(line_no)
            })
        }
    }

}

impl RecursiveViewContainer for File {

    fn get_kind(&self) -> ViewKind {
        ViewKind::File
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.path.to_str().unwrap().to_string()
    }

    fn get_children(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        self.hunks.iter_mut().map(|vh|vh as &mut dyn RecursiveViewContainer).collect()
    }
}


impl RecursiveViewContainer for Hunk {

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

    fn get_children(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        self.lines.iter_mut().filter(|l| {
            match l.kind {
                crate::LineKind::File => false,
                crate::LineKind::Hunk => false,
                _ => true
            }
        }).map(|vh|vh as &mut dyn RecursiveViewContainer).collect()
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

impl RecursiveViewContainer for Line {

    fn get_kind(&self) -> ViewKind {
        ViewKind::Line
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.content.to_string()
    }

    fn get_children(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        return Vec::new()
    }

    fn expand(&mut self, _line_no: i32) {
    }

    fn is_active_by_parent(&self, active: bool) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        println!("line {:?} is active? {:?}", self.content, active);
        active
    }
}

pub fn expand(view: &TextView, diff: &mut Diff, offset: i32, line_no: i32, sndr:Sender<crate::Event>) {
    // dangerous but required
    diff.offset = offset;
    // diff.expand(line_no);
    for file in &mut diff.files {
        file.expand(line_no)
    }
    render(view, diff, sndr);
}

pub fn cursor(txt: &TextView, diff: &mut Diff, offset: i32, line_no: i32, sndr:Sender<crate::Event>) {
    // dangerous but required
    diff.offset = offset;
    println!("CURSOR ON TOP (call on diff)");

    for file in &mut diff.files {
        file.cursor(line_no, false);
    }
    render(txt, diff, sndr);
}

pub fn render(txt: &TextView, diff: &mut Diff, sndr:Sender<crate::Event>) {
    let buffer = txt.buffer();
    let mut iter = buffer.iter_at_offset(0);

    for file in &mut diff.files {
        file.render(&buffer, &mut iter)
    }

    buffer.delete(&mut iter, &mut buffer.end_iter());
    
    iter.set_offset(diff.offset);
    buffer.place_cursor(&iter);

}
