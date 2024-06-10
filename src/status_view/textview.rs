use crate::status_view::tags;
use async_channel::Sender;
use core::time::Duration;
use glib::ControlFlow;
use gtk4::prelude::*;
use gtk4::{
    gdk, glib, pango, EventControllerKey, EventControllerMotion,
    EventSequenceState, GestureClick, MovementStep, TextIter, TextTag,
    TextView, TextWindowType,
};
use log::{debug, trace};
use pango::Style;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

//const CURSOR_TAG: &str = "CursorTag";

// gnome colors https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/css-variables.html
// #[derive(Eq, Hash, PartialEq, Debug)]
// pub enum Tag {
//     Bold,
//     Added,
//     EnhancedAdded,
//     Removed,
//     EnhancedRemoved,
//     Cursor,
//     Region,
//     Hunk,
//     Italic,
//     Pointer,
//     Staged,
//     Unstaged,
//     ConflictMarker,
//     Ours,
//     Theirs, // Link
// }
// impl Tag {
//     pub fn create(&self) -> TextTag {
//         match self {
//             Self::Bold => {
//                 let tt = self.new_tag();
//                 tt.set_weight(700);
//                 tt
//             }
//             Self::Added => {
//                 let tt = self.new_tag();
//                 tt.set_foreground(Some("#2ec27e")); // background #ebfcf1
//                 tt
//             }
//             Self::EnhancedAdded => {
//                 let tt = self.new_tag();
//                 tt.set_foreground(Some("#26a269")); // background #d3fae1. gnome green #26a269
//                 tt
//             }
//             Self::Removed => {
//                 let tt = self.new_tag();
//                 tt.set_foreground(Some("#c01c28")); // background fbf0f3
//                 tt
//             }
//             Self::EnhancedRemoved => {
//                 let tt = self.new_tag();
//                 tt.set_foreground(Some("#a51d2d")); // background #f4c3d0
//                 tt
//             }
//             Self::Cursor => {
//                 let tt = self.new_tag();
//                 tt.set_background(Some("#f6fecd")); // f6fecd mine original. f9f06b - gnome
//                 tt
//             }
//             Self::Region => {
//                 let tt = self.new_tag();
//                 tt.set_background(Some("#f6f5f4")); // f2f2f2 mine original
//                 tt
//             }
//             Self::Hunk => {
//                 let tt = self.new_tag();
//                 tt.set_background(Some("#deddda"));
//                 tt
//             }
//             Self::Italic => {
//                 let tt = self.new_tag();
//                 tt.set_style(Style::Italic);
//                 tt
//             }
//             Self::ConflictMarker => {
//                 let tt = self.new_tag();
//                 tt.set_foreground(Some("#ff0000")); //  orange
//                 tt
//             }
//             Self::Theirs => {
//                 let tt = self.new_tag();
//                 tt.set_foreground(Some("#2ec27e")); // "#813d9c" magenta
//                 tt
//             }
//             Self::Ours => {
//                 let tt = self.new_tag();
//                 tt.set_foreground(Some("#2ec27e")); // "#1a5fb4" blue
//                 tt
//             }

//             // Self::Link => {
//             //     let tt = self.new_tag();
//             //     tt.set_background(Some("0000ff"));
//             //     tt.set_style(Style::Underlined);
//             //     tt
//             // }
//             _ => {
//                 // all tags without attrs
//                 self.new_tag()
//             }
//         }
//     }
//     pub fn new_tag(&self) -> TextTag {
//         TextTag::new(Some(self.name()))
//     }
//     pub fn name(&self) -> &str {
//         match self {
//             Self::Bold => "bold",
//             Self::Added => "added",
//             Self::EnhancedAdded => "enhancedAdded",
//             Self::Removed => "removed",
//             Self::EnhancedRemoved => "enhancedRemoved",
//             Self::Cursor => CURSOR_TAG,
//             Self::Region => "region",
//             Self::Hunk => "hunk",
//             Self::Italic => "italic",
//             Self::Pointer => "pointer",
//             Self::Staged => "staged",
//             Self::Unstaged => "unstaged",
//             Self::ConflictMarker => "conflictmarker",
//             Self::Ours => "ours",
//             Self::Theirs => "theirs",
//         }
//     }
//     pub fn enhance(&self) -> &Self {
//         match self {
//             Self::Added => &Self::EnhancedAdded,
//             Self::Removed => &Self::EnhancedRemoved,
//             other => other,
//         }
//     }
// }

pub trait CharView {
    fn calc_max_char_width(&self) -> i32;
}

impl CharView for TextView {
    fn calc_max_char_width(&self) -> i32 {
        let buffer = self.buffer();
        let mut iter = buffer.iter_at_offset(0);
        let mut pos = self.cursor_locations(Some(&iter)).0.x();
        let mut strip_index = 0;
        while pos < self.width() {
            let forwarded = iter.forward_char();
            if !forwarded {
                strip_index = iter.offset();
                buffer.insert(&mut iter, " ");
            }
            pos = self.cursor_locations(Some(&iter)).0.x();
        }
        if strip_index > 0 {
            // do it need to cleanup line to the end?
            // perhaps not
        }
        iter.offset()
    }
}

