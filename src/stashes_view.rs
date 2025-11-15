// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use async_channel::Sender;
use git2::Oid;
use glib::Object;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ListBox, ScrolledWindow, SelectionMode,
};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::dialogs::{alert, confirm_dialog_factory, CANCEL, PROCEED};
use crate::git::stash;
use crate::{Event, Selected, Status};
use libadwaita::prelude::*;
use libadwaita::{
    ActionRow, AlertDialog, ApplicationWindow, EntryRow, HeaderBar, PreferencesRow,
    ResponseAppearance, SwitchRow, ToolbarStyle, ToolbarView,
};
use log::{debug, trace};
use std::cell::RefCell;
use std::rc::Rc;

glib::wrapper! {
    pub struct OidRow(ObjectSubclass<oid_row::OidRow>)
        @extends ActionRow, PreferencesRow, gtk4::ListBoxRow, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

mod oid_row {
    use crate::git::stash::StashData;

    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;
    use libadwaita::ActionRow;
    use std::cell::{Cell, RefCell};

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::OidRow)]
    pub struct OidRow {
        pub stash: RefCell<StashData>,

        #[property(get, set)]
        pub oid: RefCell<String>,

        #[property(get, set)]
        pub num: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OidRow {
        const NAME: &'static str = "StageOidRow";
        type Type = super::OidRow;
        type ParentType = ActionRow;
    }

    #[glib::derived_properties]
    impl ObjectImpl for OidRow {}
    impl WidgetImpl for OidRow {}
    impl ActionRowImpl for OidRow {}
    impl PreferencesRowImpl for OidRow {}
    impl ListBoxRowImpl for OidRow {}
}

impl OidRow {
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn from_stash(stash: &stash::StashData, sender: Sender<Event>) -> Self {
        let row = Self::new();
        row.set_property("title", &stash.title);
        row.set_oid(stash.oid.to_string());
        row.set_num(stash.num.as_i32());

        let commit_button = Button::builder()
            .label("View stash")
            .tooltip_text("View stash")
            .has_frame(false)
            .icon_name("emblem-documents-symbolic")
            .build();
        commit_button.connect_clicked({
            let oid = stash.oid;
            let num = stash.num;
            move |_| {
                sender
                    .send_blocking(Event::ShowOid(oid, Some(num), None))
                    .expect("cant send through channel");
            }
        });
        row.add_suffix(&commit_button);
        row.set_subtitle(&format!("stash@{}", &stash.num.as_usize()));
        row.bind_property("num", &row, "subtitle")
            .transform_to(|_, num: i32| Some(format!("stash@{}", &num)))
            .build();
        row.set_can_focus(true);
        row.set_css_classes(&[&String::from("nocorners")]);
        row.imp().stash.replace(stash.clone());
        row
    }

    pub fn kill(&self, path: PathBuf, window: &ApplicationWindow, sender: Sender<Event>) {
        glib::spawn_future_local({
            let window = window.clone();
            let row = self.clone();
            async move {
                let lbl = {
                    let stash = row.imp().stash.borrow();
                    Label::new(Some(&format!("Drop stash {}", stash.title)))
                };
                let dialog = confirm_dialog_factory(Some(&lbl), "Drop", "Drop");
                let result = dialog.choose_future(&window).await;
                if result == PROCEED {
                    let result = gio::spawn_blocking({
                        let stash = row.imp().stash.borrow().clone();
                        let sender = sender.clone();
                        move || stash::drop(path.clone(), stash, sender.clone())
                    })
                    .await;
                    if let Ok(stashes) = result {
                        let pa = row.parent().unwrap();
                        let lb = pa.downcast_ref::<ListBox>().unwrap();
                        let mut ind = row.num() - 1;
                        if ind < 0 {
                            ind = 0;
                        }
                        lb.remove(&row);
                        adopt_stashes(lb, stashes, sender, Some(ind));
                    }
                }
            }
        });
    }

    pub fn apply_stash(&self, path: PathBuf, window: &ApplicationWindow, sender: Sender<Event>) {
        trace!("...........apply stash {:?}", self.imp().stash);
        glib::spawn_future_local({
            let window = window.clone();
            let row = self.clone();

            async move {
                let lbl = {
                    let stash = row.imp().stash.borrow();
                    Label::new(Some(&format!("Apply stash {}", stash.title)))
                };
                let dialog = confirm_dialog_factory(Some(&lbl), "Apply", "Apply");
                let result = dialog.choose_future(&window).await;
                if result == PROCEED {
                    gio::spawn_blocking({
                        let stash = row.imp().stash.borrow().clone();
                        let sender = sender.clone();
                        move || stash::apply(path, stash.num, None, sender)
                    })
                    .await
                    .unwrap()
                    .unwrap_or_else(|e| {
                        alert(e).present(Some(&window));
                    });
                    sender
                        .send_blocking(Event::StashesPanel)
                        .expect("cant send through channel");
                }
            }
        });
    }
}

