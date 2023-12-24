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
        let txt = txt.clone();
        let sndr = sndr.clone();
        move |gesture, _some, wx, wy| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let (x, y) = txt.window_to_buffer_coords(gtk::TextWindowType::Text, wx as i32, wy as i32);
            let maybe_iter = txt.iter_at_location(x, y);
            if maybe_iter.is_none() {
                return;
            }
            highlight_cursor(&txt, maybe_iter.unwrap(), sndr.clone());
            let alloc = txt.allocation();
            println!("Box pressed! {:?} {:?} {:?} {:?} == {:?}", wx, wy, x, y, alloc);
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
            highlight_cursor(view, start_iter, sndr.clone());
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
    
    highlight_cursor(&txt, start_iter, sndr);

    txt
}

#[derive(Debug, Clone)]
pub struct Region {
    pub line_from: i32,
    pub line_to: i32
}

impl Region {
    pub fn new(line_from: i32, line_to: i32) -> Self {
        return Self {
            line_from,
            line_to
        }
    }
    pub fn is_empty(&self) -> bool {
        self.line_to == self.line_from
    }
}



impl Diff {
    pub fn get_active_region(&mut self, line: i32) -> Region {
        // println!("get active region for {:?}", line);
        let mut start_line = 0;
        let mut end_line = 0;

        let mut current_file_line: i32 = 0;
        let mut current_hunk_line: i32 = 0;
        let mut current_line_line: i32 = 0;

        let mut stop: bool = false;
        let mut next_stop: Option<ViewKind> = None;
        
        self.walk_down(&mut |rvc: &mut dyn RecursiveViewContainer| {
            // println!("walk {:?} {:?} {:?} {:?}", current_file_line, current_hunk_line, current_line_line, stop);
            if stop {
                return;
            }
            let view = rvc.get_view();
            if !view.rendered {
                return
            }
            let current_line = view.line_no;
            let expanded = view.expanded;
            // println!("view line and expanded {:?} {:?} {:?}", rvc.get_content(), current_line, expanded);
            let kind = rvc.get_kind();            
            match kind {
                ViewKind::None => (),
                ViewKind::File => {
                    match next_stop {
                        Some(ViewKind::File) => {
                            println!("next stop is file. current_line {:?} current_file_line {:?} content {:?}", current_line, current_file_line, rvc.get_content());
                            start_line = current_file_line;
                            end_line = current_line;
                            stop = true;
                            return;
                        },
                        _ => ()
                    }                    
                    current_file_line = current_line;
                },
                ViewKind::Hunk => {
                    match next_stop {
                        Some(ViewKind::Hunk) => {
                            println!("next stop is HUNK. current_line {:?} current_hunk_line {:?} content {:?}", current_line, current_hunk_line, rvc.get_content());
                            start_line = current_hunk_line;
                            end_line = current_line;
                            stop = true;
                            return;
                        },
                        _ => ()
                    }     
                    current_hunk_line = current_line;
                }
                ViewKind::Line => {
                    current_line_line = current_line;
                }
            }
            
            if current_line == line {
                match kind {
                    ViewKind::None => (),
                    ViewKind::File => {
                        if expanded {
                            start_line = current_file_line;
                            next_stop.replace(ViewKind::File);
                        }
                    },
                    ViewKind::Hunk => {
                        if expanded {
                            start_line = current_hunk_line;
                            next_stop.replace(ViewKind::Hunk);
                        } else {
                            start_line = current_file_line;
                        }
                    }
                    ViewKind::Line => {
                        start_line = current_hunk_line;
                        next_stop.replace(ViewKind::Hunk);
                    }
                }
            }
        });
        // println!("JUST WALKED {:?}. Result start {:?} end {:?}, last line {:?} {:?}", next_stop, start_line, end_line, current_hunk_line, current_line_line);
        if end_line == 0 && next_stop.is_some() {
            // eof while last something is expanded            
            end_line = current_line_line;
        }
        Region::new(start_line, end_line)
    }
}

#[derive(Debug, Clone)]
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
            rendered: false
        }
    }

    fn is_rendered_in_its_place(&self, line_no: i32) -> bool {
        self.rendered && self.line_no == line_no
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter, content: String) -> &mut Self {
        if self.is_rendered_in_its_place(iter.line()) {
            iter.forward_lines(1);
        } else {
            buffer.insert(iter, &format!("{} {}", iter.line(), content));
            self.line_no = iter.line();
            buffer.insert(iter, "\n");
        }
        self.rendered = true;
        self
    }
}


pub trait RecursiveViewContainer {

    fn get_kind(&self) -> ViewKind;

    fn get_children(&mut self) -> Vec<&mut dyn RecursiveViewContainer>;

    fn get_view(&mut self) -> &mut View;

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

impl RecursiveViewContainer for Diff {

    fn get_kind(&self) -> ViewKind {
        ViewKind::None
    }

    fn get_view(&mut self) -> &mut View {
        panic!("why are you here? this must be never called");
        &mut View::new()
    }

    fn get_content(&self) -> String {
        panic!("why are you here? this must be never called");
        String::from("")
    }

    fn get_children(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        self.files.iter_mut().map(|f| f as &mut dyn RecursiveViewContainer).collect()
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter) {
        for child in self.get_children() {
            child.render(buffer, iter)
        }
    }

