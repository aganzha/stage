use crate::status_view::context::{StatusRenderContext, TextViewWidth};
use crate::status_view::headerbar::{Scheme, SCHEME_TOKEN};
use crate::status_view::tags;
use async_channel::Sender;
use core::time::Duration;
use gdk::Display;
use glib::ControlFlow;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, Accessible, Buildable, EventControllerKey,
    EventControllerMotion, EventSequenceState, GestureClick, MovementStep,
    Settings, TextBuffer, TextTag, TextView, TextWindowType, Widget,
};
use libadwaita::prelude::*;
use libadwaita::StyleManager;
use log::{debug, trace};
use std::array::from_fn;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

glib::wrapper! {
    pub struct StageView(ObjectSubclass<stage_view::StageView>)
        @extends TextView, Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

mod stage_view {
    use glib::Properties;
    use gtk4::prelude::*;
    use gtk4::{
        gdk,
        glib,
        graphene,
        //MovementStep//, DeleteType, TextIter, TextExtendSelection,
        Snapshot,
        TextView,
        TextViewLayer,
    };
    use std::cell::{Cell, RefCell};

    use gtk4::subclass::prelude::*;
    use log::{debug, trace};

    // #[derive(Properties, Default)]
    // #[properties(wrapper_type = super::StageView)]

    #[derive(Default)]
    pub struct StageView {
        pub cursor: Cell<(i32, i32)>,
        pub active_lines: Cell<(i32, i32)>,
        pub hunks: RefCell<Vec<(i32, i32)>>,
        // TODO - update on event!
        pub known_line_height: Cell<i32>,
        // TODO! put it here!
        pub is_dark: bool,
        // #[property(get, set)]
        // pub current_line: RefCell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StageView {
        const NAME: &'static str = "StageView";
        type Type = super::StageView;
        type ParentType = TextView;
    }

    impl StageView {}

    impl TextViewImpl for StageView {
        fn snapshot_layer(&self, layer: TextViewLayer, snapshot: Snapshot) {
            if layer == TextViewLayer::BelowText {
                // this is hack. for some reason line_yrange not always
                // returns height of line :(
                // let mut known_line_height: i32 = 0;

                // highlight active_lines ----------------------------
                let (y_from, y_to) = self.active_lines.get();
                // HARCODE - 2000. color #f6f5f4/494949 - 246/255 245/255 244/255
                snapshot.append_color(
                    &gdk::RGBA::new(0.961, 0.961, 0.957, 1.0),
                    &graphene::Rect::new(
                        0.0,
                        y_from as f32,
                        2000.0,
                        y_to as f32,
                    ),
                );

                // highlight hunks -----------------------------------
                // HARCODE - 2000; #deddda/383838 - 221/255 221/255 218/255
                for (y_from, y_to) in self.hunks.borrow().iter() {
                    snapshot.append_color(
                        &gdk::RGBA::new(0.871, 0.871, 0.855, 1.0),
                        &graphene::Rect::new(
                            0.0,
                            *y_from as f32,
                            2000.0,
                            *y_to as f32,
                        ),
                    );
                }

                // highlight cursor ---------------------------------
                let (y_from, y_to) = self.cursor.get();
                // HARCODE - 2000; #cce0f8/23374f - 204/255 224/255 248/255
                snapshot.append_color(
                    &gdk::RGBA::new(0.80, 0.878, 0.972, 1.0),
                    &graphene::Rect::new(
                        0.0,
                        y_from as f32,
                        2000.0,
                        y_to as f32,
                    ),
                );
            }
            self.parent_snapshot_layer(layer, snapshot)
        }

        // fn toggle_overwrite(&self) {
        //     self.parent_toggle_overwrite()
        // }
    }
    impl ObjectImpl for StageView {}
    impl WidgetImpl for StageView {}
}

impl StageView {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn get_highliught_cursor(&self) -> ((i32, i32), i32) {
        (self.imp().cursor.get(), self.imp().cursor.get().0 / 34)
    }

    pub fn highlight_cursor(&self, line_no: i32) {
        if let Some(iter) = self.buffer().iter_at_line(line_no) {
            let (y, mut height) = self.line_yrange(&iter);
            // this is a hack. for some reason line_yrange returns wrong height :(
            let known_line_height = self.imp().known_line_height.get();
            if known_line_height == 0 {
                self.imp().known_line_height.replace(height);
            } else {
                if height > known_line_height {
                    height = known_line_height;
                }
            }
            self.imp().cursor.replace((y, height));
            trace!(
                "real highligh cursor line_no {}, y {}, height {}",
                line_no,
                y,
                height
            );
        } else {
            trace!("trying to highlight cursor BUT NO LINE HERE {}", line_no);
        }
    }

    pub fn highlight_lines(&self, from_to: (i32, i32)) {
        // this is hack, because line_yrange just after
        // rendering line returns wrong pixes coords and height!
        // see timeout value - it is just 1 msec!
        if let Some(iter) = self.buffer().iter_at_line(from_to.0) {
            let range = self.line_yrange(&iter);
            trace!(
                "highlight_lines in textview .............. {:?} {:?}",
                from_to,
                range
            );
            self.imp()
                .active_lines
                .replace((range.0, range.1 * (from_to.1 - from_to.0 + 1)));
        }
    }

    pub fn reset_highlight_lines(&self) {
        self.imp().active_lines.replace((0, 0));
    }

    pub fn has_highlight_lines(&self) -> bool {
        let (from, to) = self.imp().active_lines.get();
        return from > 0 || to > 0;
    }

    pub fn set_highlight_hunks(&self, hunks: &Vec<i32>) {
        if hunks.is_empty() {
            return;
        }
        let buffer = self.buffer();
        self.imp().hunks.replace(
            hunks
                .iter()
                .filter_map(|h| {
                    if let Some(iter) = buffer.iter_at_line(*h) {
                        let (y, mut height) = self.line_yrange(&iter);
                        let known_line_height = self.imp().known_line_height.get();
                        if known_line_height > 0 && known_line_height < height {
                            height = known_line_height;
                        }
                        return Some((y, height));
                    }
                    None
                })
                .collect(),
        );
    }

    pub fn reset_highlight_hunks(&self) {
        self.imp().hunks.replace(Vec::new());
    }

    pub fn bind_highlights(&self, context: &StatusRenderContext) {
        self.highlight_cursor(context.highlight_cursor);
        if let Some(lines) = context.highlight_lines {
            self.highlight_lines(lines);
        } else {
            self.reset_highlight_lines();
        }

        if !context.highlight_hunks.is_empty() {
            self.set_highlight_hunks(&context.highlight_hunks);
        } else {
            self.reset_highlight_hunks()
        }
        // glib::source::timeout_add_local(
        //     Duration::from_millis(1),
        //     {
        //         let txt = self.clone();
        //         let ctx = context.clone();
        //         move || {
        //             txt.bind_highlights(&ctx);
        //             glib::ControlFlow::Break
        //         }
        //     });
    }
}

// TODO - kill it all
pub trait CharView {
    fn calc_max_char_width(&self) -> i32;
}

impl CharView for TextView {
    fn calc_max_char_width(&self) -> i32 {
        let buffer = self.buffer();
        let mut iter = buffer.iter_at_offset(0);
        let x_before = self.cursor_locations(Some(&iter)).0.x();
        let forwarded = iter.forward_char();
        if !forwarded {
            buffer.insert(&mut iter, " ");
        };
        let x_after = self.cursor_locations(Some(&iter)).0.x();

        self.width() / (x_after - x_before)
    }
}

impl CharView for StageView {
    fn calc_max_char_width(&self) -> i32 {
        let buffer = self.buffer();
        let mut iter = buffer.iter_at_offset(0);
        let x_before = self.cursor_locations(Some(&iter)).0.x();
        let forwarded = iter.forward_char();
        if !forwarded {
            buffer.insert(&mut iter, " ");
        };
        let x_after = self.cursor_locations(Some(&iter)).0.x();

        self.width() / (x_after - x_before)
    }
}

pub const DARK_CLASS: &str = "dark";
pub const LIGHT_CLASS: &str = "light";

pub fn factory(
    sndr: Sender<crate::Event>,
    name: &str,
    //settings: gio::Settings,
    text_view_width: Rc<RefCell<TextViewWidth>>,
) -> StageView {
    let manager = StyleManager::default();
    let is_dark = manager.is_dark();

    // let txt = TextView::builder()
    //     .margin_start(12)
    //     .name(name)
    //     .css_classes(if is_dark { [DARK_CLASS] } else { [LIGHT_CLASS] })
    //     .margin_end(12)
    //     .margin_top(12)
    //     .margin_bottom(12)
    //     .build();
    let txt = StageView::new();
    txt.set_margin_start(12);
    txt.set_widget_name(name);
    txt.set_margin_end(12);
    txt.set_margin_top(12);
    txt.set_margin_bottom(12);
    if is_dark {
        txt.set_css_classes(&[&DARK_CLASS]);
    } else {
        txt.set_css_classes(&[&LIGHT_CLASS]);
    }

    let buffer = txt.buffer();
    let table = buffer.tag_table();
    let mut pointer: Option<TextTag> = None;
    let mut staged: Option<TextTag> = None;
    let mut unstaged: Option<TextTag> = None;

    //let scheme = Scheme::new(settings.get::<String>(SCHEME_TOKEN));

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

    manager.connect_color_scheme_notify({
        let txt = txt.clone();
        move |manager| {
            let is_dark = manager.is_dark();
            let classes = txt.css_classes();
            let mut new_classes = classes
                .iter()
                .map(|gs| gs.as_str())
                .filter(|s| {
                    if is_dark {
                        s != &LIGHT_CLASS
                    } else {
                        s != &DARK_CLASS
                    }
                })
                .collect::<Vec<&str>>();
            if is_dark {
                new_classes.push(&DARK_CLASS);
            } else {
                new_classes.push(&LIGHT_CLASS);
            }
            txt.set_css_classes(&new_classes);
            table.foreach(|tt| {
                if let Some(name) = tt.name() {
                    let t = tags::TxtTag::unknown_tag(name.to_string());
                    t.fill_text_tag(&tt, is_dark);
                }
            });
        }
    });
    let pointer = pointer.unwrap();
    let staged = staged.unwrap();
    let unstaged = unstaged.unwrap();

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
                (gdk::Key::e, _) => {
                    sndr.send_blocking(crate::Event::OpenEditor)
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
                                trace!("PERFORM REAL CLICK {:?}", n_clicks);
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
        move |view: &StageView, step, count, _selection| {
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
                // resizing window. handle both cases: initial render and further resizing
                text_view_width.borrow_mut().pixels = width;
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
                        trace!(
                            "text_view_width is changed! {:?} {:?}",
                            text_view_width,
                            visible_char_width
                        );
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

    txt.set_monospace(true);
    txt.set_editable(false);
    // let sett = txt.settings();
    // sett.set_gtk_cursor_blink(true);
    // sett.set_gtk_cursor_blink_time(3000);
    // sett.set_gtk_cursor_aspect_ratio(0.05);
    txt
}

pub fn cursor_to_line_offset(buffer: &TextBuffer, line_offset: i32) {
    let mut iter = buffer.iter_at_offset(buffer.cursor_position());
    iter.backward_line();
    iter.forward_lines(1);
    iter.forward_chars(line_offset);
    buffer.place_cursor(&iter);
}
