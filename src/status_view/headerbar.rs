use libadwaita::prelude::*;
use libadwaita::{
    ButtonContent, HeaderBar, SplitButton,
    Window,
};
use std::ffi::OsString;
// use glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;

use gtk4::{
    gio, Align, Button, FileDialog, Label, PopoverMenu,
};

pub enum HbUpdateData {
    Path(OsString),
    Staged(bool),
    Unsynced(bool),
    RepoOpen,
    RepoPopup,
}

pub fn factory(
    sender: Sender<crate::Event>,
    settings: gio::Settings,
) -> (HeaderBar, impl Fn(HbUpdateData)) {
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
    let zoom_out_btn = Button::builder()
        .label("Zoom out")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Zoom out")
        .icon_name("zoom-out-symbolic")
        .can_shrink(true)
        .margin_start(24)
        .margin_end(0)
        .build();
    zoom_out_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Zoom(false))
                .expect("cant send through channel");
        }
    });

    let zoom_in_btn = Button::builder()
        .label("Zoom in")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Zoom in")
        .icon_name("zoom-in-symbolic")
        .can_shrink(true)
        .margin_start(0)
        .build();
    zoom_in_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Zoom(true))
                .expect("cant send through channel");
        }
    });

    let branches_btn = Button::builder()
        .label("Branches")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Branches")
        .icon_name("org.gtk.gtk4.NodeEditor-symbolic")
        .can_shrink(true)
        .build();
    branches_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Branches)
                .expect("cant send through channel");
        }
    });

    let push_btn = Button::builder()
        .label("Push")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Push")
        .icon_name("send-to-symbolic")
        .can_shrink(true)
        .sensitive(false)
        .build();
    push_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Push)
                .expect("cant send through channel");
        }
    });
    let reset_btn = Button::builder()
        .label("Reset hard")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Reset hard")
        .icon_name("software-update-urgent-symbolic")
        .can_shrink(true)
        .build();
    reset_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::ResetHard)
                .expect("cant send through channel");
        }
    });
    let log_btn = Button::builder()
        .label("Log")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Log")
        .icon_name("org.gnome.Logs-symbolic")
        .can_shrink(true)
        .build();
    log_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Log)
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
    let commit_btn = Button::builder()
        .label("Commit")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Commit")
        .icon_name("object-select-symbolic")
        .can_shrink(true)
        .sensitive(false)
        .build();
    commit_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Commit)
                .expect("cant send through channel");
        }
    });

    let repo_menu = gio::Menu::new();
    for path in settings.get::<Vec<String>>("paths").iter() {
        repo_menu.append(Some(path), Some(&format!("win.open::{}", path)));
    }
    let repo_popover = PopoverMenu::from_model(Some(&repo_menu));

    let repo_opener = ButtonContent::builder()
        .icon_name("document-open-symbolic")
        .use_underline(true)
        .valign(Align::Baseline)
        .build();

    let repo_selector = SplitButton::new();
    repo_selector.set_child(Some(&repo_opener));
    repo_selector.set_popover(Some(&repo_popover));

    let updater = {
        let repo_opener = repo_opener.clone();
        let commit_btn = commit_btn.clone();
        let push_btn = push_btn.clone();
        let repo_selector = repo_selector.clone();
        move |data: HbUpdateData| match data {
            HbUpdateData::Path(path) => {
                let repo_opener_label = repo_opener.last_child().unwrap();
                let repo_opener_label =
                    repo_opener_label.downcast_ref::<Label>().unwrap();
                let clean_path =
                    path.into_string().unwrap().replace(".git/", "");
                repo_opener_label.set_markup(&format!(
                    "<span weight=\"normal\">{}</span>",
                    clean_path
                ));
                repo_opener_label.set_visible(true);
                let mut path_exists = false;
                for i in 0..repo_menu.n_items() {
                    let iter = repo_menu.iterate_item_attributes(i);
                    while let Some(attr) = iter.next() {
                        if attr.0 == "target"
                            && clean_path
                                == attr
                                    .1
                                    .get::<String>()
                                    .expect("cant get path from gvariant")
                        {
                            path_exists = true;
                            break;
                        }
                    }
                }
                if !path_exists {
                    repo_menu.append(
                        Some(&clean_path),
                        Some(&format!("win.open::{}", clean_path)),
                    );
                }
            }
            HbUpdateData::Staged(is_staged) => {
                commit_btn.set_sensitive(is_staged);
            }
            HbUpdateData::Unsynced(has_unsynced) => {
                push_btn.set_sensitive(has_unsynced);
            }
            HbUpdateData::RepoOpen => {
                repo_selector.emit_activate();
            }
            HbUpdateData::RepoPopup => {
                repo_selector.popover().expect("no popover").popup();
            }
        }
    };

    repo_selector.connect_clicked({
        let sender = sender.clone();
        move |_| {
            let dialog = FileDialog::new();
            dialog.select_folder(
                None::<&Window>,
                None::<&gio::Cancellable>,
                {
                    let sender = sender.clone();
                    move |result| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                sender
                                    .send_blocking(crate::Event::OpenRepo(
                                        path.into(),
                                    ))
                                    .expect("Could not send through channel");
                            }
                        }
                    }
                },
            );
        }
    });
    let hb = HeaderBar::new();
    hb.pack_start(&stashes_btn);
    hb.pack_start(&refresh_btn);
    hb.pack_start(&zoom_out_btn);
    hb.pack_start(&zoom_in_btn);
    hb.set_title_widget(Some(&repo_selector));
    hb.pack_end(&commit_btn);
    hb.pack_end(&branches_btn);
    hb.pack_end(&push_btn);
    hb.pack_end(&pull_btn);
    hb.pack_end(&log_btn);
    hb.pack_end(&reset_btn);
    (hb, updater)
}
