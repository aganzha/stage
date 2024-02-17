use libadwaita::prelude::*;
use gtk4::prelude::*;
use libadwaita::{
    ApplicationWindow, Window, HeaderBar, ToolbarView
};
use gtk4::{
    gio, glib, gdk,
    ScrolledWindow, EventControllerKey    
};
use glib::{clone};
use log::{debug, error, info, log_enabled, trace};


pub fn show_branches_window(app_window: &ApplicationWindow) {
    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    let hb = HeaderBar::builder()
        .build();

    let scroll = ScrolledWindow::new();
    let tb = ToolbarView::builder()
        .content(&scroll)
        .build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));


    let event_controller =
        EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                _ => {
                    debug!("some other pressed");
                }
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);
    window.present();

    // let action_close =
    //     gio::SimpleAction::new("close", None);
    // action_close.connect_activate(
    //     clone!(@weak window => move |_, _| {
    //         window.close();
    //     }),
    // );
    // window.insert_action_group();
    // window.add_action(&action_close);
    // window.action_set_enabled("close", true);
    // window.action_set_enabled("win.close", true);
    // // window.set_accels_for_action(
    // //     "win.close",
    // //     &["<Ctrl>W"],
    // // );
    // window.present();
    // window.action_name();
    // let some = window.activate_action("win.close", None);
    // debug!("action {:?}", some);
    // let some = window.activate_action("close", None);
    // debug!("action 1 {:?}", some);
}
