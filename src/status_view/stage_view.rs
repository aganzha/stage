// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::context::{CursorPosition, StatusRenderContext};
use crate::status_view::tags;
use crate::{DARK_CLASS, LIGHT_CLASS};
use async_channel::Sender;
use core::time::Duration;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, glib, EventControllerKey, EventControllerMotion, EventSequenceState, GestureClick,
    GestureDrag, MovementStep, TextBuffer, TextView, TextWindowType, Widget,
};
use libadwaita::StyleManager;
use log::{debug, trace};

use std::cell::RefCell;
use std::rc::Rc;

glib::wrapper! {
    pub struct StageView(ObjectSubclass<stage_view_internal::StageView>)
        @extends TextView, Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

mod stage_view_internal {

    use gtk4::prelude::*;
    use gtk4::{gdk, glib, graphene, Snapshot, TextView, TextViewLayer};
    use std::cell::{Cell, RefCell};

    use gtk4::subclass::prelude::*;

    // #cce0f8/23374f - 204/255 224/255 248/255  35 55 79
    const LIGHT_CURSOR: gdk::RGBA = gdk::RGBA::new(0.80, 0.878, 0.972, 1.0);

    // super bright!
    // const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.101, 0.294, 0.526, 1.0);
    // also bright
    // const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.166, 0.329, 0.525, 1.0);
    const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.094, 0.257, 0.454, 1.0);

    const DARK_BF_FILL: gdk::RGBA = gdk::RGBA::new(0.139, 0.139, 0.139, 1.0);
    const LIGHT_BG_FILL: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
    // color #f6f5f4/494949 - 246/255 245/255 244/255
    // f3f3f3 - 243/255
    // f4f4f4 - 244/255
    // const LIGHT_LINES: gdk::RGBA = gdk::RGBA::new(0.961, 0.961, 0.957, 1.0);
    const LIGHT_LINES: gdk::RGBA = gdk::RGBA::new(0.957, 0.957, 0.957, 1.0);
    const DARK_LINES: gdk::RGBA = gdk::RGBA::new(0.250, 0.250, 0.250, 1.0);

    // #deddda/383838 - 221/255 221/255 218/255
    const LIGHT_HUNKS: gdk::RGBA = gdk::RGBA::new(0.871, 0.871, 0.855, 1.0);
    const DARK_HUNKS: gdk::RGBA = gdk::RGBA::new(0.22, 0.22, 0.22, 1.0);

    #[derive(Default)]
    pub struct StageView {
        pub show_cursor: Cell<bool>,
        pub double_height_line: Cell<bool>,
        pub active_lines: Cell<(i32, i32)>,
        pub hunks: RefCell<Vec<i32>>,

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

                let known_line_height = self.obj().line_yrange(&iter).1;

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
                            rect.x() as f32,
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
                            rect.x() as f32,
                            y_from as f32,
                            rect.width() as f32,
                            y_to as f32,
                        ),
                    );
                }

                // highlight cursor ---------------------------------
                iter.set_offset(buffer.cursor_position());

                let (mut y_from, mut y_to) = self.obj().line_yrange(&iter);
                y_from = y_from + y_to - known_line_height;
                y_to = known_line_height;

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
                        rect.x() as f32,
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

    pub fn set_background(&self) {
        let manager = StyleManager::default();
        let is_dark = manager.is_dark();
        self.imp().is_dark.replace(is_dark);
        self.imp().is_dark_set.replace(true);
    }

    pub fn set_cursor_highlight(&self, value: bool) {
        self.imp().show_cursor.replace(value);
    }

    pub fn bind_highlights(&self, context: &StatusRenderContext) {
        match context.cursor_position {
            CursorPosition::CursorDiff(_) => self.imp().double_height_line.replace(true),
            _ => self.imp().double_height_line.replace(false),
        };

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

    pub fn calc_max_char_width(&self, window_width: i32) -> i32 {
        let buffer = self.buffer();
        let mut iter = buffer.iter_at_offset(0);
        let x_before = self.cursor_locations(Some(&iter)).0.x();
        let forwarded = iter.forward_char();
        if !forwarded {
            buffer.insert(&mut iter, " ");
        };
        let x_after = self.cursor_locations(Some(&iter)).0.x();
        let width = if self.width() > 0 {
            self.width()
        } else {
            window_width
        };
        width / (x_after - x_before)
    }
}

