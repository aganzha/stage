// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::git::remote::RemoteResponse;

use libadwaita::prelude::*;
use libadwaita::{AlertDialog, MessageDialog, ResponseAppearance};

use gtk4::{
    Box, Label, Orientation, ScrolledWindow, TextView, Widget,
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
                // .min_content_width(800)
                // .min_content_height(600)
                .build();
            scroll.set_child(Some(&txt));
            let bx = Box::builder()
                .hexpand(true)
                .vexpand(true)
                .vexpand_set(true)
                .hexpand_set(true)
                .orientation(Orientation::Vertical)
                .build();
            bx.append(&scroll);
            return Some(bx.into());
        }
        None
    }
}
#[derive(Default, Clone)]
pub struct DangerDialog(pub String, pub String);

impl AlertConversation for DangerDialog {
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

#[derive(Default, Clone)]
pub struct ConfirmDialog(pub String, pub String);
impl AlertConversation for ConfirmDialog {
    fn heading_and_message(&self) -> (String, String) {
        (self.0.to_string(), self.1.to_string())
    }
    fn get_response(&self) -> Vec<(&str, &str, ResponseAppearance)> {
        vec![
            (NO, NO, ResponseAppearance::Default),
            (YES, YES, ResponseAppearance::Suggested),
        ]
    }
}

pub fn alert<AC>(mut conversation: AC) -> AlertDialog
where
    AC: AlertConversation,
{
    let (heading, message) = conversation.heading_and_message();
    // let body_label = Label::builder()
    //     .label(message)
    //     .build();
    let dialog = AlertDialog::builder()
        .heading_use_markup(true)
        .heading(heading)
        .width_request(640)
        .body_use_markup(true)
        .body(message);
    let dialog = dialog.build();
    if let Some(body) = conversation.extra_child() {
        dialog.set_height_request(480);
        dialog.set_extra_child(Some(&body));
        let parent = body.parent().unwrap();
        let childs = parent.observe_children();
        let body_label = childs.item(1).unwrap();
        let body_label = body_label.downcast_ref::<Label>().unwrap();
        body_label.set_vexpand(false);
    }

    let mut default_response: Option<&str> = None;
    for (id, label, appearance) in conversation.get_response() {
        dialog.add_response(id, label);
        dialog.set_response_appearance(id, appearance);
        default_response.replace(id);
    }
    dialog.set_default_response(default_response);
    dialog
}
