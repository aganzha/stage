// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later


use crate::status_view::context::StatusRenderContext;

use crate::status_view::tags;
use async_channel::Sender;
use core::time::Duration;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, glib, EventControllerKey, EventControllerMotion, EventSequenceState,
    GestureClick, GestureDrag, MovementStep, TextBuffer, TextTag, TextView,
    TextWindowType, Widget,
};
use libadwaita::StyleManager;
use log::{debug, trace};

use std::cell::RefCell;
use std::rc::Rc;

glib::wrapper! {
    pub struct StageView(ObjectSubclass<stage_view::StageView>)
        @extends TextView, Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

mod stage_view {

    use gtk4::prelude::*;
    use gtk4::{gdk, glib, graphene, Snapshot, TextView, TextViewLayer};
    use std::cell::{Cell, RefCell};

    use gtk4::subclass::prelude::*;

    // #cce0f8/23374f - 204/255 224/255 248/255  35 55 79
    const LIGHT_CURSOR: gdk::RGBA = gdk::RGBA::new(0.80, 0.878, 0.972, 1.0);
    const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.137, 0.216, 0.310, 1.0);

    const DARK_BF_FILL: gdk::RGBA = gdk::RGBA::new(0.0, 0.0, 0.0, 1.0);
    const LIGHT_BG_FILL: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
    // color #f6f5f4/494949 - 246/255 245/255 244/255
    // f3f3f3 - 243/255
    // f4f4f4 - 244/255
    // const LIGHT_LINES: gdk::RGBA = gdk::RGBA::new(0.961, 0.961, 0.957, 1.0);
    const LIGHT_LINES: gdk::RGBA = gdk::RGBA::new(0.957, 0.957, 0.957, 1.0);
    const DARK_LINES: gdk::RGBA = gdk::RGBA::new(0.286, 0.286, 0.286, 1.0);

    // #deddda/383838 - 221/255 221/255 218/255
    const LIGHT_HUNKS: gdk::RGBA = gdk::RGBA::new(0.871, 0.871, 0.855, 1.0);
    const DARK_HUNKS: gdk::RGBA = gdk::RGBA::new(0.22, 0.22, 0.22, 1.0);

    // #[derive(Properties, Default)]
    // #[properties(wrapper_type = super::StageView)]

    #[derive(Default)]
    pub struct StageView {
        pub cursor: Cell<i32>,
        pub show_cursor: Cell<bool>,
        pub double_height_line: Cell<bool>,
        pub active_lines: Cell<(i32, i32)>,
        pub hunks: RefCell<Vec<i32>>,

        pub known_line_height: Cell<i32>,

        // TODO! put it here!
        pub is_dark: Cell<bool>,
        pub is_dark_set: Cell<bool>,
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
                let rect = self.obj().visible_rect();
                let bg_fill = if self.is_dark.get() {
                    &DARK_BF_FILL
                } else {
                    &LIGHT_BG_FILL
                };
                snapshot.append_color(
                    bg_fill,
                    &graphene::Rect::new(
                        rect.x() as f32,
                        rect.y() as f32,
                        rect.width() as f32,
                        rect.height() as f32,
                    ),
                );

                let buffer = self.obj().buffer();
                let mut iter = buffer.iter_at_offset(0);
                let (line_from, line_to) = self.active_lines.get();

                if line_from > 0 && line_to > 0 {
                    iter.set_line(line_from);
                    let y_from = self.obj().line_yrange(&iter).0;
                    iter.set_line(line_to);
                    let (y, height) = self.obj().line_yrange(&iter);
                    let y_to = y + height;
                    // highlight active_lines ----------------------------
                    snapshot.append_color(
                        if self.is_dark.get() {
                            &DARK_LINES
                        } else {
                            &LIGHT_LINES
                        },
                        &graphene::Rect::new(
                            0.0,
                            y_from as f32,
                            rect.width() as f32,
                            y_to as f32,
                        ),
                    );
                    // there is a garbage from previous highlights
                    // or perhaps i am doing something. but this will
                    // cleanup the garbage
                    snapshot.append_color(
                        bg_fill,
                        &graphene::Rect::new(
                            rect.x() as f32,
                            y_to as f32,
                            rect.width() as f32,
                            rect.height() as f32,
                        ),
                    );
                }

                // highlight hunks -----------------------------------
                for line in self.hunks.borrow().iter() {
                    iter.set_line(*line);
                    let (y_from, y_to) = self.obj().line_yrange(&iter);
                    snapshot.append_color(
                        if self.is_dark.get() {
                            &DARK_HUNKS
                        } else {
                            &LIGHT_HUNKS
                        },
                        &graphene::Rect::new(
                            0.0,
                            y_from as f32,
                            rect.width() as f32,
                            y_to as f32,
                        ),
                    );
                }

                // highlight cursor ---------------------------------
                let cursor_line = self.cursor.get();
                iter.set_line(cursor_line);
                let (mut y_from, mut y_to) = self.obj().line_yrange(&iter);

                if self.double_height_line.get() {
                    // hack for diff labels
                    y_to /= 2;
                    y_from += y_to;
                } else {
                    // huck for broken highlight
                    // during different switches
                    let known = self.known_line_height.get();
                    if known > 0 && y_to > known {
                        y_to = known;
                    }
                }

                snapshot.append_color(
                    if self.show_cursor.get() {
                        if self.is_dark.get() {
                            &DARK_CURSOR
                        } else {
                            &LIGHT_CURSOR
                        }
                    } else {
                        bg_fill
                    },
                    &graphene::Rect::new(
                        0.0,
                        y_from as f32,
                        rect.width() as f32,
                        y_to as f32,
                    ),
                );
            }
        }
    }
    impl ObjectImpl for StageView {}
    impl WidgetImpl for StageView {}
}

