// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use super::Status;
use crate::dialogs::alert;
use crate::git::remote;
use crate::Event;
use async_channel::Sender;
use gtk4::{gio, glib, Button, ListBox, SelectionMode, StringList};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, ComboRow, EntryRow, PasswordEntryRow, PreferencesDialog, PreferencesGroup,
    PreferencesPage, SwitchRow,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

impl remote::RemoteDetail {
    fn render(
        &self,
        page: &PreferencesPage,
        path: &PathBuf,
        window: &ApplicationWindow,
        sender: &Sender<Event>,
    ) -> PreferencesGroup {
        let remote_name = Rc::new(RefCell::new(self.name.clone()));

        let del_button = Button::builder().icon_name("user-trash-symbolic").build();
        let group = PreferencesGroup::builder()
            .title(&self.name)
            .header_suffix(&del_button)
            .build();
        del_button.connect_clicked({
            let path = path.clone();
            let sender = sender.clone();
            let window = window.clone();
            let group = group.clone();
            let page = page.clone();
            let remote_name = remote_name.clone();
            move |_| {
                glib::spawn_future_local({
                    let path = path.clone();
                    let sender = sender.clone();
                    let window = window.clone();
                    let group = group.clone();
                    let page = page.clone();
                    let remote_name = remote_name.clone();
                    async move {
                        let remote_name = (*(remote_name.borrow())).clone();
                        let deleted =
                            gio::spawn_blocking(move || remote::delete(path, remote_name, sender))
                                .await
                                .unwrap_or_else(|e| {
                                    alert(format!("{:?}", e)).present(Some(&window));
                                    Ok(false)
                                })
                                .unwrap_or_else(|e| {
                                    alert(e).present(Some(&window));
                                    false
                                });
                        if deleted {
                            page.remove(&group);
                        }
                    }
                });
            }
        });
        let row = EntryRow::builder()
            .title("Name")
            .text(&self.name)
            .show_apply_button(true)
            .build();
        row.connect_apply({
            let remote = self.clone();
            let path = path.clone();
            let window = window.clone();
            let remote_name = remote_name.clone();
            let group = group.clone();
            move |row| {
                let mut remote = remote.clone();
                remote.name = row.text().to_string();
                glib::spawn_future_local({
                    let path = path.clone();
                    let window = window.clone();
                    let remote_name = remote_name.clone();
                    let group = group.clone();
                    async move {
                        let remote_to_edit = (*(remote_name.borrow())).clone();
                        let new_remote = gio::spawn_blocking({
                            let path = path.clone();
                            move || remote::edit(path, remote_to_edit, remote)
                        })
                        .await
                        .unwrap_or_else(|e| {
                            alert(format!("{:?}", e)).present(Some(&window));
                            Ok(None)
                        })
                        .unwrap_or_else(|e| {
                            alert(e).present(Some(&window));
                            None
                        });
                        if let Some(remote) = new_remote {
                            group.set_title(&remote.name);
                            remote_name.replace(remote.name);
                        }
                    }
                });
            }
        });

        group.add(&row);
        let row = EntryRow::builder()
            .title("Url")
            .text(&self.url)
            .show_apply_button(true)
            .build();
        row.connect_apply({
            let remote = self.clone();
            let path = path.clone();
            let window = window.clone();
            let remote_name = remote_name.clone();
            move |row| {
                let mut edited = remote.clone();
                edited.url = row.text().to_string();
                glib::spawn_future_local({
                    let path = path.clone();
                    let window = window.clone();
                    let remote_name = remote_name.clone();
                    async move {
                        gio::spawn_blocking({
                            let path = path.clone();
                            let remote_to_edit = (*(remote_name.borrow())).clone();
                            move || remote::edit(path, remote_to_edit, edited)
                        })
                        .await
                        .unwrap_or_else(|e| {
                            alert(format!("{:?}", e)).present(Some(&window));
                            Ok(None)
                        })
                        .unwrap_or_else(|e| {
                            alert(e).present(Some(&window));
                            None
                        });
                    }
                });
            }
        });
        group.add(&row);
        // for refspec in &self.refspecs {
        //     let row = EntryRow::builder()
        //         .title("Refspec")
        //         .text(refspec)
        //         .editable(false)
        //         .show_apply_button(false)
        //         .build();
        //     group.add(&row);
        // }
        // let row = EntryRow::builder()
        //     .title("Push url")
        //     .text(&remote.push_url)
        //     .show_apply_button(true)
        //     .build();
        // group.add(&row);
        // for refspec in &remote.push_refspecs {
        //     let row = EntryRow::builder()
        //         .title("Push refspec")
        //         .text(refspec)
        //         .show_apply_button(true)
        //         .build();
        //     group.add(&row);
        // }
        group
    }
}

