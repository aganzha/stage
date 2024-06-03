use crate::git::remote::RemoteResponse;
use async_channel::Sender;
use libadwaita::prelude::*;
use libadwaita::{AlertDialog, MessageDialog, ResponseAppearance, SwitchRow};

use std::collections::HashMap;

// use glib::Sender;
// use std::sync::mpsc::Sender;

use gtk4::{
    ListBox, ScrolledWindow, SelectionMode, TextView, Widget,
    Window as Gtk4Window,
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

pub const YES: &str = "yes";
pub const NO: &str = "no";
const CLOSE: &str = "close";

pub trait AlertConversation {
    fn heading_and_message(&self) -> (String, String);

    fn extra_child(&mut self) -> Option<Widget> {
        None
    }
    fn get_response(&self) -> Vec<(&str, &str, ResponseAppearance)> {
        vec![(CLOSE, CLOSE, ResponseAppearance::Destructive)]
    }
}

impl AlertConversation for git2::Error {
    fn heading_and_message(&self) -> (String, String) {
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
impl AlertConversation for String {
    fn heading_and_message(&self) -> (String, String) {
        (
            String::from("<span color=\"#ff0000\">Error</span>"),
            String::from(self),
        )
    }
}
impl AlertConversation for RemoteResponse {
    fn heading_and_message(&self) -> (String, String) {
        (
            String::from("<span color=\"#ff0000\">Error</span>"),
            self.error.clone().unwrap().clone(),
        )
    }
    fn extra_child(&mut self) -> Option<Widget> {
        if let Some(body) = &self.body {
            let txt = TextView::builder()
                .margin_start(12)
                .margin_end(12)
                .margin_top(12)
                .margin_bottom(12)
                .build();
            let buffer = txt.buffer();
            let mut iter = buffer.iter_at_offset(0);
            let body: String =
                body.iter().fold("".to_string(), |cur, nxt| cur + nxt);
            buffer.insert(&mut iter, &body);

            let scroll = ScrolledWindow::builder()
                .vexpand(true)
                .vexpand_set(true)
                .hexpand(true)
                .hexpand_set(true)
                .min_content_width(800)
                .min_content_height(600)
                .build();
            scroll.set_child(Some(&txt));
            return Some(scroll.into());
        }
        None
    }
}
#[derive(Default, Clone)]
pub struct YesNoString(pub String, pub String);

impl AlertConversation for YesNoString {
    fn heading_and_message(&self) -> (String, String) {
        (
            format!("<span color=\"#ff0000\">{}</span>", self.0),
            self.1.to_string(),
        )
    }
    fn get_response(&self) -> Vec<(&str, &str, ResponseAppearance)> {
        vec![
            (NO, NO, ResponseAppearance::Default),
            (YES, YES, ResponseAppearance::Destructive),
        ]
    }
}

// TODO kill that. switch to confirmation dialog instead!
pub struct YesNoWithVariants(pub YesNoString, pub HashMap<String, bool>);

impl AlertConversation for YesNoWithVariants {
    fn heading_and_message(&self) -> (String, String) {
        self.0.heading_and_message()
    }
    fn get_response(&self) -> Vec<(&str, &str, ResponseAppearance)> {
        self.0.get_response()
    }
    fn extra_child(&mut self) -> Option<Widget> {
        let lb = ListBox::builder()
            .selection_mode(SelectionMode::None)
            .css_classes(vec![String::from("boxed-list")])
            .build();
        let kv = self.1.clone();
        for (key, value) in &kv {
            let row = SwitchRow::builder()
                .title(key)
                .css_classes(vec!["input_field"])
                .active(*value)
                .build();
            // row.bind_property("selected", &model, "selected_pos");
            // row.connect_active_notify(|sw_row| {
            //     // self.1.insert(key.to_string(), sw_row.is_active());
            //     debug!("-------------------> {:?}", self.1);
            // });
            lb.append(&row);
        }
        Some(lb.into())
    }
}

pub fn alert<AC>(mut conversation: AC) -> AlertDialog
where
    AC: AlertConversation,
{
    let (heading, message) = conversation.heading_and_message();
    let mut dialog = AlertDialog::builder()
        .heading_use_markup(true)
        .heading(heading)
        .width_request(640)
        .body_use_markup(true)
        .body(message);
    if let Some(body) = conversation.extra_child() {
        dialog = dialog.extra_child(&body);
    }
    let dialog = dialog.build();
    for (id, label, appearance) in conversation.get_response() {
        dialog.add_response(id, label);
        dialog.set_response_appearance(id, appearance);
    }
    dialog
}
