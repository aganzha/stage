use gtk::prelude::*;
use gtk::{glib, gdk, TextView, TextBuffer, TextTag};// TextIter

const HIGHLIGHT: &str = "highlight";
const HIGHLIGHT_START: &str  = "HightlightStart";
const HIGHLIGHT_END: &str = "HightlightEnd";

pub fn text_view_factory() ->  TextView {
    let txt = TextView::builder()
        .build();
    let buffer = txt.buffer();
    let tag = TextTag::new(Some(HIGHLIGHT));
    
    let event_controller = gtk::EventControllerKey::new();
    event_controller.connect_key_pressed({
        |_, key, _, _| {
            match key {
                gdk::Key::Tab => {
                    println!("taaaaaaaaaaaaaaaaaaaaaaaaaaaaaab!")
                },
                gdk::Key::s => {
                    println!("ssssssssssssssssssssssssssssssssssss!")
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
            let buffer = txt.buffer();
            let maybe_iter = txt.iter_at_location(x, y);
            if maybe_iter.is_none() {
                return;
            }
            highlight_if_need(buffer, maybe_iter.unwrap());
            println!("Box pressed! {:?} {:?} {:?} {:?}", wx, wy, x, y);
        }
    });
    
    txt.add_controller(gesture);



    tag.set_background(Some("#f6fecd"));

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
            highlight_if_need(buffer, start_iter);
        }
    });
    
    let lorem: &str = "Untracked files (1)
src/style.css

Recent commits
a2959bf master origin/master begin textview
7601dc7 added adwaita
c622f7f init";

    
    buffer.set_text(&lorem);

    buffer.tag_table().add(&tag);
    txt.set_monospace(true);
    txt.set_editable(false);

    buffer.place_cursor(&buffer.iter_at_offset(0));


    let start_iter = buffer.iter_at_offset(0);
    buffer.create_mark(Some(HIGHLIGHT_START), &start_iter, false);
        
    let mut end_iter = buffer.iter_at_offset(0);
    end_iter.forward_to_line_end();
    buffer.create_mark(Some(HIGHLIGHT_END), &end_iter, false);    
    
    buffer.apply_tag(&tag, &start_iter, &end_iter);
    println!("ADDED TAG {:?}", buffer.slice(&start_iter, &end_iter, true), );
    txt
}

pub fn highlight_if_need(buffer: gtk::TextBuffer,
                         mut start_iter: gtk::TextIter) {
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
    let mut end_iter = buffer.iter_at_offset(start_iter.offset());
    end_iter.forward_to_line_end();

    buffer.move_mark(&start_mark, &start_iter);
    buffer.move_mark(&end_mark, &end_iter);
    buffer.apply_tag_by_name(HIGHLIGHT, &start_iter, &end_iter);    
}
