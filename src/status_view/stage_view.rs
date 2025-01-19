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
    gdk, glib, pango, EventControllerKey, EventControllerMotion, EventControllerScroll,
    EventSequenceState, GestureClick, GestureDrag, MovementStep, PropagationPhase, ScrollStep,
    ScrolledWindow, TextBuffer, TextTag, TextView, TextWindowType, Widget,
};
use libadwaita::StyleManager;
use log::{debug, trace};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

glib::wrapper! {
    pub struct StageView(ObjectSubclass<stage_view_internal::StageView>)
        @extends TextView, Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

const MAP_WIDTH: i32 = 150;

mod stage_view_internal {

    use super::Convert;
    use crate::glib::Properties;
    use log::{debug, trace};

    use gtk4::prelude::*;
    use gtk4::{gdk, glib, graphene, Snapshot, TextView, TextViewLayer};
    use std::cell::{Cell, RefCell};

    use gtk4::subclass::prelude::*;

    // #cce0f8/23374f - 204/255 224/255 248/255  35 55 79
    const LIGHT_CURSOR: gdk::RGBA = gdk::RGBA::new(0.80, 0.878, 0.972, 1.0);
    const DARK_CURSOR: gdk::RGBA = gdk::RGBA::new(0.137, 0.216, 0.310, 1.0);

    const DARK_BG_FILL: gdk::RGBA = gdk::RGBA::new(0.139, 0.139, 0.139, 1.0);
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

    const SLIDER_HEIGHT: f32 = 50.0;
    const SLIDER_MARGIN: f32 = 10.0;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::StageView)]
    pub struct StageView {
        pub is_map: Cell<bool>,
        pub map_slider_start: Cell<f64>,

        pub drag_diff: Cell<(i32, i32)>,

        //pub map_slider_diff: Cell<f64>,
        pub show_cursor: Cell<bool>,
        //pub double_height_line: Cell<bool>,
        pub active_lines: Cell<(i32, i32)>,
        pub hunks: RefCell<Vec<i32>>,

        // TODO! put it here!
        pub is_dark: Cell<bool>,
        pub is_dark_set: Cell<bool>,

        #[property(get, set)]
        pub visible_start_line: Cell<i32>,

        #[property(get, set)]
        pub visible_end_line: Cell<i32>,

        pub possible_line_count: Cell<Option<i32>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StageView {
        const NAME: &'static str = "StageView";
        type Type = super::StageView;
        type ParentType = TextView;
    }

    impl StageView {
        fn snapshot_layer_map(&self, snapshot: Snapshot) {
            self.obj().adjust_height();
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

            let slider_fill = if self.is_dark.get() {
                &LIGHT_BG_FILL.with_alpha(0.1)
            } else {
                &DARK_BG_FILL.with_alpha(0.1)
            };

            let buffer = self.obj().buffer();
            let mut iter = buffer.iter_at_offset(0);

            let line_no_start = self.obj().visible_start_line();
            let line_no_end = self.obj().visible_end_line();
            if let Some(mut iter) = self.obj().buffer().iter_at_line(line_no_start) {
                let y_from = self.obj().line_yrange(&iter).0;
                iter.set_line(line_no_end);
                let y_to = self.obj().line_yrange(&iter).0;

                snapshot.append_color(
                    slider_fill,
                    &graphene::Rect::new(0 as f32, y_from as f32, rect.width() as f32, y_to as f32),
                );
                snapshot.append_color(
                    bg_fill,
                    &graphene::Rect::new(
                        0 as f32,
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
        }
    }

    impl TextViewImpl for StageView {
        fn snapshot_layer(&self, layer: TextViewLayer, snapshot: Snapshot) {
            if layer == TextViewLayer::BelowText {
                trace!(
                    "snapshot_layer am i map? {:?} my visible end {:?}",
                    self.is_map.get(),
                    self.visible_end_line.get()
                );
                if self.is_map.get() {
                    self.snapshot_layer_map(snapshot);
                    return;
                }
                let rect = self.obj().visible_rect();
                let (line_from, line_to) =
                    self.obj().ys_to_lines((rect.y(), rect.y() + rect.height()));

                self.obj().set_visible_start_line(line_from);
                self.obj().set_visible_end_line(line_to);

                trace!(
                    "highlight stage ................. {:?} {:?} prtops vs values {:?} {:?}",
                    self.obj().visible_start_line(),
                    self.obj().visible_end_line(),
                    line_from,
                    line_to
                );
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
                let common_line_height = self.obj().line_yrange(&iter).1;

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

                if y_to > common_line_height {
                    y_from += common_line_height / 2;
                    y_to = common_line_height;
                }
                // y_from = y_from + y_to - known_line_height;
                // y_to = known_line_height;

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
    #[glib::derived_properties]
    impl ObjectImpl for StageView {}

    impl WidgetImpl for StageView {}
}

impl Default for StageView {
    fn default() -> Self {
        Self::new(false)
    }
}

pub trait Convert<T> {
    fn ys_to_lines(&self, ys: (T, T)) -> (i32, i32);
}

impl StageView {
    pub fn new(is_map: bool) -> Self {
        let me: Self = glib::Object::builder().build();
        me.imp().is_map.replace(is_map);
        me.imp().map_slider_start.replace(0.0);
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
        // why use method here? just to not expose imp()?
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

    pub fn after_window_present(&self) {
        if self.imp().is_map.get() {
            self.set_cursor(Some(&gdk::Cursor::from_name("pointer", None).unwrap()));
        }
    }

    pub fn set_possible_line_count(&self, line_count: usize) {
        self.imp()
            .possible_line_count
            .replace(Some(line_count as i32));
    }

    pub fn adjust_height(&self) {
        if self.imp().possible_line_count.get().is_none() {
            return;
        }
        let line_count = self.imp().possible_line_count.get().unwrap();
        let buffer = self.buffer();
        let iter = buffer.iter_at_offset(0);
        let current_height = self.line_yrange(&iter).1 as f32;
        debug!(
            "IIIIIIIIIIIIIIIIIIII y_range {:?} MAP IS MAPPED? {:?} vs {:?}",
            current_height,
            self.is_mapped(),
            self.is_realized()
        );
        let rect = self.visible_rect();
        let desired_height = (rect.height() / line_count) as f32;
        let mut ratio = desired_height / current_height;
        debug!(
            ">>>>> current_height(yrange) {:?} desired_height {:?} ratio {:?} rect {:?}",
            current_height, desired_height, ratio, rect
        );
        if ratio < 1.0 {
            let pango_ctx = self.ltr_context();
            let metrics = pango_ctx.metrics(None, None);
            let pango_height = metrics.height();
            let desired_pango_height = pango_height as f32 * ratio;

            if let Some(mut descr) = pango_ctx.font_description() {
                let size = descr.size();
                loop {
                    let desired_size = size as f32 * ratio;
                    // set desired font size and get metrics for it.
                    // if metrics height is less then desired - decrement ratio and repeat.
                    descr.set_size(desired_size as i32);
                    let metrics = pango_ctx.metrics(Some(&descr), None);
                    debug!(
                        "loooooooooooooooooooooop. metrics height {:?} desired_pango_height {:?} {:?}. descr size {:?} descr desired_size {:?}",
                        metrics.height(),
                        desired_pango_height,
                        ratio,
                        size,
                        desired_size
                    );
                    if metrics.height() as f32 > desired_pango_height {
                        ratio -= 0.01;
                        if ratio <= 0.0 {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                debug!("looop is over. here is the ration {:?}", ratio);
            }
        }
        let css_class = {
            if ratio >= 1.0 {
                ""
            } else if ratio < 1.0 && ratio >= 0.5 {
                "percent50"
            } else if ratio < 0.50 && ratio >= 0.30 {
                "percent30"
            } else if ratio < 0.30 && ratio >= 0.25 {
                "percent25"
            } else if ratio < 0.25 && ratio >= 0.20 {
                "percent20"
            } else if ratio < 0.20 && ratio >= 0.18 {
                "percent18"
            } else if ratio < 0.18 && ratio >= 0.16 {
                "percent16"
            } else if ratio < 0.16 && ratio >= 0.14 {
                "percent14"
            } else if ratio < 0.14 && ratio >= 0.12 {
                "percent12"
            } else if ratio < 0.12 && ratio >= 0.10 {
                "percent10"
            } else if ratio < 0.10 && ratio >= 0.9 {
                "percent9"
            } else if ratio < 0.9 && ratio >= 0.08 {
                "percent8"
            } else if ratio < 0.08 && ratio >= 0.07 {
                "percent7"
            } else if ratio < 0.07 && ratio >= 0.06 {
                "percent6"
            } else if ratio < 0.06 && ratio >= 0.05 {
                "percent5"
            } else if ratio < 0.05 && ratio >= 0.04 {
                "percent4"
            } else if ratio < 0.04 && ratio >= 0.03 {
                "percent3"
            } else if ratio < 0.03 && ratio >= 0.02 {
                "percent2"
            } else if ratio < 0.02 && ratio >= 0.01 {
                "percent1"
            } else {
                "percent1"
            }
        };
        debug!("css_class finally {:?}", css_class);
        if css_class.is_empty() {
            self.set_visible(false);
        } else {
            self.set_visible(true);
            self.add_css_class(css_class);
        }
        self.imp().possible_line_count.replace(None);
    }
}

impl Convert<i32> for StageView {
    fn ys_to_lines(&self, ys: (i32, i32)) -> (i32, i32) {
        if ys.0 == 0 && ys.1 == 0 {
            return (0, 0);
        }
        if let Some(mut start_iter) = self.iter_at_location(0, ys.0) {
            let from_line = start_iter.line();
            trace!("at least i have start iter {:?}", from_line);
            let to_line = if let Some(iter) = self.iter_at_location(0, ys.1) {
                trace!("......CORRECT from_line {:?} to line {:?} line_diff {:?} from_y {:?} to_y {:?} diff {:?}", from_line, iter.line(), iter.line() - from_line, ys.0, ys.1, ys.1 - ys.0);
                iter.line()
            } else {
                let mut to_line = from_line;
                while start_iter.forward_lines(1) {
                    to_line += 1;
                    if self.line_yrange(&start_iter).0 >= ys.1 {
                        break;
                    }
                }
                trace!("TO LINE from LOOOOP {:?}", to_line);
                to_line
            };
            return (from_line, to_line);
        }
        debug!("no start iter");
        return (0, 0);
    }
}
impl Convert<f64> for StageView {
    fn ys_to_lines(&self, ys: (f64, f64)) -> (i32, i32) {
        return self.ys_to_lines((ys.0 as i32, ys.1 as i32));
    }
}

pub fn make_map(
    stage: &StageView,
    name: &str,
    is_dark: bool,
    scroll: &ScrolledWindow,
) -> StageView {
    let map = StageView::new(true);
    map.set_widget_name(&format!("{}_map", name));
    map.set_vexpand(false);
    map.set_hexpand(false);
    map.set_margin_end(5);
    map.set_margin_top(5);

    map.set_cursor(Some(&gdk::Cursor::from_name("pointer", None).unwrap()));

    map.set_is_dark(is_dark, true);

    map.set_monospace(true);
    map.set_editable(false);

    map.set_width_request(MAP_WIDTH);

    let drag = GestureDrag::new();
    drag.set_propagation_phase(PropagationPhase::Capture);
    drag.connect_drag_begin({
        let stage = stage.clone();
        let map = map.clone();

        move |drag, _x: f64, y: f64| {
            drag.set_state(EventSequenceState::Claimed);
            // let current_y = map.imp().map_slider_start.get();
            //scroll_lock.replace(true);
            debug!("START SLIDING {:?}", y);
            // map.imp().map_slider_start.replace(y);
            // there are 2 cases for start dragging.
            // lets call 'start dragging' a 'click'.
            // a click either inside slider itself
            // or it is outside.
            // 1. click inside slider. just do nothing.
            // we will wait for drag update. here new y will
            // arrive and it will be exactly the diff on which it need
            // to move top edge of stage (stage then will paint its visible rect on map).
            // 2. click outside slider - it need to scroll directly to that line: e.g.
            // move stage top edge to that line.
            // IMPORTANT! buffer coords of map and stage are the same, cuase buffer is the same!
            let (_, new_y) = map.window_to_buffer_coords(TextWindowType::Text, 0, y as i32);
            let may_be_iter = map.iter_at_location(0, new_y);
            if may_be_iter.is_none() {
                return;
            }
            let mut new_y_iter = may_be_iter.unwrap();
            let new_y_line = new_y_iter.line();
            let rect = stage.visible_rect();
            let stage_rect_lines = stage.ys_to_lines((rect.y(), rect.y() + rect.height()));
            if new_y_line >= stage_rect_lines.0 && new_y_line <= stage_rect_lines.1 {
                debug!(
                    "click WITHIN SLIDER. STORE START POINT {:?} {:?}",
                    new_y, rect
                );
                stage
                    .imp()
                    .drag_diff
                    .replace((new_y, new_y_line - stage_rect_lines.0));
                // click inside slider. store start point. update will move slider later.
            } else {
                debug!(
                    "click OUTSIDE SLIDER. SCROLL to {:?} (visible lines {:?})",
                    new_y_line, stage_rect_lines
                );
                new_y_iter.set_line_offset(0);
                stage.scroll_to_iter(&mut new_y_iter, 0.0, true, 0.0, 0.0);
            }
        }
    });

    drag.connect_drag_update({
        let map = map.clone();
        let stage = stage.clone();
        move |drag, _x: f64, y: f64| {
            drag.set_state(EventSequenceState::Claimed);
            // see explanation in drug start.
            // to add - here it just need to move stage top visible edge
            // by DIFF of y.
            if y == 0.0 {
                debug!("empty update by y....");
                return;
            }
            // let (_, new_y) = map.window_to_buffer_coords(TextWindowType::Text, 0, y as i32);
            let new_y = y as i32;
            debug!(
                "~~~~~~~~~~~~~~~~~~~~~~ drag update y (event) {:?} and new_y (to buffer) {:?}",
                y, new_y
            );
            if new_y == 0 {
                debug!("empty update....");
                return;
            }
            // let top_edge_stage_y = new_y - stage.imp().drag_diff.get();
            // here we are in buffer coords and they are the same among stage and map!
            let (drag_start, line_diff) = stage.imp().drag_diff.get();
            if let Some(mut iter) = map.iter_at_location(0, drag_start + new_y) {
                iter.forward_lines(0 - line_diff);
                debug!(
                    "DRAG UPDATE new_y {:?} line_diff {:?} line >>>>>>>> {:?}",
                    new_y,
                    line_diff,
                    iter.line()
                );
                iter.set_line_offset(0);
                stage.scroll_to_iter(&mut iter, 0.0, true, 0.0, 0.0);
                // also it need to scroll map accordingly!
                // map.emit_move_viewport(ScrollStep::Steps, 1);
            } else {
                debug!("hmmmmmmmmmmmmmmmmmmmmmm {:?} {:?}", y, new_y);
            }
        }
    });
    drag.connect_drag_end({
        let map = map.clone();
        let stage = stage.clone();
        //let scroll_lock = scroll_lock.clone();
        move |drag, _x: f64, y: f64| {
            drag.set_state(EventSequenceState::Claimed);
            if y == 0.0 {
                debug!("empty end by y....");
                return;
            }
            // let (_, new_y) = map.window_to_buffer_coords(TextWindowType::Text, 0, y as i32);
            let new_y = y as i32;
            debug!("__________________________________ drug end {:?}", new_y);
            if new_y == 0 {
                debug!("empty end drag....");
                return;
            }
            let (drag_start, line_diff) = stage.imp().drag_diff.get();
            if let Some(mut iter) = map.iter_at_location(0, drag_start + new_y) {
                iter.forward_lines(0 - line_diff);

                debug!(
                    "DRAG END new_y {:?} line_diff {:?} line ~~~~~~~~~~> {:?}",
                    new_y,
                    line_diff,
                    iter.line()
                );
                iter.set_line_offset(0);
                stage.scroll_to_iter(&mut iter, 0.0, true, 0.0, 0.0);
                // also it need to scroll map accordingly!
                // map.emit_move_viewport(ScrollStep::Steps, 1);
            } else {
                debug!("END hmmmmmmmmmmmmmmmmmmmmmm {:?} {:?}", y, new_y);
            }
        }
    });
    map.add_controller(drag);

    let click = GestureClick::new();
    click.set_propagation_phase(PropagationPhase::Capture);
    click.connect_pressed({
        move |click, _n_clicks: i32, _x: f64, _y: f64| {
            click.set_state(EventSequenceState::Claimed);
        }
    });
    map.add_controller(click);

    scroll.vadjustment().connect_value_changed({
        let map = map.clone();
        move |adj| {
            map.queue_draw();
        }
    });
    let _ = stage
        .bind_property("visible-start-line", &map, "visible-start-line")
        .build();
    let _ = stage
        .bind_property("visible-end-line", &map, "visible-end-line")
        .build();
    map
}

pub fn make_stage(
    sndr: Sender<crate::Event>,
    name: &str,
    scroll: &ScrolledWindow,
) -> (StageView, StageView) {
    let manager = StyleManager::default();
    let is_dark = manager.is_dark();

    let stage = StageView::new(false);
    let buffer = stage.buffer();

    stage.set_margin_start(12);
    stage.set_widget_name(name);
    stage.set_margin_end(12);
    stage.set_margin_top(12);
    stage.set_margin_bottom(12);
    stage.set_is_dark(is_dark, true);
    stage.set_monospace(true);
    stage.set_editable(false);

    let map = make_map(&stage, name, is_dark, scroll);
    map.set_buffer(Some(&buffer));

    if is_dark {
        stage.set_css_classes(&[DARK_CLASS]);
        map.set_css_classes(&[DARK_CLASS]);
    } else {
        stage.set_css_classes(&[LIGHT_CLASS]);
        map.set_css_classes(&[LIGHT_CLASS]);
    }

    let buffer = stage.buffer();
    let table = buffer.tag_table();
    let mut pointer: Option<TextTag> = None;
    let mut staged: Option<TextTag> = None;
    let mut unstaged: Option<TextTag> = None;
    let mut file: Option<TextTag> = None;
    let mut hunk: Option<TextTag> = None;

    for tag_name in tags::TEXT_TAGS {
        let table_tag = tags::TxtTag::from_str(tag_name).make_table_tag();
        table.add(&table_tag);
        match tag_name {
            tags::POINTER => {
                pointer.replace(table_tag);
            }
            tags::STAGED => {
                staged.replace(table_tag);
            }
            tags::UNSTAGED => {
                unstaged.replace(table_tag);
            }
            tags::FILE => {
                file.replace(table_tag);
            }
            tags::HUNK => {
                hunk.replace(table_tag);
            }
            _ => {}
        };
    }

    manager.connect_color_scheme_notify({
        let stage = stage.clone();
        move |manager| {
            let is_dark = manager.is_dark();
            let classes = stage.css_classes();
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
            stage.set_css_classes(&new_classes);
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
                (gdk::Key::Tab | gdk::Key::space, _) => {
                    let iter = buffer.iter_at_offset(buffer.cursor_position());
                    sndr.send_blocking(crate::Event::Expand(iter.offset(), iter.line()))
                        .expect("Could not send through channel");
                    return glib::Propagation::Stop;
                }
                (gdk::Key::s | gdk::Key::a | gdk::Key::Return, _) => {
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
    stage.add_controller(key_controller);

    let gesture_controller = GestureDrag::new();
    gesture_controller.connect_drag_update({
        let stage = stage.clone();
        move |_, _, _| {
            stage.set_cursor_highlight(false);
        }
    });
    stage.add_controller(gesture_controller);

    let gesture_controller = GestureClick::new();
    let click_lock: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    gesture_controller.connect_released({
        let sndr = sndr.clone();
        let stage = stage.clone();
        move |gesture, n_clicks, _wx, _wy| {
            gesture.set_state(EventSequenceState::Claimed);
            stage.set_cursor_highlight(true);
            let pos = stage.buffer().cursor_position();
            let iter = stage.buffer().iter_at_offset(pos);
            sndr.send_blocking(crate::Event::Cursor(iter.offset(), iter.line()))
                .expect("Cant send through channel");
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

    stage.add_controller(gesture_controller);

    stage.connect_move_cursor({
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
        let stage = stage.clone();
        move |_c, x, y| {
            let (x, y) = stage.window_to_buffer_coords(TextWindowType::Text, x as i32, y as i32);
            if let Some(iter) = stage.iter_at_location(x, y) {
                if iter.has_tag(&pointer) {
                    stage.set_cursor(Some(&gdk::Cursor::from_name("pointer", None).unwrap()));
                } else {
                    stage.set_cursor(Some(&gdk::Cursor::from_name("text", None).unwrap()));
                }
            }
        }
    });
    stage.add_controller(motion_controller);

    (stage, map)
}

pub fn cursor_to_line_offset(buffer: &TextBuffer, line_offset: i32) {
    let mut iter = buffer.iter_at_offset(buffer.cursor_position());
    iter.backward_line();
    iter.forward_lines(1);
    iter.forward_chars(line_offset);
    buffer.place_cursor(&iter);
}
