use std::cell::RefCell;
use std::rc::Rc;

use crate::status_view::Tag;
use async_channel::Sender;
use core::time::Duration;
use glib::ControlFlow;
use gtk4::prelude::*;
use gtk4::{
    gdk, glib, EventControllerKey, EventSequenceState, GestureClick,
    MovementStep, TextIter, TextView, TextWindowType,
};
use log::{debug, trace};

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

pub fn text_view_factory(
    sndr: Sender<crate::Event>,
    text_view_width: Rc<RefCell<(i32, i32)>>,
) -> TextView {
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
    buffer.tag_table().add(&Tag::Hunk.create());
    
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
                (gdk::Key::k, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Kill(
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
                    // let loffset = start_iter.line_offset();
                    start_iter.forward_lines(count);
                    // handle_line_offset(
                    //     &mut start_iter,
                    //     loffset,
                    //     &latest_char_offset,
                    // );
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

    let calc_max_char_width = |view: &TextView, width: i32| -> Option<i32> {
        if let Some((mut iter, _over_text)) = view.iter_at_position(1, 1) {
            let buff = iter.buffer();
            iter.forward_to_line_end();
            let mut pos = view.cursor_locations(Some(&iter)).0.x();
            while pos < width {
                buff.insert(&mut iter, " ");
                pos = view.cursor_locations(Some(&iter)).0.x();
            }
            return Some(iter.offset());
        }
        None
    };

    txt.add_tick_callback({
        move |view, _clock| {
            let width = view.width();
            let stored_width = (*text_view_width.borrow()).0;
            if width > 0 && width != stored_width {
                // resizing window. handle both cases: initial render and further resizing
                text_view_width.replace((width, 0));
                if stored_width == 0 {
                    // initial render
                    if let Some(char_width) = calc_max_char_width(&view, width) {
                        text_view_width.replace((width, char_width));
                    }
                } else {
                    // resizing window by user action
                    // do need to calc char width every time (perhaps changing window by dragging)
                    // only do it once after 30 mills of LAST resize signal
                    // 30 - magic number. 20 is not enough.
                    glib::source::timeout_add_local(Duration::from_millis(30), {
                        let text_view_width = text_view_width.clone();
                        let view = view.clone();
                        let sndr = sndr.clone();
                        move || {
                            if width == text_view_width.borrow().0 {
                                if let Some(char_width) = calc_max_char_width(&view, width) {
                                    trace!("text view char width IN resize {:?} {:?}", text_view_width, char_width);
                                    text_view_width.replace((width, char_width));
                                    sndr.send_blocking(crate::Event::TextViewResize).expect("could not sent through channel");
                                }
                            }
                            ControlFlow::Break
                        }});
                }
            }
            ControlFlow::Continue
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
