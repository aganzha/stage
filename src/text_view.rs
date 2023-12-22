use gtk::prelude::*;
use gtk::{glib, gdk, TextView, TextBuffer, TextTag};// TextIter
use glib::{Sender, subclass::Signal, subclass::signal::SignalId, value::Value};
use crate::{View, Diff};

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
                    sndr.send(crate::Event::Expand(buffer.iter_at_offset(buffer.cursor_position()).line()))
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

impl View {
    fn new(line_no: i32) -> Self {
        return View {
            line_no: line_no,
            expanded: false
        }
    }
}

impl Diff {
    pub fn set_expand(&mut self, line_no: i32) {
        println!("expand {:?}", line_no);
        self.tmp = line_no;
    }
}

pub fn render(view: &TextView, diff: &mut Diff, _sndr: Sender<crate::Event>) { // , signal: SignalId
    let buffer = view.buffer();
    let mut iter = buffer.iter_at_offset(0);
    for file in &mut diff.files  {
        if file.view.is_none() {
            file.view.replace(View::new(iter.line()));
        }
        buffer.insert(&mut iter, file.path.to_str().unwrap());
        buffer.insert(&mut iter, "\n");
        // if !file.view.unwrap().expanded {
        //     continue
        // }
        for hunk in &mut file.hunks {
            if hunk.view.is_none() {
                hunk.view.replace(View::new(iter.line()));
            }
            // if !hunk.view.unwrap().expanded {
            //     continue
            // }
            buffer.insert(&mut iter, &hunk.header);
            for line in &mut hunk.lines {
                match line.kind {
                    crate::LineKind::File => continue,
                    crate::LineKind::Hunk => continue,
                    _ => ()
                }
                line.view.replace(View::new(iter.line()));
                buffer.insert(&mut iter, &line.content);
            }
        }
    }
    iter.set_line(0);
    iter.set_line_offset(0);
    buffer.place_cursor(&iter);
}
