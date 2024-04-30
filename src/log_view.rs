use async_channel::Sender;
use glib::{clone, closure, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, AlertDialog, Box, Button, EventControllerKey,
    Image, Label, ListBox, ListHeader, ListItem, ListScrollFlags, ListView,
    Orientation, ScrolledWindow, SearchBar, SearchEntry, SectionModel,
    SelectionMode, SignalListItemFactory, SingleSelection, Spinner, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, EntryRow, HeaderBar, SwitchRow, ToolbarView, Window,
};
use log::{debug, trace};


pub fn show_log_window(
    repo_path: std::ffi::OsString,
    app_window: &ApplicationWindow,
    head: String,
    main_sender: Sender<crate::Event>,
) {
    // let (sender, receiver) = async_channel::unbounded();
    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);
    let scroll = ScrolledWindow::new();
    let tb = ToolbarView::builder().content(&scroll).build();

    let title = Label::builder()
        .label(head)
        .build();
    let hb = HeaderBar::builder().build();
    hb.set_title_widget(Some(&title));
    
    tb.add_top_bar(&hb);
    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        // let sender = sender.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::Escape, _) => {
                    window.close();
                }                
                (key, modifier) => {
                    trace!("key pressed {:?} {:?}", key, modifier);
                }
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);

    window.present();
}