impl Default for StageView {
    fn default() -> Self {
        Self::new()
    }
}

impl StageView {
    pub fn new() -> Self {
        let me: Self = glib::Object::builder().build();
        me.set_cursor_highlight(true);
        me
    }

    pub fn set_is_dark(&self, _is_dark: bool, force: bool) {
        if !force && self.imp().is_dark_set.get() {
            return;
        }
        let manager = StyleManager::default();
        let is_dark = manager.is_dark();
        self.imp().is_dark.replace(is_dark);
        self.imp().is_dark_set.replace(true);
    }

    pub fn set_cursor_highlight(&self, value: bool) {
        self.imp().show_cursor.replace(value);
    }

    pub fn bind_highlights(&self, context: &StatusRenderContext) {
        // here it need to pass pixels above line!
        self.imp().cursor.replace(context.cursor);
        // Diff labels have top margin with height of line.
        // it does not need to highlight them, only highlight
        // diff label itself
        self.imp()
            .double_height_line
            .replace(context.cursor_diff.is_some());
        if let Some(lines) = context.highlight_lines {
            self.imp().active_lines.replace(lines);
        } else {
            self.imp().active_lines.replace((0, 0));
        }
        self.imp().hunks.replace(Vec::new());
        for h in &context.highlight_hunks {
            self.imp().hunks.borrow_mut().push(*h);
        }
    }

