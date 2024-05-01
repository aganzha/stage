use libadwaita::prelude::*;
use libadwaita::{
    ButtonContent, HeaderBar, MessageDialog, ResponseAppearance, SplitButton,
    Window,
};
use std::ffi::OsString;
// use glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;

use gtk4::{
    gio, AlertDialog, Align, Button, FileDialog, Label, PopoverMenu, Widget,
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

pub fn make_header_bar(
    sender: Sender<crate::Event>,
    settings: gio::Settings,
) -> (HeaderBar, impl Fn(OsString)) {
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

    let path_updater = {
        let repo_opener = repo_opener.clone();
        move |path: OsString| {
            let repo_opener_label = repo_opener.last_child().unwrap();
            let repo_opener_label =
                repo_opener_label.downcast_ref::<Label>().unwrap();
            let clean_path = path.into_string().unwrap().replace(".git/", "");
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
    };

    let repo_selector = SplitButton::new();
    repo_selector.set_child(Some(&repo_opener));
    repo_selector.set_popover(Some(&repo_popover));

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
    hb.pack_end(&branches_btn);
    hb.pack_end(&push_btn);
    hb.pack_end(&pull_btn);
    hb.pack_end(&log_btn);
    hb.pack_end(&reset_btn);
    (hb, path_updater)
}