    fn expand(&mut self, line_no: i32) {
        self.walk_down(&mut |rvc: &mut dyn RecursiveViewContainer| {
            rvc.expand(line_no)
        })
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
}

pub fn apply_tag_on_marks(buffer: TextBuffer,
                          start_mark: &str,
                          end_mark: &str,
                          tag_name: &str) {
    let start_mark = buffer.mark(start_mark);
    let end_mark = buffer.mark(end_mark);
    if start_mark.is_none() || end_mark.is_none() {
        return
    }
    buffer.apply_tag_by_name(
        tag_name,
        &buffer.iter_at_mark(&start_mark.unwrap()),
        &buffer.iter_at_mark(&end_mark.unwrap())
    );
}

pub fn highlight_cursor(view: &TextView,
                        mut start_iter: gtk::TextIter,
                        sndr: Sender<crate::Event>) {
    sndr.send(crate::Event::HighlightRegion(start_iter.line()))
                        .expect("Could not send through channel");
    let buffer = view.buffer();
    let start_mark = buffer.mark(CURSOR_HIGHLIGHT_START).unwrap();
    if start_iter.line() ==  buffer.iter_at_mark(&start_mark).line() {
        return;
    }
    let end_mark = buffer.mark(CURSOR_HIGHLIGHT_END).unwrap();
    buffer.remove_tag_by_name(
        CURSOR_HIGHLIGHT,
        &buffer.iter_at_mark(&start_mark),
        &buffer.iter_at_mark(&end_mark)
    );
    start_iter.set_line_offset(0);
    let start_pos = start_iter.offset();
    let mut end_iter = buffer.iter_at_offset(start_iter.offset());
    end_iter.forward_to_line_end();

    // let mut cnt = 0;
    // let  max_width = view.visible_rect().width();
    // while max_width >  view.iter_location(&end_iter).x() {
    //     buffer.insert(&mut end_iter, " ");
    //     cnt += 1;
    //     if cnt > 100 {
    //         break;
    //     }
    // }
    // if cnt > 0 {
    //     buffer.backspace(&mut end_iter, false, true);
    // }
    let start_iter = buffer.iter_at_offset(start_pos);
    buffer.move_mark(&start_mark, &start_iter);
    buffer.move_mark(&end_mark, &end_iter);
    buffer.apply_tag_by_name(CURSOR_HIGHLIGHT, &start_iter, &end_iter);
    // buffer.apply_tag_by_name(CURSOR_HIGHLIGHT, &start_iter, &end_iter);
}

pub fn highlight_region(view: &TextView, r: Region) {
    // println!("highlight_region {:?}", r);
    
    let buffer = view.buffer();
    let start_mark = buffer.mark(REGION_HIGHLIGHT_START).unwrap();
    let end_mark = buffer.mark(REGION_HIGHLIGHT_END).unwrap();
    let mut start_iter = buffer.iter_at_mark(&start_mark);
    let mut end_iter = buffer.iter_at_mark(&end_mark);
    if start_iter.line() == r.line_from && end_iter.line() == r.line_to {
        return
    }
    // let tag = buffer.tag_table().lookup(REGION_HIGHLIGHT);
    // if tag.is_some() {
    //     println!("remove tag!");
    //     buffer.remove_tag(&tag.unwrap(), &start_iter, &end_iter);
    // }
    buffer.remove_tag_by_name(
        REGION_HIGHLIGHT,
        &start_iter,
        &end_iter
    );
    if r.is_empty() {
        return
    }

    start_iter.set_line(r.line_from);
    start_iter.forward_lines(1);
    end_iter.set_line(r.line_to);
    end_iter.backward_char();
    
    // end_iter.backward_line();
    // end_iter.backward_line();
    // end_iter.backward_char();
    buffer.move_mark(&start_mark, &start_iter);
    buffer.move_mark(&end_mark, &end_iter);
    println!("APPLY TAG AT {:?} {:?}. offsets {:?} {:?}", start_iter.line(), end_iter.line(), start_iter.line_offset(), end_iter.line_offset());
    // println!("{:?}", buffer.slice(&start_iter, &end_iter, true));
    buffer.apply_tag_by_name(REGION_HIGHLIGHT, &start_iter, &end_iter);
    // let tag = buffer.tag_table().lookup(REGION_HIGHLIGHT);
    // println!("tag after apply! {:?}", tag);
    // if tag.is_some() {
    //     println!("tag changed");
    //     tag.unwrap().changed(true);
    // }
    // does not work
    // apply_tag_on_marks(buffer, CURSOR_HIGHLIGHT, CURSOR_HIGHLIGHT_START, CURSOR_HIGHLIGHT_END);
}

pub fn expand(view: &TextView, diff: &mut Diff, offset: i32, line_no: i32, sndr:Sender<crate::Event>) {
    diff.offset = offset;
    diff.expand(line_no);
    render(view, diff, sndr);
    println!("FINISHED RENDER IN EXPAND");
}

pub fn render(view: &TextView, diff: &mut Diff, sndr:Sender<crate::Event>) {
    let buffer = view.buffer();
    let mut iter = buffer.iter_at_offset(0);

    diff.render(&buffer, &mut iter);

    buffer.delete(&mut iter, &mut buffer.end_iter());

    iter.set_offset(diff.offset);
    buffer.place_cursor(&iter);
    highlight_cursor(&view, iter, sndr);
}
