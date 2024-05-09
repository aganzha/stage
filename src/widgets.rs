use async_channel::Sender;
use libadwaita::prelude::*;
use libadwaita::{
    MessageDialog, ResponseAppearance,
};
use log::{debug};

// use glib::Sender;
// use std::sync::mpsc::Sender;


use gtk4::{
    AlertDialog, Widget,
    Window as Gtk4Window,
};

pub fn display_error(
    w: &impl IsA<Gtk4Window>, // Application
    message: &str,
) {
    let d = AlertDialog::builder().message(message).build();
    d.show(Some(w));
}

pub fn confirm_dialog_factory(
    window: &impl IsA<Gtk4Window>,
    child: Option<&impl IsA<Widget>>,
    heading: &str,
    confirm_title: &str,
) -> MessageDialog {
    let cancel_response = "cancel";
    let confirm_response = "confirm";

    let dialog = MessageDialog::builder()
        .heading(heading)
        .transient_for(window)
        .modal(true)
        .destroy_with_parent(true)
        .close_response(cancel_response)
        .default_response(confirm_response)
        .default_width(720)
        .default_height(120)
        .build();

    dialog.set_extra_child(child);
    dialog.add_responses(&[
        (cancel_response, "Cancel"),
        (confirm_response, confirm_title),
    ]);

    dialog.set_response_appearance(
        confirm_response,
        ResponseAppearance::Suggested,
    );
    dialog
}


pub fn merge_dialog_factory(_window: &impl IsA<Gtk4Window>, _sender: Sender<crate::Event>) {
    debug!("cliiiiiiiiiiiiiiiick");
}
