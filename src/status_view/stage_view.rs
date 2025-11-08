// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::context::StatusRenderContext;
use crate::status_view::tags;
use crate::{DARK_CLASS, LIGHT_CLASS};
use async_channel::Sender;
use core::time::Duration;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, glib, pango::Underline, EventControllerKey, EventControllerMotion, EventSequenceState,
    GestureClick, GestureDrag, MovementStep, TextBuffer, TextIter, TextTag, TextView,
    TextWindowType, Widget,
};
use libadwaita::StyleManager;
use log::trace;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

glib::wrapper! {
    pub struct StageView(ObjectSubclass<stage_view_internal::StageView>)
        @extends TextView, Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Editable, gtk4::Scrollable;
}

mod stage_view_internal {

    use crate::LineKind;
    use git2::DiffLineType;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use gtk4::{gdk, glib, graphene, gsk, pango, Snapshot, TextView, TextViewLayer};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    // #cce0f8/23374f - 204/255 224/255 248/255  35 55 79
    const LIGHT_CURSOR: gdk::RGBA = gdk::RGBA::new(0.80, 0.878, 0.972, 1.0);

    // super bright!
    // const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.101, 0.294, 0.526, 1.0);
    // also bright
    // const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.166, 0.329, 0.525, 1.0);
    const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.094, 0.257, 0.454, 1.0);

    const DARK_BG_FILL: gdk::RGBA = gdk::RGBA::new(0.139, 0.139, 0.139, 1.0);
    const LIGHT_BG_FILL: gdk::RGBA = gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
    // color #f6f5f4/494949 - 246/255 245/255 244/255
    // f3f3f3 - 243/255
    // f4f4f4 - 244/255
    // const LIGHT_LINES: gdk::RGBA = gdk::RGBA::new(0.961, 0.961, 0.957, 1.0);
    const LIGHT_LINES: gdk::RGBA = gdk::RGBA::new(0.957, 0.957, 0.957, 1.0);
    const DARK_LINES: gdk::RGBA = gdk::RGBA::new(0.200, 0.200, 0.200, 1.0); // was 250

    // #deddda/383838 - 221/255 221/255 218/255
    const LIGHT_HUNKS: gdk::RGBA = gdk::RGBA::new(0.871, 0.871, 0.855, 1.0);
    const DARK_HUNKS: gdk::RGBA = gdk::RGBA::new(0.22, 0.22, 0.22, 1.0);

    const MAX_LINES_ON_SCREEN: i32 = 100;

    #[derive(Default)]
    pub struct StageView {
        pub show_cursor: Cell<bool>,
        pub active_lines: Cell<(i32, i32)>,
        pub hunks: RefCell<Vec<i32>>,
        pub linenos: RefCell<HashMap<i32, (String, DiffLineType, LineKind)>>,

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

    impl StageView {
        pub fn get_line_no_margin(&self) -> i32 {
            // this related to lower
            80
        }
        fn get_line_no_offset(&self, _line_height: i32) -> f32 {
            // this related to upper
            5.0
        }

        fn lineno_label_layout(
            &self,
            line_no: i32,
            is_current: bool,
            is_dark: bool,
        ) -> Option<(pango::Layout, gdk::RGBA)> {
            let linenos = self.linenos.borrow();
            let line_attrs = linenos.get(&line_no)?;
            let layout = self.obj().create_pango_layout(Some(&line_attrs.0));
            let mut rgba = gdk::RGBA::BLACK;
            if is_dark {
                rgba = gdk::RGBA::WHITE;
            }
            match line_attrs.2 {
                LineKind::None => match line_attrs.1 {
                    DiffLineType::Addition => {
                        rgba = gdk::RGBA::GREEN;
                    }
                    DiffLineType::Deletion => {
                        rgba = gdk::RGBA::RED;
                    }
                    _ => {}
                },
                LineKind::Ours(_) => {}
                LineKind::Theirs(_) => {}
                _ => {}
            }
            if !is_current {
                rgba.set_alpha(0.15);
            }
            Some((layout, rgba))
        }
    }

    impl TextViewImpl for StageView {
        fn snapshot_layer(&self, layer: TextViewLayer, snapshot: Snapshot) {
            if layer == TextViewLayer::BelowText {
                let rect = self.obj().visible_rect();
                let bg_fill = if self.is_dark.get() {
                    &DARK_BG_FILL
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
                            64000.0,
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
            } else {
                let rect = self.obj().visible_rect();
                let rect_height = rect.height();
                if rect_height == 0 {
                    return;
                }

                let buffer = self.obj().buffer();
                let mut iter = buffer.iter_at_offset(0);
                let (_, line_height) = self.obj().line_yrange(&iter);
                if line_height <= 0 {
                    return;
                }
                let mut line_no = rect.y() / line_height - 5;

                let cursor_iter = buffer.iter_at_offset(buffer.cursor_position());
                let cursor_line = cursor_iter.line();

                let is_dark = self.is_dark.get();
                let mut passed = line_height;
                loop {
                    let is_current = line_no == cursor_line;
                    if let Some((label, color)) =
                        self.lineno_label_layout(line_no, is_current, is_dark)
                    {
                        let mut transform = gsk::Transform::new();
                        iter.set_line(line_no);
                        let (y_from, _) = self.obj().line_yrange(&iter);
                        transform = transform.translate(&graphene::Point::new(
                            self.get_line_no_offset(line_height),
                            y_from as f32,
                        ));
                        snapshot.save();
                        snapshot.transform(Some(&transform));
                        snapshot.append_layout(&label, &color);
                        snapshot.restore();
                    }
                    passed += line_height;
                    if passed > rect_height + 128 {
                        break;
                    }
                    if passed > MAX_LINES_ON_SCREEN * line_height {
                        break;
                    }
                    line_no += 1;
                }
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
        if let Some(lines) = context.highlight_lines {
            self.imp().active_lines.replace(lines);
        } else {
            self.imp().active_lines.replace((0, 0));
        }
        self.imp().hunks.replace(Vec::new());
        for h in &context.highlight_hunks {
            self.imp().hunks.borrow_mut().push(*h);
        }
        self.imp().linenos.replace(context.linenos.clone());
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

    // ----------------- COLORS -------------------------------------
    // Dark theme - first color on tuple. Light - last one.
    let green = tags::Color(("#4a8e09".to_string(), "#10ac64".to_string())); //
    let red = tags::Color(("#a51d2d".to_string(), "#c01c28".to_string()));
    // in terms of dark theme: white  - is white font on black backgroud. grey is near white.
    // in terms of light theme: black - is black font on white color. greay is near black
    let grey = tags::Color(("#999999".to_string(), "#555555".to_string()));

    let diff_color = tags::Color(("#a78a44".to_string(), "#8b6508".to_string()));
    let conflict_color = tags::Color(("#ff0000".to_string(), "#ff0000".to_string()));

    // ----------------- COLORS -------------------------------------

    let diff = tags::ColorTag((tags::DIFF, diff_color));
    let diff_tag = diff.create(&table, is_dark);
    diff_tag.set_weight(700);
    diff_tag.set_pixels_above_lines(32);

    let conflict_marker = tags::ColorTag((tags::CONFLICT_MARKER, conflict_color.clone()));
    let conflict_marker_tag = conflict_marker.create(&table, is_dark);

    let spaces_added = tags::ColorTag((tags::SPACES_ADDED, green.clone()));
    let spaces_added_tag = spaces_added.create(&table, is_dark);

    let spaces_removed = tags::ColorTag((tags::SPACES_REMOVED, red.clone()));
    let spaces_removed_tag = spaces_removed.create(&table, is_dark);

    let added = tags::ColorTag((tags::ADDED, green.clone()));
    let added_tag = added.create(&table, is_dark);

    let enhanced_added = tags::ColorTag((
        tags::ENHANCED_ADDED,
        green.from_hsl(tags::HslAdjustment::Enhance),
    ));
    let enhanced_added_tag = enhanced_added.create(&table, is_dark);

    let removed = tags::ColorTag((tags::REMOVED, red.clone()));
    let removed_tag = removed.create(&table, is_dark);

    let enhanced_removed = tags::ColorTag((
        tags::ENHANCED_REMOVED,
        red.from_hsl(tags::HslAdjustment::Enhance),
    ));
    let enhanced_removed_tag = enhanced_removed.create(&table, is_dark);

    let context = tags::ColorTag((tags::CONTEXT, grey.clone()));
    let context_tag = context.create(&table, is_dark);

    let enhanced_context = tags::ColorTag((tags::ENHANCED_CONTEXT, grey.darken(Some(0.2)))); //grey.from_hsl(tags::HslAdjustment::Enhance)));
    let enhanced_context_tag = enhanced_context.create(&table, is_dark);

    let syntax = tags::ColorTag((tags::SYNTAX, grey.darken(Some(0.3)))); //grey.from_hsl(tags::HslAdjustment::Up(false))));
    let syntax_tag = syntax.create(&table, is_dark);

    let enhanced_syntax = tags::ColorTag((tags::ENHANCED_SYNTAX, grey.darken(Some(0.4)))); // grey.from_hsl(tags::HslAdjustment::Up(true))
    let enhanced_syntax_tag = enhanced_syntax.create(&table, is_dark);

    let syntax_1 = tags::ColorTag((tags::SYNTAX_1, grey.darken(Some(-0.2)))); //grey.from_hsl(tags::HslAdjustment::Down(false))));
    let syntax_1_tag = syntax_1.create(&table, is_dark);

    let enhanced_syntax_1 = tags::ColorTag((tags::ENHANCED_SYNTAX_1, grey.darken(Some(-0.2)))); //grey.from_hsl(tags::HslAdjustment::Down(true))));
    let enhanced_syntax_1_tag = enhanced_syntax_1.create(&table, is_dark);

    let syntax_added = tags::ColorTag((
        tags::SYNTAX_ADDED,
        green.from_hsl(tags::HslAdjustment::Up(false)),
    ));
    let syntax_added_tag = syntax_added.create(&table, is_dark);

    let enhanced_syntax_added = tags::ColorTag((
        tags::ENHANCED_SYNTAX_ADDED,
        green.from_hsl(tags::HslAdjustment::Up(true)),
    ));
    let enhanced_syntax_added_tag = enhanced_syntax_added.create(&table, is_dark);

    let syntax_removed = tags::ColorTag((
        tags::SYNTAX_REMOVED,
        red.from_hsl(tags::HslAdjustment::Up(false)),
    ));
    let syntax_removed_tag = syntax_removed.create(&table, is_dark);

    let enhanced_syntax_removed = tags::ColorTag((
        tags::ENHANCED_SYNTAX_REMOVED,
        red.from_hsl(tags::HslAdjustment::Up(true)),
    ));
    let enhanced_syntax_removed_tag = enhanced_syntax_removed.create(&table, is_dark);

    let syntax_1_added = tags::ColorTag((
        tags::SYNTAX_1_ADDED,
        green.from_hsl(tags::HslAdjustment::Down(false)),
    )); //magenta_color.clone()
    let syntax_1_added_tag = syntax_1_added.create(&table, is_dark);

    let syntax_1_removed = tags::ColorTag((
        tags::SYNTAX_1_REMOVED,
        red.from_hsl(tags::HslAdjustment::Down(false)),
    )); // yellow_color.clone()
    let syntax_1_removed_tag = syntax_1_removed.create(&table, is_dark);

    let enhanced_syntax_1_added = tags::ColorTag((
        tags::ENHANCED_SYNTAX_1_ADDED,
        green.from_hsl(tags::HslAdjustment::Down(true)),
    )); //magenta_color
    let enhanced_syntax_1_added_tag = enhanced_syntax_1_added.create(&table, is_dark);

    let enhanced_syntax_1_removed = tags::ColorTag((
        tags::ENHANCED_SYNTAX_1_REMOVED,
        red.from_hsl(tags::HslAdjustment::Down(true)),
    )); // yellow_color
    let enhanced_syntax_1_removed_tag = enhanced_syntax_1_removed.create(&table, is_dark);

    let pointer = tags::Tag(tags::POINTER).create(&table);
    let staged = tags::Tag(tags::STAGED).create(&table);
    let unstaged = tags::Tag(tags::UNSTAGED).create(&table);
    let file = tags::Tag(tags::FILE).create(&table);
    let hunk = tags::Tag(tags::HUNK).create(&table);
    let oid = tags::Tag(tags::OID).create(&table);

    let bold = tags::Tag(tags::BOLD).create(&table);
    bold.set_weight(700);

    let underline = tags::Tag(tags::UNDERLINE).create(&table);
    underline.set_underline(Underline::Single);

    tags::Tag(tags::OURS).create(&table);
    tags::Tag(tags::THEIRS).create(&table);

    for tag_name in tags::TEXT_TAGS {
        if table.lookup(tag_name).is_none() {
            panic!("tag is not added to the table {:?}", tag_name);
        }
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
            txt.set_background();

            diff.toggle(&diff_tag, is_dark);
            conflict_marker.toggle(&conflict_marker_tag, is_dark);

            spaces_added.toggle(&spaces_added_tag, is_dark);
            spaces_removed.toggle(&spaces_removed_tag, is_dark);

            added.toggle(&added_tag, is_dark);
            enhanced_added.toggle(&enhanced_added_tag, is_dark);

            removed.toggle(&removed_tag, is_dark);
            enhanced_removed.toggle(&enhanced_removed_tag, is_dark);

            context.toggle(&context_tag, is_dark);
            enhanced_context.toggle(&enhanced_context_tag, is_dark);

            syntax.toggle(&syntax_tag, is_dark);
            enhanced_syntax.toggle(&enhanced_syntax_tag, is_dark);

            syntax_1.toggle(&syntax_1_tag, is_dark);
            enhanced_syntax_1.toggle(&enhanced_syntax_1_tag, is_dark);

            syntax_added.toggle(&syntax_added_tag, is_dark);
            enhanced_syntax_added.toggle(&enhanced_syntax_added_tag, is_dark);

            syntax_removed.toggle(&syntax_removed_tag, is_dark);
            enhanced_syntax_removed.toggle(&enhanced_syntax_removed_tag, is_dark);

            syntax_1_added.toggle(&syntax_1_added_tag, is_dark);
            enhanced_syntax_1_added.toggle(&enhanced_syntax_1_added_tag, is_dark);

            syntax_1_removed.toggle(&syntax_1_removed_tag, is_dark);
            enhanced_syntax_1_removed.toggle(&enhanced_syntax_1_removed_tag, is_dark);
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
                    if key == gdk::Key::Return {
                        let pos = buffer.cursor_position();
                        let iter = buffer.iter_at_offset(pos);
                        if let Some((start_iter, end_iter)) = iters_for(&oid, &iter) {
                            let oid_text = buffer.text(&start_iter, &end_iter, true);
                            sndr.send_blocking(crate::Event::ShowTextOid(oid_text.to_string()))
                                .expect("Cant send through channel");
                            return glib::Propagation::Stop;
                        }
                    }
                    sndr.send_blocking(crate::Event::Stage(crate::StageOp::Stage))
                        .expect("Could not send through channel");
                }
                (gdk::Key::u | gdk::Key::r, _) => {
                    sndr.send_blocking(crate::Event::Stage(crate::StageOp::Unstage))
                        .expect("Could not send through channel");
                }
                (gdk::Key::k | gdk::Key::Delete | gdk::Key::BackSpace, _) => {
                    sndr.send_blocking(crate::Event::Stage(crate::StageOp::Kill))
                        .expect("Could not send through channel");
                }
                (gdk::Key::b, gdk::ModifierType::CONTROL_MASK) => {
                    sndr.send_blocking(crate::Event::Blame)
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
                (gdk::Key::o, _) => {
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
        let oid = oid.clone();
        move |gesture, n_clicks, _wx, _wy| {
            gesture.set_state(EventSequenceState::Claimed);
            txt.set_cursor_highlight(true);
            let pos = txt.buffer().cursor_position();
            let iter = txt.buffer().iter_at_offset(pos);
            sndr.send_blocking(crate::Event::Cursor(iter.offset(), iter.line()))
                .expect("Cant send through channel");
            if let Some((start_iter, end_iter)) = iters_for(&oid, &iter) {
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

    let current_underline: Rc<Cell<Option<(i32, i32)>>> = Rc::new(Cell::new(None));

    txt.add_controller(gesture_controller);

    txt.connect_move_cursor({
        let sndr = sndr.clone();
        let txt = txt.clone();
        let current_underline = current_underline.clone();
        let oid = oid.clone();
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
                    let char_offset = start_iter.line_offset();
                    start_iter.forward_lines(count);
                    start_iter.forward_to_line_end();
                    if start_iter.line_offset() > char_offset {
                        start_iter.set_line_offset(char_offset);
                    }
                }
                MovementStep::DisplayLineEnds
                | MovementStep::Paragraphs
                | MovementStep::ParagraphEnds
                | MovementStep::Pages
                | MovementStep::BufferEnds
                | MovementStep::HorizontalPages => {}
                _ => todo!(),
            }

            if let Some((u_start, u_end)) = current_underline.get() {
                let u_start_iter = txt.buffer().iter_at_offset(u_start);
                let u_end_iter = txt.buffer().iter_at_offset(u_end);
                buffer.remove_tag_by_name(tags::UNDERLINE, &u_start_iter, &u_end_iter);
                current_underline.replace(None);
            }
            if let Some((u_start_iter, u_end_iter)) = iters_for(&oid, &start_iter) {
                if u_start_iter.line_offset() <= start_iter.line_offset()
                    && u_end_iter.line_offset() >= start_iter.line_offset()
                {
                    // condition above and below are weird cases, when on line 0
                    // start tag fall backward to line offset 0
                    // on other lines those conditions are not required...
                    if u_start_iter.has_tag(&oid) {
                        buffer.apply_tag_by_name(tags::UNDERLINE, &u_start_iter, &u_end_iter);
                        current_underline
                            .replace(Some((u_start_iter.offset(), u_end_iter.offset())));
                    }
                }
            }
            let current_line = start_iter.line();
            if line_before != current_line {
                let mut cycle_iter = buffer.iter_at_offset(start_iter.offset());
                cycle_iter.set_line_offset(0);
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
                let buffer = txt.buffer();
                if let Some((u_start, u_end)) = current_underline.get() {
                    let start_iter = txt.buffer().iter_at_offset(u_start);
                    let end_iter = txt.buffer().iter_at_offset(u_end);
                    buffer.remove_tag_by_name(tags::UNDERLINE, &start_iter, &end_iter);
                    current_underline.replace(None);
                }
                if let Some((start_iter, end_iter)) = iters_for(&oid, &iter) {
                    buffer.apply_tag_by_name(tags::UNDERLINE, &start_iter, &end_iter);
                    current_underline.replace(Some((start_iter.offset(), end_iter.offset())));
                }
                if iter.has_tag(&pointer) {
                    txt.set_cursor(Some(&gdk::Cursor::from_name("pointer", None).unwrap()));
                } else {
                    txt.set_cursor(Some(&gdk::Cursor::from_name("text", None).unwrap()));
                }
            }
        }
    });
    txt.add_controller(motion_controller);

    txt.set_monospace(true);
    gtk4::prelude::TextViewExt::set_editable(&txt, false);
    txt
}

pub fn iters_for(tag: &TextTag, iter: &TextIter) -> Option<(TextIter, TextIter)> {
    if iter.has_tag(tag) {
        let mut start_iter = iter.buffer().iter_at_offset(iter.offset());
        let mut end_iter = iter.buffer().iter_at_offset(iter.offset());
        start_iter.backward_to_tag_toggle(Some(tag));
        end_iter.forward_to_tag_toggle(Some(tag));
        return Some((start_iter, end_iter));
    }
    None
}

pub fn cursor_to_line_offset(buffer: &TextBuffer, line_offset: i32) {
    let mut iter = buffer.iter_at_offset(buffer.cursor_position());
    iter.backward_line();
    iter.forward_lines(1);
    iter.forward_chars(line_offset);
    buffer.place_cursor(&iter);
}
