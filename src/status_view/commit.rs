use crate::dialogs::{alert, DangerDialog, YES};
use crate::{git::commit as git_commit, Event};
use std::cell::RefCell;
use async_channel::Sender;
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Box, EventControllerKey, Label as GtkLabel, ListBox,
    Orientation, ScrolledWindow, SelectionMode, TextBuffer, TextView, Widget,
    WrapMode,
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, Banner, EntryRow, PasswordEntryRow, SwitchRow,
};
use log::{debug, trace};
use std::path::PathBuf;

pub fn commit(
    path: Option<PathBuf>,
    amend_message: Option<String>,
    window: &ApplicationWindow,
    sender: Sender<Event>,
) {
    glib::spawn_future_local({
        let window = window.clone();
        let sender = sender.clone();
        let path = path.clone();
        async move {
            let list_box = ListBox::builder()
                .selection_mode(SelectionMode::None)
                .css_classes(vec![String::from("boxed-list")])
                .build();
            let commit_message = EntryRow::builder()
                .title("commit message")
                .show_apply_button(true)
                .css_classes(vec!["input_field"])
                .text("")
                .build();

            let amend_switch = SwitchRow::builder()
                .title("amend")
                .css_classes(vec!["input_field"])
                .active(false)
                .build();

            list_box.append(&commit_message);
            if amend_message.is_some() || true {
                list_box.append(&amend_switch);
            }

            let txt = TextView::builder()
                .margin_start(12)
                .margin_end(12)
                .margin_top(12)
                .margin_bottom(12)
                .wrap_mode(WrapMode::Word)
                .build();
            let scroll = ScrolledWindow::builder()
                .vexpand(true)
                .vexpand_set(true)
                .hexpand(true)
                .visible(false)
                .hexpand_set(true)
                .min_content_width(480)
                .min_content_height(320)
                .build();

            commit_message.connect_apply({
                let txt = txt.clone();
                let scroll = scroll.clone();
                move |entry: &EntryRow| {
                    let mut iter = txt.buffer().iter_at_offset(0);
                    if !entry.text().is_empty() {
                        txt.buffer().insert(&mut iter, &entry.text());
                        txt.buffer().insert(&mut iter, "\n");
                    }
                    entry.set_visible(false);
                    scroll.set_visible(true);
                    txt.grab_focus();
                    txt.buffer().place_cursor(&mut iter);
                }
            });
            
            amend_switch.connect_active_notify({
                let txt = txt.clone();
                let scroll = scroll.clone();
                let entry = commit_message.clone();
                let amend_inserted = RefCell::new(false);
                move |_| {
                    if !scroll.get_visible() {
                        // force text view
                        // on any toggle
                        // amend is not inserted for sure
                        let mut iter = txt.buffer().iter_at_offset(0);
                        if !entry.text().is_empty() {
                            txt.buffer().insert(&mut iter, &entry.text());
                            txt.buffer().insert(&mut iter, "\n");
                        }
                        txt.buffer().insert(&mut iter, &amend_message.clone().unwrap());
                        entry.set_visible(false);
                        scroll.set_visible(true);
                        *amend_inserted.borrow_mut() = true;
                        // no need to put cursor
                        // lets proceed straight to commit
                        // txt.grab_focus();
                        // txt.buffer().place_cursor(&mut iter);
                    } else {
                        // how do we know if amend message was already inserted???
                        if !(*amend_inserted.borrow()) {
                            debug!("insert text");
                            let mut iter = txt.buffer().end_iter();
                            txt.buffer().insert(&mut iter, &amend_message.clone().unwrap());
                            *amend_inserted.borrow_mut() = true;
                        } else {
                            debug!("noooooooooooooo way");
                        }
                    }
                }
            });

            scroll.set_child(Some(&txt));

            let text_view_box = Box::builder()
                .hexpand(true)
                .vexpand(true)
                .vexpand_set(true)
                .hexpand_set(true)
                .orientation(Orientation::Vertical)
                .build();

            text_view_box.append(&scroll);
            text_view_box.append(&list_box);

            let dialog = crate::confirm_dialog_factory(
                &window,
                Some(&text_view_box),
                "Commit",
                "Commit",
            );

            let response = dialog.choose_future().await;
            if "confirm" != response {
                return;
            }

            gio::spawn_blocking({
                // let message = format!("{}", input.text());
                let message = {
                    if scroll.get_visible() {
                        let buffer = txt.buffer();
                        let start_iter = buffer.iter_at_offset(0);
                        let eof_iter = buffer.end_iter();
                        buffer
                            .text(&start_iter, &eof_iter, true)
                            .to_string()
                            .to_string()
                    } else {
                        commit_message.text().to_string()
                    }
                };

                let amend = amend_switch.is_active();
                move || {
                    git_commit::create_commit(
                        path.expect("no path"),
                        message,
                        amend,
                        sender,
                    )
                }
            })
            .await
            .unwrap_or_else(|e| {
                alert(format!("{:?}", e)).present(&window);
                Ok(())
            })
            .unwrap_or_else(|e| {
                alert(e).present(&window);
            });
        }
    });
}