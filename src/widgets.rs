use adw::prelude::*;
use adw::{ApplicationWindow, MessageDialog, ResponseAppearance};
use glib::Sender;
use gtk::prelude::*;
use gtk::{glib, TextView};

pub fn display_error(w: &ApplicationWindow, message: &str) {
    let d = gtk::AlertDialog::builder().message(message).build();
    d.show(Some(w));
}

pub fn show_commit_message(window: &ApplicationWindow, sndr: Sender<crate::Event>) {
    let txt = TextView::builder()
        .monospace(true)
        .css_classes(["commit_message"])
        .build();
    let cancel_response = "cancel";
    let create_response = "create";

    let dialog = MessageDialog::builder()
        .heading("Commit")
        .transient_for(window)
        .modal(true)
        .destroy_with_parent(true)
        .close_response(cancel_response)
        .default_response(create_response)
        .extra_child(&txt)
        .default_width(640)
        .default_height(120)
        .build();
    dialog.add_responses(&[(cancel_response, "Cancel"), (create_response, "Create")]);
    // Make the dialog button insensitive initially
    dialog.set_response_enabled(create_response, false);
    dialog.set_response_appearance(create_response, ResponseAppearance::Suggested);

    let event_controller = gtk::EventControllerKey::new();
    event_controller.connect_key_pressed({
        let dialog = dialog.clone();
        move |_, _, _, _| {
            dialog.set_response_enabled(create_response, true);
            glib::Propagation::Proceed
        }
    });
    txt.add_controller(event_controller);
    // Connect response to dialog
    dialog.connect_response(None, move |dialog, response| {
        // clone!(@weak window, @weak entry =>
        // Destroy dialog
        dialog.destroy();

        // Return if the user chose a response different than `create_response`
        if response != create_response {
            println!("return from commit dialog");
            return;
        }
        let buffer = txt.buffer();
        let start = buffer.iter_at_offset(0);
        let end = buffer.end_iter();
        let message = buffer.slice(&start, &end, false);
        sndr.send(crate::Event::Commit(message.to_string()))
            .expect("cant send through channel");
    });
    dialog.present();
}