fn remote_adding(
    page: &PreferencesPage,
    path: &PathBuf,
    window: &ApplicationWindow,
    sender: &Sender<Event>,
) -> PreferencesGroup {
    let add_button = Button::builder().icon_name("list-add-symbolic").build();
    let adding = PreferencesGroup::builder()
        .title("New remote")
        .header_suffix(&add_button)
        .build();
    let adding_name = EntryRow::builder()
        .title("Name")
        .show_apply_button(false)
        .build();
    adding.add(&adding_name);
    let adding_url = EntryRow::builder()
        .title("Url")
        .show_apply_button(false)
        .build();
    adding.add(&adding_url);
    add_button.connect_clicked({
        let path = path.clone();
        let sender = sender.clone();
        let window = window.clone();
        let page = page.clone();
        let adding = adding.clone();
        move |_| {
            let name = adding_name.text();
            let url = adding_url.text();
            if name.len() > 0 && url.len() > 0 {
                glib::spawn_future_local({
                    let path = path.clone();
                    let sender = sender.clone();
                    let window = window.clone();
                    let page = page.clone();
                    let adding = adding.clone();
                    async move {
                        let remote = gio::spawn_blocking({
                            let path = path.clone();
                            let sender = sender.clone();
                            move || remote::add(path, name.to_string(), url.to_string(), sender)
                        })
                        .await
                        .unwrap_or_else(|e| {
                            alert(format!("{:?}", e)).present(Some(&window));
                            Ok(None)
                        })
                        .unwrap_or_else(|e| {
                            alert(e).present(Some(&window));
                            None
                        });
                        if let Some(remote) = remote {
                            page.remove(&adding);
                            page.add(&remote.render(&page, &path, &window, &sender));
                            page.add(&remote_adding(&page, &path, &window, &sender));
                        }
                    }
                });
            }
        }
    });
    adding
}