    pub fn calc_max_char_width(&self) -> i32 {
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

pub fn factory(sndr: Sender<crate::Event>, name: &str) -> StageView {
    let manager = StyleManager::default();
    let is_dark = manager.is_dark();

    let txt = StageView::new();
    txt.set_margin_start(12);
    txt.set_widget_name(name);
    txt.set_margin_end(12);
    txt.set_margin_top(12);
    txt.set_margin_bottom(12);
    txt.set_is_dark(is_dark, true);
    if is_dark {
        txt.set_css_classes(&[DARK_CLASS]);
    } else {
        txt.set_css_classes(&[LIGHT_CLASS]);
    }

    let buffer = txt.buffer();
    let table = buffer.tag_table();
    let mut pointer: Option<TextTag> = None;
    let mut staged: Option<TextTag> = None;
    let mut unstaged: Option<TextTag> = None;
    let mut file: Option<TextTag> = None;
    let mut hunk: Option<TextTag> = None;
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
            tags::FILE => {
                file.replace(text_tag);
            }
            tags::HUNK => {
                hunk.replace(text_tag);
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
                new_classes.push(DARK_CLASS);
            } else {
                new_classes.push(LIGHT_CLASS);
            }
            txt.set_css_classes(&new_classes);
            table.foreach(|tt| {
                if let Some(name) = tt.name() {
                    let t = tags::TxtTag::unknown_tag(name.to_string());
                    t.fill_text_tag(tt, is_dark);
                }
            });
        }
    });
    let pointer = pointer.unwrap();
    let staged = staged.unwrap();
    let unstaged = unstaged.unwrap();
    let file = file.unwrap();
    let hunk = hunk.unwrap();

    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed({
        let buffer = buffer.clone();
        let sndr = sndr.clone();

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
                (
                    gdk::Key::s
                    | gdk::Key::a
                    | gdk::Key::ISO_Enter
                    | gdk::Key::KP_Enter,
                    _,
                ) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Stage(
                        crate::StageOp::Stage(iter.line()),
                    ))
                    .expect("Could not send through channel");
                }
                (gdk::Key::u, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Stage(
                        crate::StageOp::Unstage(iter.line()),
                    ))
                    .expect("Could not send through channel");
                }
                (gdk::Key::k, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Stage(
                        crate::StageOp::Kill(iter.line()),
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
                    sndr.send_blocking(crate::Event::ShowBranches)
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
                    sndr.send_blocking(crate::Event::OpenFileDialog)
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
                (gdk::Key::d, gdk::ModifierType::CONTROL_MASK) => {
                    let _iter =
                        buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Dump)
                        .expect("Could not send through channel");
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
                (gdk::Key::t, _) => {
                    sndr.send_blocking(crate::Event::Tags(None))
                        .expect("Could not send through channel");
                }
                (_, gdk::ModifierType::LOCK_MASK) => {
                    sndr.send_blocking(crate::Event::Toast(String::from(
                        "CapsLock pressed",
                    )))
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

    // let num_clicks = Rc::new(Cell::new(0));
    let gesture_controller = GestureDrag::new();
    gesture_controller.connect_drag_update({
        let txt = txt.clone();
        move |_, _, _| {
            debug!("its byggy!. it kills active highlight!");
            txt.set_cursor_highlight(false);
        }
    });
    txt.add_controller(gesture_controller);

    let gesture_controller = GestureClick::new();
    let click_lock: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    gesture_controller.connect_released({
        let sndr = sndr.clone();
        let txt = txt.clone();
        move |gesture, n_clicks, _wx, _wy| {
            gesture.set_state(EventSequenceState::Claimed);
            txt.set_cursor_highlight(true);
            let pos = txt.buffer().cursor_position();
            let iter = txt.buffer().iter_at_offset(pos);
            sndr.send_blocking(crate::Event::Cursor(
                iter.offset(),
                iter.line(),
            ))
            .expect("Could not send through channel");
            trace!(
                "click!.......................... n_click {:?} {:?}",
                n_clicks,
                click_lock
            );
            if n_clicks == 1 && (iter.has_tag(&file) || iter.has_tag(&hunk)) {
                click_lock.borrow_mut().replace(true);
                glib::source::timeout_add_local(Duration::from_millis(200), {
                    let sndr = sndr.clone();
                    let click_lock = click_lock.clone();
                    move || {
                        if click_lock.borrow().is_none() {
                            trace!("double click handled..............");
                            return glib::ControlFlow::Break;
                        }
                        click_lock.borrow_mut().take();
                        sndr.send_blocking(crate::Event::Expand(
                            iter.offset(),
                            iter.line(),
                        ))
                        .expect("Could not send through channel");
                        glib::ControlFlow::Break
                    }
                });
            }
            if n_clicks == 2 && iter.has_tag(&staged) {
                click_lock.borrow_mut().take();
                sndr.send_blocking(crate::Event::Stage(
                    crate::StageOp::Unstage(iter.line()),
                ))
                .expect("Could not send through channel");
            }
            if n_clicks == 2 && iter.has_tag(&unstaged) {
                click_lock.borrow_mut().take();
                sndr.send_blocking(crate::Event::Stage(
                    crate::StageOp::Stage(iter.line()),
                ))
                .expect("Could not send through channel");
            }
        }
    });

    txt.add_controller(gesture_controller);

    txt.connect_move_cursor({
        let sndr = sndr.clone();
        // let latest_char_offset = RefCell::new(0);
        move |view: &StageView, step, count, _selection| {
            view.set_cursor_highlight(true);
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
            }
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

    txt.connect_copy_clipboard({
        let sndr = sndr.clone();
        move |view| {
            let buffer = view.buffer();
            if let Some((start_iter, end_iter)) = buffer.selection_bounds() {
                sndr.send_blocking(crate::Event::CopyToClipboard(
                    start_iter.offset(),
                    end_iter.offset(),
                ))
                .expect("could not sent through channel");
            }
        }
    });

    txt.set_monospace(true);
    txt.set_editable(false);
    txt
}

pub fn cursor_to_line_offset(buffer: &TextBuffer, line_offset: i32) {
    let mut iter = buffer.iter_at_offset(buffer.cursor_position());
    iter.backward_line();
    iter.forward_lines(1);
    iter.forward_chars(line_offset);
    buffer.place_cursor(&iter);
}
