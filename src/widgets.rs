use libadwaita::prelude::*;
use libadwaita::{HeaderBar, MessageDialog, ResponseAppearance, Window};
use std::ffi::OsString;
// use glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;

use gtk4::{
    glib, AlertDialog, Button, EventControllerKey, TextView, Widget,
    Window as Gtk4Window,
};

pub fn display_error(
    w: &impl IsA<Gtk4Window>, // Application
    message: &str,
) {
    let d = AlertDialog::builder().message(message).build();
    d.show(Some(w));
}

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

pub fn make_header_bar(sender: Sender<crate::Event>) -> HeaderBar {
    let stashes_btn = Button::builder()
        .label("Stashes")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Stashes")
        .icon_name("sidebar-show-symbolic")
        .can_shrink(true)
        .build();
    stashes_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::StashesPanel)
                .expect("cant send through channel");
        }
    });
    let refresh_btn = Button::builder()
        .label("Refresh")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Refresh")
        .icon_name("view-refresh-symbolic")
        .can_shrink(true)
        .build();
    refresh_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Refresh)
                .expect("Could not send through channel");
        }
    });
    let push_btn = Button::builder()
        .label("Push")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Push")
        .icon_name("document-send-symbolic")
        .can_shrink(true)
        .build();
    push_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Push)
                .expect("cant send through channel");
        }
    });
    let pull_btn = Button::builder()
        .label("Pull")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Pull")
        .icon_name("document-save-symbolic")
        .can_shrink(true)
        .build();
    pull_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Pull)
                .expect("cant send through channel");
        }
    });
    let hb = HeaderBar::new();
    hb.pack_start(&stashes_btn);
    hb.pack_start(&refresh_btn);
    hb.pack_end(&push_btn);
    hb.pack_end(&pull_btn);
    hb
}