impl Status {
    pub fn push(&self, window: &ApplicationWindow, remote_dialog: Option<(String, bool, bool)>) {
        glib::spawn_future_local({
            let window = window.clone();
            let path = self.path.clone().unwrap();
            let sender = self.sender.clone();
            let remote_branch_name = self.choose_remote_branch_name();
            async move {
                let lb = ListBox::builder()
                    .selection_mode(SelectionMode::None)
                    .css_classes(vec![String::from("boxed-list")])
                    .build();

                let remotes = gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || remote::list(path, sender)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(Some(&window));
                    Ok(Vec::new())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(Some(&window));
                    Vec::new()
                });
                let remotes_list = StringList::new(&[]);
                for remote in &remotes {
                    remotes_list.append(&remote.name);
                }

                let remotes = ComboRow::builder()
                    .title("Remote")
                    .model(&remotes_list)
                    .build();
                let upstream = SwitchRow::builder()
                    .title("Set upstream")
                    .css_classes(vec!["input_field"])
                    .active(true)
                    .build();

                let remote_branch_name = EntryRow::builder()
                    .title("Remote branch name:")
                    .show_apply_button(false)
                    .css_classes(vec!["input_field"])
                    .text(remote_branch_name)
                    .build();

                let user_name = EntryRow::builder()
                    .title("User name:")
                    .show_apply_button(true)
                    .css_classes(vec!["input_field"])
                    .build();
                let password = PasswordEntryRow::builder()
                    .title("Password:")
                    .css_classes(vec!["input_field"])
                    .build();

                let dialog = crate::confirm_dialog_factory(
                    Some(&lb),
                    "Push to remote", // TODO here is harcode
                    "Push",
                );
                dialog.connect_realize({
                    let remote_branch_name = remote_branch_name.clone();
                    move |_| {
                        remote_branch_name.grab_focus();
                    }
                });

                let enter_pressed = Rc::new(Cell::new(false));

                remote_branch_name.connect_apply({
                    let dialog = dialog.clone();
                    let enter_pressed = enter_pressed.clone();
                    move |_| {
                        // someone pressed enter
                        enter_pressed.replace(true);
                        dialog.close();
                    }
                });

                remote_branch_name.connect_entry_activated({
                    let dialog = dialog.clone();
                    let enter_pressed = enter_pressed.clone();
                    move |_| {
                        // someone pressed enter
                        enter_pressed.replace(true);
                        dialog.close();
                    }
                });
                let mut pass = false;
                let mut this_is_tag = false;
                match remote_dialog {
                    None => {
                        lb.append(&remotes);
                        lb.append(&remote_branch_name);
                        lb.append(&upstream);
                    }
                    Some((remote_branch, track_remote, is_tag)) => {
                        this_is_tag = is_tag;
                        remote_branch_name.set_text(&remote_branch);
                        if track_remote {
                            upstream.set_active(true);
                        }
                        lb.append(&user_name);
                        lb.append(&password);
                        pass = true;
                    }
                }

                let response = dialog.choose_future(&window).await;

                if !("confirm" == response || enter_pressed.get()) {
                    sender
                        .send_blocking(crate::Event::UpstreamProgress)
                        .expect("Could not send through channel");
                    return;
                }
                let remote_branch_name = format!("{}", remote_branch_name.text());
                let track_remote = upstream.is_active();
                let mut user_pass: Option<(String, String)> = None;
                if pass {
                    user_pass.replace((
                        format!("{}", user_name.text()),
                        format!("{}", password.text()),
                    ));
                }
                glib::spawn_future_local({
                    async move {
                        gio::spawn_blocking({
                            let sender = sender.clone();
                            move || {
                                remote::push(
                                    path,
                                    remote_branch_name,
                                    track_remote,
                                    this_is_tag,
                                    sender,
                                    user_pass,
                                )
                            }
                        })
                        .await
                        .unwrap_or_else(|e| {
                            sender
                                .send_blocking(crate::Event::UpstreamProgress)
                                .expect("Could not send through channel");
                            alert(format!("{:?}", e)).present(Some(&window));
                            Ok(())
                        })
                        .unwrap_or_else(|e| {
                            sender
                                .send_blocking(crate::Event::UpstreamProgress)
                                .expect("Could not send through channel");
                            alert(e).present(Some(&window));
                        });
                    }
                });
            }
        });
    }

    fn choose_remote_branch_name(&self) -> String {
        if let Some(upstream) = &self.upstream {
            if let Some(name) = &upstream.branch_name {
                return name.to_string()
            }
        }
        if let Some(head) = &self.head {
            if let Some(name) = &head.branch_name {
                return name.to_string()
            }
        }
        "".to_string()
    }

    pub fn show_remotes_dialog(&self, window: &ApplicationWindow) {
        let window = window.clone();
        let sender = self.sender.clone();
        let path = self.path.clone().unwrap();

        glib::spawn_future_local({
            async move {
                let remotes = gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || remote::list(path, sender)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(Some(&window));
                    Ok(Vec::new())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(Some(&window));
                    Vec::new()
                });

                let dialog = PreferencesDialog::builder()
                    // when here will be more then one page
                    // remove .title and it will display Preferences
                    // and Remotes title will be moved to page tab
                    .title("Remotes")
                    .build();
                let page = PreferencesPage::builder()
                    .title("Remotes")
                    .icon_name("network-server-symbolic")
                    .build();
                for remote in &remotes {
                    let group = remote.render(&page, &path, &window, &sender);
                    page.add(&group);
                }

                let adding = remote_adding(&page, &path, &window, &sender);

                page.add(&adding);
                dialog.add(&page);
                dialog.present(Some(&window));
            }
        });
    }
}
