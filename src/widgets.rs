use libadwaita::prelude::*;
use libadwaita::{MessageDialog, ResponseAppearance, Window};
// use glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;

use gtk4::{
    glib, AlertDialog, EventControllerKey, TextView, Widget,
    Window as Gtk4Window,
};

pub fn display_error(
    w: &impl IsA<Gtk4Window>, // Application
    message: &str,
) {
    let d = AlertDialog::builder().message(message).build();
    d.show(Some(w));
}

// pub fn get_new_branch_name(
//     window: &Window,
//     _current_branch: &crate::BranchData,
//     sndr: Sender<crate::BranchesEvent>,
// ) {
//     let txt = TextView::builder()
//         .monospace(true)
//         .css_classes(["commit_message"])
//         .build();
//     let cancel_response = "cancel";
//     let create_response = "create";

//     let dialog = MessageDialog::builder()
//         .heading("New branch")
//         .transient_for(window)
//         .modal(true)
//         .destroy_with_parent(true)
//         .close_response(cancel_response)
//         .default_response(create_response)
//         .extra_child(&txt)
//         .default_width(640)
//         .default_height(120)
//         .build();
//     dialog.add_responses(&[
//         (cancel_response, "Cancel"),
//         (create_response, "Create"),
//     ]);
//     // Make the dialog button insensitive initially
//     dialog.set_response_enabled(create_response, false);
//     dialog.set_response_appearance(
//         create_response,
//         ResponseAppearance::Suggested,
//     );

//     let event_controller = EventControllerKey::new();
//     event_controller.connect_key_pressed({
//         let dialog = dialog.clone();
//         move |_, _, _, _| {
//             dialog.set_response_enabled(create_response, true);
//             glib::Propagation::Proceed
//         }
//     });
//     txt.add_controller(event_controller);
//     // Connect response to dialog
//     dialog.connect_response(None, move |dialog, response| {
//         // clone!(@weak window, @weak entry =>
//         // Destroy dialog
//         dialog.destroy();

//         // Return if the user chose a response different than `create_response`
//         if response != create_response {
//             println!("return new branch name");
//             return;
//         }
//         let buffer = txt.buffer();
//         let start = buffer.iter_at_offset(0);
//         let end = buffer.end_iter();
//         let new_branch_name = buffer.slice(&start, &end, false);
//         // clbk(new_branch_name.to_string());
//         sndr.send_blocking(crate::BranchesEvent::NewBranch(
//             new_branch_name.to_string(),
//         ))
//         .expect("cant send through channel");
//     });
//     dialog.present();
// }

pub fn make_confirm_dialog(
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
