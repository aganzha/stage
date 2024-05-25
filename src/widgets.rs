use async_channel::Sender;
use libadwaita::prelude::*;
use libadwaita::{AlertDialog, MessageDialog, ResponseAppearance};

// use glib::Sender;
// use std::sync::mpsc::Sender;

use gtk4::{
    gio, AlertDialog as GTK4AlertDialog, Widget, Window as Gtk4Window,
};


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

pub const OURS: &str = "ours";
pub const THEIRS: &str = "theirs";
pub const ABORT: &str = "abort";
pub const PROCEED: &str = "proceed";

pub fn merge_dialog_factory(
    window: &impl IsA<Gtk4Window>,
    _sender: Sender<crate::Event>,
) -> MessageDialog {
    // let abort = "abort";
    // let merge_ours = "ours";
    // let merge_theirs = "theirs";
    // let proceed = "proceed";
    let body = "Conflicts during merging. You can Abort merge, choose Our side, Their side or proceed with resolving conflicts ";
    let dialog = MessageDialog::builder()
        .heading("Conflicts during merge")
        .transient_for(window)
        .modal(true)
        .destroy_with_parent(true)
        .default_width(720)
        .default_height(120)
        .body(body)
        .build();

    dialog.add_responses(&[
        (ABORT, "Abort"),
        (OURS, "Ours"),
        (THEIRS, "Theirs"),
        (PROCEED, "Proceed"),
    ]);

    dialog.set_response_appearance(PROCEED, ResponseAppearance::Suggested);
    dialog.set_response_appearance(ABORT, ResponseAppearance::Destructive);
    dialog
}

pub trait AlertError {
    fn error(&self) -> (String, String);
}

impl AlertError for git2::Error {
    fn error(&self) -> (String, String) {
        (
            String::from("<span color=\"#ff0000\">Git error</span>"),
            format!(
                "class: {:?}\ncode: {:?}\n{}",
                self.class(),
                self.code(),
                self.message()
            ),
        )
    }
}
impl AlertError for String {
    fn error(&self) -> (String, String) {
        (
            String::from("<span color=\"#ff0000\">Error</span>"),
            String::from(self),
        )
    }
}

pub fn alert<E>(err: E, window: &impl IsA<Widget>)
where
    E: AlertError,
{
    let (heading, body) = err.error();
    let dialog = AlertDialog::builder()
        .heading_use_markup(true)
        .heading(heading)
        .body_use_markup(true)
        .body(body)
        .build();
    dialog.add_response("close", "close");
    dialog.set_response_appearance("close", ResponseAppearance::Destructive);
    dialog.choose(window, None::<&gio::Cancellable>, |_response| {});
}
