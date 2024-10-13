// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use libadwaita::prelude::*;
use libadwaita::{ButtonContent, ColorScheme, HeaderBar, SplitButton, StyleManager, Window};
// use glib::Sender;
// use std::sync::mpsc::Sender;

use async_channel::Sender;
use gtk4::{
    gio, Align, Box, Button, FileDialog, Label, MenuButton, Orientation, PopoverMenu, ToggleButton,
};
use std::path::PathBuf;

pub enum HbUpdateData {
    Path(PathBuf),
    Staged(bool),
    Unsynced(bool),
    RepoOpen,
    RepoPopup,
}

#[derive(Eq, Hash, PartialEq, Debug)]
pub struct Scheme(String);

pub const DARK: &str = "dark";
pub const LIGHT: &str = "light";
pub const DEFAULT: &str = "default";

impl Scheme {
    pub fn new(s: String) -> Self {
        Self(s)
    }
    pub fn from_str(s: &str) -> Self {
        Self(s.to_string())
    }

    pub fn scheme_name(&self) -> ColorScheme {
        match &self.0[..] {
            DARK => ColorScheme::ForceDark,
            LIGHT => ColorScheme::ForceLight,
            _ => ColorScheme::Default,
        }
    }
    pub fn str(&self) -> &str {
        &self.0
    }
    fn setting_key(&self) -> String {
        SCHEME_TOKEN.to_string()
    }
}

pub struct MenuItem(String);
pub const CUSTOM_ATTR: &str = "custom";
pub const SCHEME_TOKEN: &str = "scheme";
pub const ZOOM_TOKEN: &str = "zoom";

pub fn scheme_selector(stored_scheme: Scheme, sender: Sender<crate::Event>) -> Box {
    let scheme_selector = Box::builder()
        .orientation(Orientation::Horizontal)
        .css_name("scheme_selector")
        .build();

    let mut first_toggle: Option<ToggleButton> = None;
    for scheme in [
        Scheme::from_str(DEFAULT),
        Scheme::from_str(LIGHT),
        Scheme::from_str(DARK),
    ] {
        let toggle = ToggleButton::builder()
            .active(false)
            .icon_name("")
            .name(scheme.str())
            .css_classes(vec![scheme.str()])
            .margin_end(10)
            .build();
        if stored_scheme == scheme {
            toggle.set_icon_name("object-select-symbolic");
            toggle.set_active(true);
        }
        toggle.last_child().unwrap().set_halign(Align::Center);
        toggle
            .bind_property("active", &toggle, "icon_name")
            .transform_to({
                let sender = sender.clone();
                move |_, is_active: bool| {
                    if is_active {
                        let manager = StyleManager::default();
                        manager.set_color_scheme(scheme.scheme_name());

                        sender
                            .send_blocking(crate::Event::StoreSettings(
                                scheme.setting_key(),
                                scheme.0.to_string(),
                            ))
                            .expect("cant send through sender");
                        Some("object-select-symbolic")
                    } else {
                        Some("")
                    }
                }
            })
            .build();
        scheme_selector.append(&toggle);
        if let Some(ref ft) = first_toggle {
            toggle.set_group(Some(ft));
        } else {
            first_toggle.replace(toggle);
        }
    }

    let bx = Box::builder()
        .orientation(Orientation::Vertical)
        .margin_top(2)
        .margin_bottom(2)
        .margin_start(2)
        .margin_end(2)
        .spacing(12)
        .build();
    bx.append(&scheme_selector);
    bx
}