glib::wrapper! {
    pub struct EmptyLayoutManager(ObjectSubclass<empty_layout_manager_internal::EmptyLayoutManager>)
        @extends gtk4::LayoutManager;
}

mod empty_layout_manager_internal {

    use gtk4::subclass::prelude::*;
    use gtk4::{glib, LayoutManager, Widget};

    #[derive(Default)]
    pub struct EmptyLayoutManager {}
    #[glib::object_subclass]
    impl ObjectSubclass for EmptyLayoutManager {
        const NAME: &'static str = "EmptyLayoutManager";
        type Type = super::EmptyLayoutManager;
        type ParentType = LayoutManager;
    }
    impl ObjectImpl for EmptyLayoutManager {}
    impl LayoutManagerImpl for EmptyLayoutManager {
        fn allocate(&self, _widget: &Widget, _width: i32, _height: i32, _baseline: i32) {
            // just an empty method
        }
    }
}

impl Default for EmptyLayoutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl EmptyLayoutManager {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }
}

pub fn factory(sndr: Sender<crate::Event>, name: &str) -> StageView {
    let manager = StyleManager::default();
    let is_dark = manager.is_dark();

    let txt = StageView::new();
    // txt.set_accessible_role(gtk4::AccessibleRole::None);

    txt.set_margin_start(12);
    txt.set_widget_name(name);
    txt.set_margin_end(12);
    txt.set_margin_top(12);
    txt.set_margin_bottom(12);
    txt.set_background();
    if is_dark {
        txt.set_css_classes(&[DARK_CLASS]);
    } else {
        txt.set_css_classes(&[LIGHT_CLASS]);
    }

    let buffer = txt.buffer();
    let table = buffer.tag_table();
    let pointer = tags::TxtTag::from_str(tags::POINTER).create();
    let staged = tags::TxtTag::from_str(tags::STAGED).create();
    let unstaged = tags::TxtTag::from_str(tags::UNSTAGED).create();
    let file = tags::TxtTag::from_str(tags::FILE).create();
    let hunk = tags::TxtTag::from_str(tags::HUNK).create();
    let oid = tags::TxtTag::from_str(tags::OID).create();

    for tag_name in tags::TEXT_TAGS {
        match tag_name {
            tags::POINTER => table.add(&pointer),
            tags::STAGED => table.add(&staged),
            tags::UNSTAGED => table.add(&unstaged),
            tags::FILE => table.add(&file),
            tags::HUNK => table.add(&hunk),
            tags::OID => table.add(&oid),
            _ => table.add(&tags::TxtTag::from_str(tag_name).create()),
        };
    }

    manager.connect_dark_notify({
        // color_scheme
        let txt = txt.clone();
        move |manager| {
            let is_dark = manager.is_dark();
            if is_dark {
                txt.remove_css_class(LIGHT_CLASS);
                txt.add_css_class(DARK_CLASS);
            } else {
                txt.remove_css_class(DARK_CLASS);
                txt.add_css_class(LIGHT_CLASS);
            }
            debug!("JUST SET new css classes {:?}", txt.css_classes());
            table.foreach(|tt| {
                if let Some(name) = tt.name() {
                    let t = tags::TxtTag::unknown_tag(name.to_string());
                    t.fill_text_tag(tt, is_dark);
                }
            });
            txt.set_background();
        }
    });

    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed({
        let buffer = buffer.clone();
        let sndr = sndr.clone();
        let oid = oid.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::Tab | gdk::Key::space, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Expand(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                    return glib::Propagation::Stop;
                }
                (gdk::Key::s | gdk::Key::a | gdk::Key::Return, _) => {
                    let pos = buffer.cursor_position();
                    let iter = buffer.iter_at_offset(pos);
                    if iter.has_tag(&oid) {
                        let mut start_iter = buffer.iter_at_offset(pos);
                        let mut end_iter = buffer.iter_at_offset(pos);
                        start_iter.backward_to_tag_toggle(Some(&oid));
                        end_iter.forward_to_tag_toggle(Some(&oid));
                        let oid_text = buffer.text(&start_iter, &end_iter, true);
                        sndr.send_blocking(crate::Event::ShowTextOid(oid_text.to_string()))
                            .expect("Cant send through channel");
                        return glib::Propagation::Stop;
                    }
                    sndr.send_blocking(crate::Event::Stage(crate::StageOp::Stage))
                        .expect("Could not send through channel");
                }
                (gdk::Key::u, _) => {
                    sndr.send_blocking(crate::Event::Stage(crate::StageOp::Unstage))
                        .expect("Could not send through channel");
                }
                (gdk::Key::k | gdk::Key::Delete | gdk::Key::BackSpace, _) => {
                    sndr.send_blocking(crate::Event::Stage(crate::StageOp::Kill))
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
                    let _iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Dump)
                        .expect("Could not send through channel");
                }
                (gdk::Key::d, _) => {
                    let _iter = buffer.iter_at_offset(buffer.cursor_position());
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
                    sndr.send_blocking(crate::Event::Toast(String::from("CapsLock pressed")))
                        .expect("Could not send through channel");
                }
                (key, modifier) => {
                    trace!("key press in status view {:?} {:?}", key.name(), modifier);
                }
            }
            glib::Propagation::Proceed
        }
    });
    txt.add_controller(key_controller);

    let gesture_controller = GestureDrag::new();
    gesture_controller.connect_drag_update({
        let txt = txt.clone();
        move |_, _, _| {
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
            sndr.send_blocking(crate::Event::Cursor(iter.offset(), iter.line()))
                .expect("Cant send through channel");

            if iter.has_tag(&oid) {
                let mut start_iter = txt.buffer().iter_at_offset(pos);
                let mut end_iter = txt.buffer().iter_at_offset(pos);
                start_iter.backward_to_tag_toggle(Some(&oid));
                end_iter.forward_to_tag_toggle(Some(&oid));
                let oid_text = buffer.text(&start_iter, &end_iter, true);
                sndr.send_blocking(crate::Event::ShowTextOid(oid_text.to_string()))
                    .expect("Cant send through channel");
            }
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
                        sndr.send_blocking(crate::Event::Expand(iter.offset(), iter.line()))
                            .expect("Could not send through channel");
                        glib::ControlFlow::Break
                    }
                });
            }
            if n_clicks == 2 && iter.has_tag(&staged) {
                click_lock.borrow_mut().take();
                sndr.send_blocking(crate::Event::Stage(crate::StageOp::Unstage))
                    .expect("Could not send through channel");
            }
            if n_clicks == 2 && iter.has_tag(&unstaged) {
                click_lock.borrow_mut().take();
                sndr.send_blocking(crate::Event::Stage(crate::StageOp::Stage))
                    .expect("Could not send through channel");
            }
        }
    });

    txt.add_controller(gesture_controller);

    txt.connect_move_cursor({
        let sndr = sndr.clone();
        move |view: &StageView, step, count, _selection| {
            view.set_cursor_highlight(true);
            let buffer = view.buffer();
            let pos = buffer.cursor_position();
            let mut start_iter = buffer.iter_at_offset(pos);
            let line_before = start_iter.line();
            match step {
                MovementStep::LogicalPositions | MovementStep::VisualPositions => {
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
                sndr.send_blocking(crate::Event::Cursor(start_iter.offset(), current_line))
                    .expect("Could not send through channel");
            }
        }
    });

    let motion_controller = EventControllerMotion::new();
    motion_controller.connect_motion({
        let txt = txt.clone();
        move |_c, x, y| {
            let (x, y) = txt.window_to_buffer_coords(TextWindowType::Text, x as i32, y as i32);
            if let Some(iter) = txt.iter_at_location(x, y) {
                if iter.has_tag(&pointer) {
                    txt.set_cursor(Some(&gdk::Cursor::from_name("pointer", None).unwrap()));
                } else {
                    txt.set_cursor(Some(&gdk::Cursor::from_name("text", None).unwrap()));
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
