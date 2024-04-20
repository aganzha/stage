use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use libadwaita::prelude::*;

use log::{debug, info, trace};

use crate::Event;
use async_channel::Sender;
use gtk4::{gdk, gio, glib, pango, EventControllerKey, Label, ScrolledWindow};
use libadwaita::{ApplicationWindow, HeaderBar, ToolbarView, Window};

pub fn make_headerbar(
    _repo_path: std::ffi::OsString,
    sender: Sender<Event>,
) -> HeaderBar {
    let hb = HeaderBar::builder().build();
    let lbl = Label::builder()
        .label("Oid view")
        .single_line_mode(true)
        .build();

    hb.set_title_widget(Some(&lbl));
    hb.set_show_end_title_buttons(true);
    hb.set_show_back_button(true);
    hb
}

pub fn show_oid_window(
    repo_path: std::ffi::OsString,
    app_window: &ApplicationWindow,
    main_sender: Sender<crate::Event>,
) {
    let (sender, receiver) = async_channel::unbounded();

    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let scroll = ScrolledWindow::new();

    // let list_view = make_list_view(repo_path.clone(), main_sender.clone());

    let hb = make_headerbar(repo_path.clone(), sender.clone());

    // scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        let sender = sender.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::Escape, _) => {
                    window.close();
                }
                (gdk::Key::a, _) => {
                    debug!("key pressed {:?} {:?}", key, modifier);
                    // sender
                    //     .send_blocking(Event::CherryPickRequest)
                    //     .expect("Could not send through channel");
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);
    window.present();
}
