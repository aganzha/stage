use std::ffi;
use gtk::prelude::*;
use gtk::{glib, gdk, TextView, TextBuffer, TextTag, TextIter};
use glib::{Sender, subclass::Signal, subclass::signal::SignalId, value::Value};
use crate::{View, Diff, File, Hunk, Line};

const HIGHLIGHT: &str = "highlight";
const HIGHLIGHT_START: &str  = "HightlightStart";
const HIGHLIGHT_END: &str = "HightlightEnd";

pub fn text_view_factory(sndr: Sender<crate::Event>) ->  TextView {
    let txt = TextView::builder()
        .build();
    let buffer = txt.buffer();
    // let signal_id = signal.signal_id();
    let tag = TextTag::new(Some(HIGHLIGHT));
    tag.set_background(Some("#f6fecd"));

    let event_controller = gtk::EventControllerKey::new();
    event_controller.connect_key_pressed({
        let buffer = buffer.clone();
        move |_, key, _, _| {
            match key {
                gdk::Key::Tab => {
                    println!("taaaaaaaaaaaaaaaaaaaaaaaaaaaaaab!");
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send(crate::Event::Expand(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                },
                gdk::Key::s => {
                    let start_mark = buffer.mark(HIGHLIGHT_START).unwrap();
                    let end_mark = buffer.mark(HIGHLIGHT_END).unwrap();
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
        move |gesture, _some, wx, wy| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let (x, y) = txt.window_to_buffer_coords(gtk::TextWindowType::Text, wx as i32, wy as i32);
            let maybe_iter = txt.iter_at_location(x, y);
            if maybe_iter.is_none() {
                return;
            }
            highlight_if_need(&txt, maybe_iter.unwrap());
            let alloc = txt.allocation();
            println!("Box pressed! {:?} {:?} {:?} {:?} == {:?}", wx, wy, x, y, alloc);
        }
    });

    txt.add_controller(gesture);

    txt.connect_move_cursor({
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
            highlight_if_need(view, start_iter);
        }
    });

    buffer.tag_table().add(&tag);
    txt.set_monospace(true);
    txt.set_editable(false);

    buffer.place_cursor(&buffer.iter_at_offset(0));


    let start_iter = buffer.iter_at_offset(0);
    buffer.create_mark(Some(HIGHLIGHT_START), &start_iter, false);

    let mut end_iter = buffer.iter_at_offset(0);
    end_iter.forward_to_line_end();
    buffer.create_mark(Some(HIGHLIGHT_END), &end_iter, false);

    highlight_if_need(&txt, start_iter);

    txt
}

pub fn highlight_if_need(view: &TextView,
                         mut start_iter: gtk::TextIter) {
    let buffer = view.buffer();
    let start_mark = buffer.mark(HIGHLIGHT_START).unwrap();
    if start_iter.line() ==  buffer.iter_at_mark(&start_mark).line() {
        return;
    }
    let end_mark = buffer.mark(HIGHLIGHT_END).unwrap();
    buffer.remove_tag_by_name(
        HIGHLIGHT,
        &buffer.iter_at_mark(&start_mark),
        &buffer.iter_at_mark(&end_mark)
    );
    start_iter.set_line_offset(0);
    let start_pos = start_iter.offset();
    let mut end_iter = buffer.iter_at_offset(start_iter.offset());
    end_iter.forward_to_line_end();

    let mut cnt = 0;
    let  max_width = view.visible_rect().width();
    while max_width >  view.iter_location(&end_iter).x() {
        buffer.insert(&mut end_iter, " ");
        cnt += 1;
        if cnt > 100 {
            break;
        }
    }
    if cnt > 0 {
        buffer.backspace(&mut end_iter, false, true);
    }
    let start_iter = buffer.iter_at_offset(start_pos);
    buffer.move_mark(&start_mark, &start_iter);
    buffer.move_mark(&end_mark, &end_iter);
    println!("APPLY!");
    buffer.apply_tag_by_name(HIGHLIGHT, &start_iter, &end_iter);

}

impl Diff {

    pub fn set_expand(&mut self, offset: i32, line_no: i32) {
        self.offset = offset;
        for file in &mut self.files {
            // getting view here with line does not make sense. it must be already ecists
            let view = file.get_view(line_no);
            println!("do it need to set expand? on line {:?} for view {:?}", line_no, view.line_no);
            if view.line_no == line_no {
                println!("yes, set, please");
                view.expanded = !view.expanded;
                // when some item is expanded/collapsed
                // all other views after it will become invalid
                // by is_renderred_in_its_place
                // and will be rerendered. no further calculations
                // are reuired for them
                view.rendered = false;
                // but if view is collapsed it need to mark all inside it as not renderred
                if !view.expanded {
                    for hunk in &mut file.hunks {
                        // getting view here with line does not make sense. it must be already ecists
                        hunk.get_view(line_no).rendered = false;
                        for line in &mut hunk.lines {
                            line.get_view(line_no).rendered = false;
                        }
                    }
                }
            }
        }
    }
}



impl View {
    fn new(line_no: i32, expanded: bool, content: String) -> Self {
        return View {
            line_no: line_no,
            expanded: expanded,
            rendered: false,
            content: content
        }
    }

    fn is_renderred_in_its_place(&self, line_no: i32) -> bool {
        self.rendered && self.line_no == line_no
    }

    fn render(&mut self, buffer: &TextBuffer, iter: &mut TextIter) -> &mut Self {
        if self.is_renderred_in_its_place(iter.line()) {
            iter.forward_lines(1);
        } else {
            buffer.insert(iter, &self.content);
            self.line_no = iter.line();
            buffer.insert(iter, "\n");
        }
        self.rendered = true;
        self
    }
}

pub trait ViewHolder {
    fn get_view(&mut self, line_no: i32) -> &mut View;
}

pub trait ViewsContainer {
    fn get_views(&mut self) -> Vec<&mut dyn ViewHolder>;
}


pub trait RecursiveViewContainer {
    fn get_own_view(&mut self, line_no: i32) -> &mut View;
    fn get_children_views(&mut self) -> Vec<&mut dyn RecursiveViewContainer>;
}

impl ViewsContainer for Diff {
    fn get_views(&mut self) -> Vec<&mut dyn ViewHolder> {
        self.files.iter_mut().map(|f| f as &mut dyn ViewHolder).collect()
    }
}

impl RecursiveViewContainer for Diff {

    fn get_own_view(&mut self, _line_no: i32) -> &mut View {
        // never used
        self.files[0].view.as_mut().unwrap()
    }
    
    fn get_children_views(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        self.files.iter_mut().map(|f| f as &mut dyn RecursiveViewContainer).collect()
    }
}

impl ViewsContainer for File {
    fn get_views(&mut self) -> Vec<&mut dyn ViewHolder> {
        self.hunks.iter_mut().map(|vh|vh as &mut dyn ViewHolder).collect()
    }
}

impl RecursiveViewContainer for File {
    fn get_own_view(&mut self, line_no: i32) -> &mut View {
        self.view.get_or_insert_with(|| {
            View::new(line_no, false, self.path.to_str().unwrap().to_string())
        })
    }
    fn get_children_views(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        self.hunks.iter_mut().map(|vh|vh as &mut dyn RecursiveViewContainer).collect()
    }
}


impl ViewsContainer for Hunk {
    fn get_views(&mut self) -> Vec<&mut dyn ViewHolder> {
        self.lines.iter_mut().map(|vh|vh as &mut dyn ViewHolder).collect()
    }
}

impl RecursiveViewContainer for Hunk {
    fn get_own_view(&mut self, line_no: i32) -> &mut View {
        let header = &self.header;
        self.view.get_or_insert_with(|| {
            View::new(line_no, true, header.to_string())
        })
    }
    fn get_children_views(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        self.lines.iter_mut().map(|vh|vh as &mut dyn RecursiveViewContainer).collect()
    }
}


impl ViewHolder for Line {
    fn get_view(&mut self, line_no: i32) -> &mut View {
        let content = &self.content;
        self.view.get_or_insert_with(|| {
            View::new(line_no, false, content.to_string())
        })
    }
}

impl RecursiveViewContainer for Line {
    fn get_own_view(&mut self, line_no: i32) -> &mut View {
        let content = &self.content;
        self.view.get_or_insert_with(|| {
            View::new(line_no, false, content.to_string())
        })
    }
    fn get_children_views(&mut self) -> Vec<&mut dyn RecursiveViewContainer> {
        return Vec::new()
    }
}

impl ViewHolder for Hunk {
    fn get_view(&mut self, line_no: i32) -> &mut View {
        let header = &self.header;
        self.view.get_or_insert_with(|| {
            View::new(line_no, true, header.to_string())
        })
    }
}

impl ViewHolder for File {
    fn get_view(&mut self, line_no: i32) -> &mut View {
        self.view.get_or_insert_with(|| {
            View::new(line_no, false, self.path.to_str().unwrap().to_string())
        })
    }
}


pub fn render(view: &TextView, diff: &mut Diff) {
    let buffer = view.buffer();
    let mut iter = buffer.iter_at_offset(0);

    for vh in diff.get_views() {

        let view = vh.get_view(iter.line()).render(&buffer, &mut iter);
        if !view.expanded {
            continue
        }
    }
    // for file in &mut diff.files  {

    //     let view = file.get_view(iter.line()).render(&buffer, &mut iter);
    //     if !view.expanded {
    //         continue
    //     }


    //     for hunk in &mut file.hunks {
    //         let view = hunk.get_view(iter.line()).render(&buffer, &mut iter);
    //         if !view.expanded {
    //             continue
    //         }


    //         for line in &mut hunk.lines {
    //             match line.kind {
    //                 crate::LineKind::File => continue,
    //                 crate::LineKind::Hunk => continue,
    //                 _ => ()
    //             }
    //             line.get_view(iter.line()).render(&buffer, &mut iter);
    //         }
    //     }
    // }

    buffer.delete(&mut iter, &mut buffer.end_iter());

    // TODO! place cursor properly
    iter.set_offset(diff.offset);
    buffer.place_cursor(&iter);
    highlight_if_need(&view, iter);
}