pub fn zoom(
    // stored_size: Scheme,
    sender: Sender<crate::Event>,
) -> Box {
    let bx = Box::builder()
        .orientation(Orientation::Horizontal)
        .halign(Align::Center)
        .valign(Align::Center)
        .margin_top(12)
        .margin_bottom(4)
        .margin_start(2)
        .margin_end(2)
        .spacing(12)
        .build();
    let zoom_out_btn = Button::builder()
        .label("Zoom out")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Zoom out")
        .icon_name("zoom-out-symbolic")
        .can_shrink(true)
        .margin_start(2)
        .margin_end(2)
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
        .margin_start(2)
        .margin_end(2)
        .build();
    zoom_in_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Zoom(true))
                .expect("cant send through channel");
        }
    });
    bx.append(&zoom_out_btn);
    bx.append(&Label::new(Some("zoom")));
    bx.append(&zoom_in_btn);
    bx
}

pub fn burger_menu(stored_scheme: Scheme, sender: Sender<crate::Event>) -> MenuButton {
    let menu_model = gio::Menu::new();

    let scheme_model = gio::Menu::new();

    let menu_item = gio::MenuItem::new(Some(SCHEME_TOKEN), Some("win.menu::1"));

    let scheme_id = SCHEME_TOKEN.to_variant();
    menu_item.set_attribute_value(CUSTOM_ATTR, Some(&scheme_id));
    scheme_model.insert_item(0, &menu_item);

    let zoom_model = gio::Menu::new();
    let menu_item = gio::MenuItem::new(Some(ZOOM_TOKEN), Some("win.menu::2"));

    let zoom_id = ZOOM_TOKEN.to_variant();
    menu_item.set_attribute_value(CUSTOM_ATTR, Some(&zoom_id));
    zoom_model.insert_item(1, &menu_item);

    menu_model.append_section(None, &scheme_model);
    menu_model.append_section(None, &zoom_model);

    let popover_menu = PopoverMenu::from_model(Some(&menu_model)); // menu_model

    popover_menu.add_child(
        &scheme_selector(stored_scheme, sender.clone()),
        SCHEME_TOKEN,
    );
    popover_menu.add_child(&zoom(sender.clone()), ZOOM_TOKEN);
    MenuButton::builder()
        .popover(&popover_menu)
        .icon_name("open-menu-symbolic")
        .build()
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
                .send_blocking(crate::Event::ShowBranches)
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
                .send_blocking(crate::Event::ResetHard(None))
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
                .send_blocking(crate::Event::Log(None, None))
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
                let some_box = repo_opener.last_child().unwrap();
                let repo_opener_label = some_box.last_child().unwrap();
                let repo_opener_label = repo_opener_label.downcast_ref::<Label>().unwrap();
                let clean_path = path
                    .into_os_string()
                    .into_string()
                    .expect("wrog path")
                    .replace(".git/", "");
                repo_opener_label
                    .set_markup(&format!("<span weight=\"normal\">{}</span>", clean_path));
                repo_opener_label.set_visible(true);
                let mut path_exists = false;
                for i in 0..repo_menu.n_items() {
                    let iter = repo_menu.iterate_item_attributes(i);
                    while let Some(attr) = iter.next() {
                        if attr.0 == "target"
                            && clean_path
                                == attr.1.get::<String>().expect("cant get path from gvariant")
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
            dialog.select_folder(None::<&Window>, None::<&gio::Cancellable>, {
                let sender = sender.clone();
                move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            sender
                                .send_blocking(crate::Event::OpenRepo(path))
                                .expect("Could not send through channel");
                        }
                    }
                }
            });
        }
    });
    let hb = HeaderBar::new();
    hb.pack_start(&stashes_btn);
    // hb.pack_start(&refresh_btn);

    hb.set_title_widget(Some(&repo_selector));

    hb.pack_end(&burger_menu(
        Scheme::new(settings.get::<String>(SCHEME_TOKEN)),
        sender,
    ));
    hb.pack_end(&commit_btn);
    hb.pack_end(&branches_btn);
    hb.pack_end(&push_btn);
    hb.pack_end(&pull_btn);
    hb.pack_end(&log_btn);
    hb.pack_end(&reset_btn);
    hb.pack_end(&refresh_btn);
    (hb, updater)
}