pub fn factory(
    sndr: Sender<crate::Event>,
    name: &str,
    text_view_width: Rc<RefCell<crate::context::TextViewWidth>>,
) -> TextView {
    let txt = TextView::builder()
        .margin_start(12)
        .name(name)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();
    let buffer = txt.buffer();
    let table = buffer.tag_table();
    let mut pointer: Option<TextTag> = None;
    let mut staged: Option<TextTag> = None;
    let mut unstaged: Option<TextTag> = None;
    
    for tag_name in tags::TEXT_TAGS {
        let text_tag = tags::TxtTag::from_str(tag_name).create();
        table.add(&text_tag);
        match tag_name {
            tags::POINTER => {
                pointer.replace(text_tag);
            }
            tags::STAGED => {
                staged.replace(text_tag);
            }
            tags::UNSTAGED => {
                unstaged.replace(text_tag);
            }
            _ => {}
        };
    }
    let pointer = pointer.unwrap();
    let staged = staged.unwrap();
    let unstaged = unstaged.unwrap();
    // buffer.tag_table().add(&pointer);
    // buffer.tag_table().add(&Tag::Cursor.create());
    // buffer.tag_table().add(&Tag::Region.create());
    // buffer.tag_table().add(&Tag::Bold.create());
    // buffer.tag_table().add(&Tag::Added.create());
    // buffer.tag_table().add(&Tag::EnhancedAdded.create());
    // buffer.tag_table().add(&Tag::Removed.create());
    // buffer.tag_table().add(&Tag::EnhancedRemoved.create());
    // buffer.tag_table().add(&Tag::Hunk.create());
    // buffer.tag_table().add(&staged);
    // buffer.tag_table().add(&unstaged);

    // buffer.tag_table().add(&Tag::ConflictMarker.create());
    // buffer.tag_table().add(&Tag::Theirs.create());
    // buffer.tag_table().add(&Tag::Ours.create());

    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed({
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
                    return glib::Propagation::Stop;
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
                (gdk::Key::i, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Ignore(
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
                (gdk::Key::f, _) => {
                    sndr.send_blocking(crate::Event::Pull)
                        .expect("Could not send through channel");
                }
                (gdk::Key::b, _) => {
                    sndr.send_blocking(crate::Event::Branches)
                        .expect("Could not send through channel");
                }
                (gdk::Key::l, _) => {
                    sndr.send_blocking(crate::Event::Log(None, None))
                        .expect("Could not send through channel");
                }
                (gdk::Key::g, _) => {
                    sndr.send_blocking(crate::Event::Refresh)
                        .expect("Could not send through channel");
                }
                (gdk::Key::o, gdk::ModifierType::CONTROL_MASK) => {
                    sndr.send_blocking(crate::Event::RepoOpen)
                        .expect("Could not send through channel");
                }
                (gdk::Key::r, _) => {
                    sndr.send_blocking(crate::Event::RepoPopup)
                        .expect("Could not send through channel");
                }
                (gdk::Key::z, _) => {
                    sndr.send_blocking(crate::Event::StashesPanel)
                        .expect("cant send through channel");
                }
                (gdk::Key::d, _) => {
                    let _iter =
                        buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Debug)
                        .expect("Could not send through channel");
                }
                (gdk::Key::equal, gdk::ModifierType::CONTROL_MASK) => {
                    sndr.send_blocking(crate::Event::Zoom(true))
                        .expect("Could not send through channel");
                }
                (gdk::Key::minus, gdk::ModifierType::CONTROL_MASK) => {
                    sndr.send_blocking(crate::Event::Zoom(false))
                        .expect("Could not send through channel");
                }
                (key, modifier) => {
                    trace!(
                        "key press in status view {:?} {:?}",
                        key.name(),
                        modifier
                    );
                }
            }
            glib::Propagation::Proceed
        }
    });
    txt.add_controller(key_controller);

    let num_clicks = Rc::new(Cell::new(0));

    let gesture_controller = GestureClick::new();
    gesture_controller.connect_released({
        let sndr = sndr.clone();
        let txt = txt.clone();
        let pointer = pointer.clone();
        move |gesture, n_clicks, wx, wy| {
            gesture.set_state(EventSequenceState::Claimed);
            let (x, y) = txt.window_to_buffer_coords(
                TextWindowType::Text,
                wx as i32,
                wy as i32,
            );
            if let Some(iter) = txt.iter_at_location(x, y) {
                let pos = iter.offset();
                let has_pointer = iter.has_tag(&pointer);
                sndr.send_blocking(crate::Event::Cursor(
                    iter.offset(),
                    iter.line(),
                )).expect("Could not send through channel");
                if has_pointer {
                    num_clicks.replace(n_clicks);
                    glib::source::timeout_add_local(Duration::from_millis(200), {
                        let num_clicks = num_clicks.clone();
                        let staged = staged.clone();
                        let unstaged = unstaged.clone();
                        let sndr = sndr.clone();
                        let txt = txt.clone();
                        move || {
                            if num_clicks.get() == n_clicks {
                                let iter = txt.buffer().iter_at_offset(pos);
                                match n_clicks {
                                    1 => {
                                        sndr.send_blocking(crate::Event::Expand(
                                            iter.offset(),
                                            iter.line(),
                                        )).expect("Could not send through channel");
                                    },
                                    2 => {
                                        if iter.has_tag(&staged) {
                                            sndr.send_blocking(crate::Event::UnStage(
                                                iter.offset(),
                                                iter.line(),
                                            )).expect("Could not send through channel");
                                        }
                                        if iter.has_tag(&unstaged) {
                                            sndr.send_blocking(crate::Event::Stage(
                                                iter.offset(),
                                                iter.line(),
                                            )).expect("Could not send through channel");
                                        }

                                    },
                                    _ => {
                                    }
                                }
                                debug!("PERFORM REAL CLICK {:?}", n_clicks);
                            }
                            ControlFlow::Break
                        }
                    });

                }
            }
        }
    });

    txt.add_controller(gesture_controller);

    txt.connect_move_cursor({
        let sndr = sndr.clone();
        // let latest_char_offset = RefCell::new(0);
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
                    start_iter.forward_lines(count);
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
            } //  else {
              //     let mut cnt = latest_char_offset.borrow_mut();
              //     *cnt = 0;
              // }
        }
    });

    txt.add_tick_callback({
        move |view, _clock| {
            let width = view.width();
            let stored_width = text_view_width.borrow().pixels;
            if width > 0 && width != stored_width {
                // debug!("once for view! text view width in pixes. real{:?} vs stored {:?}", width, stored_width);
                // resizing window. handle both cases: initial render and further resizing
                text_view_width.borrow_mut().pixels = width;
                // debug!("replaced screen width {:?}", text_view_width);
                if stored_width == 0 {
                    // initial render
                    let visible_char_width = view.calc_max_char_width();
                    text_view_width.borrow_mut().visible_chars =
                        visible_char_width;
                    sndr.send_blocking(crate::Event::TextCharVisibleWidth(
                        visible_char_width,
                    ))
                    .expect("could not sent through channel");
                    if visible_char_width > text_view_width.borrow().chars {
                        text_view_width.borrow_mut().chars =
                            visible_char_width;
                        sndr.send_blocking(crate::Event::TextViewResize(
                            visible_char_width,
                        ))
                        .expect("could not sent through channel");
                    }
                } else {
                    // resizing window by user action
                    // do need to calc char width every time (perhaps changing window by dragging)
                    // only do it once after 30 mills of LAST resize signal
                    // 30 - magic number. 20 is not enough.
                    glib::source::timeout_add_local(
                        Duration::from_millis(30),
                        {
                            let text_view_width = text_view_width.clone();
                            let view = view.clone();
                            let sndr = sndr.clone();
                            move || {
                                if width == text_view_width.borrow().pixels {
                                    let visible_char_width =
                                        view.calc_max_char_width();
                                    if visible_char_width
                                        > text_view_width.borrow().chars
                                    {
                                        text_view_width.borrow_mut().chars =
                                            visible_char_width;
                                        sndr.send_blocking(
                                            crate::Event::TextViewResize(
                                                visible_char_width,
                                            ),
                                        )
                                        .expect(
                                            "could not sent through channel",
                                        );
                                    }
                                }
                                ControlFlow::Break
                            }
                        },
                    );
                }
            }
            ControlFlow::Continue
        }
    });

    let motion_controller = EventControllerMotion::new();
    motion_controller.connect_motion({
        let txt = txt.clone();
        move |_c, x, y| {
            let (x, y) = txt.window_to_buffer_coords(
                TextWindowType::Text,
                x as i32,
                y as i32,
            );
            if let Some(iter) = txt.iter_at_location(x, y) {
                if iter.has_tag(&pointer) {
                    txt.set_cursor(Some(
                        &gdk::Cursor::from_name("pointer", None).unwrap(),
                    ));
                } else {
                    txt.set_cursor(Some(
                        &gdk::Cursor::from_name("text", None).unwrap(),
                    ));
                }
            }
        }
    });
    txt.add_controller(motion_controller);

    txt.add_css_class("stage");
    txt.set_monospace(true);
    txt.set_editable(false);
    // let sett = txt.settings();
    // sett.set_gtk_cursor_blink(true);
    // sett.set_gtk_cursor_blink_time(3000);
    // sett.set_gtk_cursor_aspect_ratio(0.05);
    txt
}
