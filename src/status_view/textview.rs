use crate::status_view::headerbar::{Scheme, SCHEME_TOKEN};
use crate::status_view::tags;
use std::array::from_fn;
use async_channel::Sender;
use core::time::Duration;
use glib::ControlFlow;
use gdk::Display;
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, EventControllerKey, EventControllerMotion,
    EventSequenceState, GestureClick, MovementStep, TextTag, TextView,
    TextWindowType, Settings, Widget, Accessible, Buildable
};
use gtk4::subclass::prelude::*;
use libadwaita::prelude::*;
use libadwaita::StyleManager;
use log::{debug, trace};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

glib::wrapper! {
    pub struct StageView(ObjectSubclass<stage_view::StageView>)
        @extends TextView, Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

mod stage_view {
    use gtk4::prelude::*;
    use gtk4::{glib, TextView, TextViewLayer, Snapshot, gdk, graphene,
      //MovementStep//, DeleteType, TextIter, TextExtendSelection, 
    };
    use std::cell::{Cell, RefCell};
    use glib::Properties;
    
    use gtk4::subclass::prelude::*;
    use log::{debug, trace};
    
    // #[derive(Properties, Default)]
    // #[properties(wrapper_type = super::StageView)]

    #[derive(Default)]
    pub struct StageView {

        pub current_line: Cell<(i32, i32)>,
        pub highlight_lines: Cell<(i32, i32)>,
        
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

    impl StageView {
        
    }
    
    impl TextViewImpl for StageView {
        // fn backspace(&self) {
        //     self.parent_backspace()
        // }

        // fn copy_clipboard(&self) {
        //     self.parent_copy_clipboard()
        // }

        // fn cut_clipboard(&self) {
        //     self.parent_cut_clipboard()
        // }

        // fn delete_from_cursor(&self, type_: DeleteType, count: i32) {
        //     self.parent_delete_from_cursor(type_, count)
        // }

        // fn extend_selection(
        //     &self,
        //     granularity: TextExtendSelection,
        //     location: &TextIter,
        //     start: &mut TextIter,
        //     end: &mut TextIter,
        // ) -> glib::ControlFlow {
        //     self.parent_extend_selection(granularity, location, start, end)
        // }

        // fn insert_at_cursor(&self, text: &str) {
        //     self.parent_insert_at_cursor(text)
        // }

        // fn insert_emoji(&self) {
        //     self.parent_insert_emoji()
        // }

        // fn move_cursor(&self, step: MovementStep, count: i32, extend_selection: bool) {
        //     debug!("oooooooooooooooooooooooooooo {:?} {:?}", step, count);
        //     self.parent_move_cursor(step, count, extend_selection)
        // }

        // fn paste_clipboard(&self) {
        //     self.parent_paste_clipboard()
        // }

        // fn set_anchor(&self) {
        //     self.parent_set_anchor()
        // }

        fn snapshot_layer(&self, layer: TextViewLayer, snapshot: Snapshot) {
            if layer == TextViewLayer::BelowText {

                let (y_from, y_to) = self.highlight_lines.get();
                // HARCODE - 2000
                debug!("............... HIGHLIGHT {:?} {:?}", y_from, y_to);
                snapshot.append_color(
                    &gdk::RGBA::new(0.965, 0.961, 0.957, 1.0),                    
                    &graphene::Rect::new(0.0, y_from as f32, 2000.0, y_to as f32)
                );
                
                let (y_from, y_to) = self.current_line.get();
                // HARCODE - 2000
                snapshot.append_color(
                    &gdk::RGBA::new(0.80, 0.87, 0.97, 1.0),                    
                    &graphene::Rect::new(0.0, y_from as f32, 2000.0, y_to as f32)
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

    pub fn set_current_line(&self, line_no: i32) {
        let iter = self.buffer().iter_at_line(line_no).unwrap();
        let range = self.line_yrange(&iter);
        self.imp().current_line.replace(range);
    }

    pub fn set_highlight(&self, from_to: (i32, i32)) {        
        let iter = self.buffer().iter_at_line(from_to.0).unwrap();
        let from_range = self.line_yrange(&iter);   
        debug!("oixxxxxxxxxxxels {:?} {:?}", from_range, from_to);
        self.imp().highlight_lines.replace((from_range.0, from_range.1 * (from_to.1 - from_to.0)));
    }
    
}

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
    text_view_width: Rc<RefCell<crate::context::TextViewWidth>>,
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
            let mut new_classes = classes.iter().map(|gs| gs.as_str()).filter(|s| {
                if is_dark {
                    s != &LIGHT_CLASS
                } else {
                    s != &DARK_CLASS
                }
            }).collect::<Vec<&str>>();
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
        }});
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
                        debug!("text_view_width is changed! {:?} {:?}", text_view_width, visible_char_width);
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
