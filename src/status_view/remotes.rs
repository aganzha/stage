// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use super::Status;
use crate::dialogs::alert;
use crate::git::remote;
use crate::Event;
use async_channel::Sender;
use gtk4::{gio, glib, Button};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, EntryRow, PreferencesDialog, PreferencesGroup, PreferencesPage,
};
use log::{debug, trace};

impl Status {
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
                    let del_button = Button::builder().icon_name("user-trash-symbolic").build();
                    del_button.connect_clicked({
                        let path = path.clone();
                        let sender = sender.clone();
                        let window = window.clone();
                        let name = remote.name.clone();
                        move |_| {
                            glib::spawn_future_local({
                                let path = path.clone();
                                let sender = sender.clone();
                                let window = window.clone();
                                let name = name.clone();
                                async move {
                                    let deleted = gio::spawn_blocking(move || {
                                        remote::delete(path, name, sender)
                                    })
                                    .await
                                    .unwrap_or_else(|e| {
                                        alert(format!("{:?}", e)).present(Some(&window));
                                        Ok(false)
                                    })
                                    .unwrap_or_else(|e| {
                                        alert(e).present(Some(&window));
                                        false
                                    });
                                    debug!("uuuuuuuuuuuuu DELETED {:?}", deleted);
                                }
                            });
                        }
                    });
                    let group = PreferencesGroup::builder()
                        .title(&remote.name)
                        .header_suffix(&del_button)
                        .build();
                    let row = EntryRow::builder()
                        .title("Name")
                        .text(&remote.name)
                        .show_apply_button(true)
                        .build();
                    group.add(&row);
                    let row = EntryRow::builder()
                        .title("Url")
                        .text(&remote.url)
                        .show_apply_button(true)
                        .build();
                    group.add(&row);
                    for refspec in &remote.refspecs {
                        let row = EntryRow::builder()
                            .title("Refspec")
                            .text(refspec)
                            .show_apply_button(true)
                            .build();
                        group.add(&row);
                    }
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
                    page.add(&group);
                }
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
                let adding_refspec = EntryRow::builder()
                    .title("Refspec (optional)")
                    .show_apply_button(false)
                    .build();
                adding.add(&adding_refspec);
                add_button.connect_clicked({
                    let path = path.clone();
                    let sender = sender.clone();
                    let window = window.clone();
                    move |_| {
                        let name = adding_name.text();
                        let url = adding_url.text();
                        debug!(
                            "add clicked! {:?} {:?} {:?}",
                            adding_name.text(),
                            adding_url.text(),
                            adding_refspec.text()
                        );
                        if name.len() > 0 && url.len() > 0 {
                            glib::spawn_future_local({
                                let path = path.clone();
                                let sender = sender.clone();
                                let window = window.clone();
                                async move {
                                    let added = gio::spawn_blocking(move || {
                                        remote::add(path, name.to_string(), url.to_string(), sender)
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
                                    debug!("eeeeeeeeeeeeeeeeeeeeeeeeeee {:?}", added);
                                }
                            });
                        }
                    }
                });
                page.add(&adding);
                dialog.add(&page);
                dialog.present(Some(&window));
            }
        });
    }
}