impl Default for OidRow {
    fn default() -> Self {
        Self::new()
    }
}

pub fn add_stash(
    path: PathBuf,
    window: &ApplicationWindow,
    stashes_box: &ListBox,
    selected: Selected,
    sender: Sender<Event>,
) {
    glib::spawn_future_local({
        let window = window.clone();
        let sender = sender.clone();
        let stashes_box = stashes_box.clone();
        async move {
            let lb = ListBox::builder()
                .selection_mode(SelectionMode::None)
                .css_classes(vec![String::from("boxed-list")])
                .build();
            let input = EntryRow::builder()
                .title("Stash message:")
                .css_classes(vec!["input_field"])
                .show_apply_button(false)
                .build();
            lb.append(&input);
            let staged = SwitchRow::builder()
                .title("Include staged changes")
                .css_classes(vec!["input_field"])
                .active(true)
                .build();

            lb.append(&staged);

            let title = "Stash changes";
            let dialog = AlertDialog::builder()
                .heading(title)
                .close_response(CANCEL)
                .default_response(PROCEED)
                .width_request(720)
                .height_request(120)
                .focus_widget(&input)
                .build();

            let file_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
            if let Some(selected) = &selected {
                if let Some(path) = &selected.1 {
                    let str_path = path.to_string_lossy().to_string();
                    let file_chooser = SwitchRow::builder()
                        .title(format!("Only changes in file {:}", &str_path))
                        .subtitle("will reset everything to head and save only choosen path")
                        .css_classes(vec!["input_field"])
                        .active(false)
                        .build();
                    file_chooser.connect_active_notify({
                        let file_path = file_path.clone();
                        let path = path.clone();
                        let dialog = dialog.clone();
                        let input = input.clone();
                        move |row| {
                            if row.is_active() {
                                input.set_visible(false);
                                dialog.set_response_enabled(PROCEED, true);
                                file_path.borrow_mut().replace(path.clone());
                            } else {
                                input.set_visible(false);
                                file_path.borrow_mut().take();
                            }
                        }
                    });
                    lb.append(&file_chooser);
                }
            }

            dialog.set_extra_child(Some(&lb));
            dialog.add_responses(&[(CANCEL, "Cancel"), (PROCEED, title)]);

            dialog.set_response_appearance(PROCEED, ResponseAppearance::Suggested);
            dialog.set_response_enabled(PROCEED, false);
            input.connect_changed({
                let dialog = dialog.clone();
                move |row| {
                    dialog.set_response_enabled(PROCEED, !row.text().is_empty());
                }
            });

            input.connect_entry_activated({
                let dialog = dialog.clone();
                move |row| {
                    if !row.text().is_empty() {
                        dialog.emit_by_name::<()>("response", &[&PROCEED.to_string()]);
                    }
                    dialog.close();
                }
            });

            let response = dialog.choose_future(&window).await;
            if response != PROCEED {
                return;
            }
            let stash_message = format!("{}", input.text());
            let stash_staged = staged.is_active();
            let result = gio::spawn_blocking({
                let sender = sender.clone();
                let file_path = file_path.borrow().clone();
                move || stash::stash(path, stash_message, stash_staged, file_path.clone(), sender)
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
            if let Some(stashes) = result {
                adopt_stashes(&stashes_box, stashes, sender, None);
            }
        }
    });
}

pub fn adopt_stashes(
    lb: &ListBox,
    stashes: stash::Stashes,
    sender: Sender<Event>,
    o_row_ind: Option<i32>,
) {
    let mut ind = 0;
    let mut map: HashMap<Oid, stash::StashData> = HashMap::new();
    stashes.stashes.iter().for_each(|stash| {
        map.insert(stash.oid, stash.clone());
    });
    while let Some(row) = lb.row_at_index(ind) {
        let oid_row = row.downcast_ref::<OidRow>().expect("cant get oid row");
        let oid = oid_row.imp().stash.borrow().oid;
        let new_stash = map.remove(&oid).unwrap();
        oid_row.set_num(new_stash.num.as_i32());
        oid_row.imp().stash.replace(new_stash);
        ind += 1;
    }
    if let Some(row_ind) = o_row_ind {
        // deleting row
        if let Some(row) = lb.row_at_index(row_ind) {
            lb.select_row(Some(&row));
            row.grab_focus();
        }
    }
    // adding new row
    for (_, stash_data) in map.iter_mut() {
        lb.prepend(&OidRow::from_stash(stash_data, sender.clone()))
    }
}

pub fn factory(window: &ApplicationWindow, status: &Status) -> (ToolbarView, impl FnOnce()) {
    let scroll = ScrolledWindow::new();
    scroll.set_css_classes(&[&String::from("nocorners")]);
    let lb = ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .css_classes(vec![String::from("boxed-list"), String::from("nocorners")])
        .build();
    if let Some(data) = &status.stashes {
        for stash in &data.stashes {
            let row = OidRow::from_stash(stash, status.sender.clone());
            lb.append(&row);
        }
    }
    scroll.set_child(Some(&lb));

    let hb = HeaderBar::builder().show_title(false).build();
    let tb = ToolbarView::builder()
        .top_bar_style(ToolbarStyle::Flat)
        .content(&scroll)
        .build();

    let add = Button::builder()
        .tooltip_text("Stash (Z)")
        .icon_name("list-add-symbolic")
        .build();
    let apply = Button::builder()
        .tooltip_text("Apply (A)")
        .icon_name("emblem-shared-symbolic")
        .build();
    let kill = Button::builder()
        .tooltip_text("Kill stash (K)")
        .icon_name("user-trash-symbolic") // process-stop-symbolic
        .build();

    add.connect_clicked({
        let sender = status.sender.clone();
        let window = window.clone();
        let path = status.path.clone().expect("no path");
        let lb = lb.clone();
        let selected = status.selected().clone();
        move |_| {
            add_stash(path.clone(), &window, &lb, selected.clone(), sender.clone());
        }
    });
    apply.connect_clicked({
        let window = window.clone();
        let path = status.path.clone().expect("no path");
        let sender = status.sender.clone();
        let lb = lb.clone();
        move |_| {
            if let Some(row) = lb.selected_row() {
                let oid_row = row.downcast_ref::<OidRow>().expect("cant get oid row");
                oid_row.apply_stash(path.clone(), &window, sender.clone());
            }
        }
    });
    kill.connect_clicked({
        let window = window.clone();
        let path = status.path.clone().expect("no path");
        let sender = status.sender.clone();
        let lb = lb.clone();
        move |_| {
            if let Some(row) = lb.selected_row() {
                let oid_row = row.downcast_ref::<OidRow>().expect("cant get oid row");
                oid_row.kill(path.clone(), &window, sender.clone());
            }
        }
    });

    hb.pack_end(&add);
    hb.pack_end(&apply);
    hb.pack_end(&kill);

    tb.add_top_bar(&hb);

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let sender = status.sender.clone();
        let lb = lb.clone();
        let window = window.clone();
        let path = status.path.clone().expect("no path");
        let selected = status.selected().clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::Escape, _) => {
                    sender
                        .send_blocking(crate::Event::StashesPanel)
                        .expect("cant send through channel");
                }
                (gdk::Key::a, _) => {
                    if let Some(row) = lb.selected_row() {
                        let oid_row = row.downcast_ref::<OidRow>().expect("cant get oid row");
                        oid_row.apply_stash(path.clone(), &window, sender.clone());
                    }
                }
                (gdk::Key::k | gdk::Key::d, _) => {
                    if let Some(row) = lb.selected_row() {
                        let oid_row = row.downcast_ref::<OidRow>().expect("cant get oid row");
                        oid_row.kill(path.clone(), &window, sender.clone());
                    }
                }
                (gdk::Key::z | gdk::Key::c | gdk::Key::n, _) => {
                    add_stash(
                        path.clone(),
                        &window,
                        &lb.clone(),
                        selected.clone(),
                        sender.clone(),
                    );
                }
                (gdk::Key::v | gdk::Key::Return, _) => {
                    if let Some(row) = lb.selected_row() {
                        let oid_row = row.downcast_ref::<OidRow>().expect("cant get oid row");
                        let oid = oid_row.imp().stash.borrow().oid;
                        let num = oid_row.imp().stash.borrow().num;
                        sender
                            .send_blocking(Event::ShowOid(oid, Some(num), None))
                            .expect("cant send through channel");
                    }
                }
                (key, modifier) => {
                    debug!("key press in stashes view{:?} {:?}", key.name(), modifier);
                }
            }
            glib::Propagation::Proceed
        }
    });
    tb.add_controller(event_controller);

    let focus = move || {
        lb.select_row(lb.row_at_index(0).as_ref());
        if let Some(first_row) = lb.row_at_index(0) {
            first_row.grab_focus();
        }
    };
    (tb, focus)
}
