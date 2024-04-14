use async_channel::Sender;
use glib::{clone, Object};
use gtk4::builders::ButtonBuilder;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, Box, Button, EventControllerKey, Label, ListBox,
    Orientation, ScrolledWindow, SelectionMode, Window as Gtk4Window,
};
use std::ffi::OsString;
use std::rc::Rc;

use crate::{
    apply_stash as git_apply_stash, drop_stash, display_error, make_confirm_dialog,
    stash_changes, Event, StashData, Status,
};
use libadwaita::prelude::*;
use libadwaita::{
    ActionRow, EntryRow, HeaderBar, PreferencesRow, SwitchRow, ToolbarStyle,
    ToolbarView,
};

use log::{debug, trace};

glib::wrapper! {
    pub struct OidRow(ObjectSubclass<oid_row::OidRow>)
        @extends ActionRow, PreferencesRow, gtk4::ListBoxRow, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Actionable, gtk4::Buildable, gtk4::ConstraintTarget;
}

mod oid_row {
    use crate::StashData;
    use git2::Oid;
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;
    use libadwaita::ActionRow;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::OidRow)]
    pub struct OidRow {
        pub stash: RefCell<StashData>,

        #[property(get, set)]
        pub oid: RefCell<String>,
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

    pub fn from_stash(stash: &StashData) -> Self {
        let row = Self::new();
        row.set_property("title", &stash.title);
        row.set_oid(stash.oid.to_string());
        //row.add_suffix(&Label::builder().label("suffix").build());
        //row.add_prefix(&Label::builder().label("prefix").build());
        row.set_subtitle(&format!("stash@{}", &stash.num));
        // row.set_property("activatable", true);
        // row.set_property("selectable", true);
        row.set_can_focus(true);
        row.set_css_classes(&[&String::from("nocorners")]);
        row.imp().stash.replace(stash.clone());
        row
    }

    pub fn kill(
        &self,
        path: OsString,
        kill: impl FnOnce(OidRow) + 'static,
        window: &impl IsA<Gtk4Window>,
        sender: Sender<Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as row,
            @strong window as window => async move {
                let stash = row.imp().stash.borrow();
                let lbl = Label::new(Some(&format!("Drop stash {}", stash.title)));
                let dialog = make_confirm_dialog(
                    &window,
                    Some(&lbl),
                    "Drop",
                    "Drop"
                );
                let result = dialog.choose_future().await;
                if result == "confirm" {
                    let result = gio::spawn_blocking({
                        let stash = stash.clone();
                        let sender = sender.clone();
                        move || {
                            drop_stash(path.clone(), stash, sender.clone());
                        }
                    }).await;
                    if let Ok(_) = result {
                        kill(row.clone());
                    }
                }
            })
        });
    }
    
    pub fn apply_stash(
        &self,
        path: OsString,
        window: &impl IsA<Gtk4Window>,
        sender: Sender<Event>,
    ) {
        debug!("...........apply stash {:?}", self.imp().stash);
        glib::spawn_future_local({
            clone!(@weak self as row,
            @strong window as window => async move {
                let stash = row.imp().stash.borrow();
                let lbl = Label::new(Some(&format!("Apply stash {}", stash.title)));
                let dialog = make_confirm_dialog(
                    &window,
                    Some(&lbl),
                    "Apply",
                    "Apply"
                );
                let result = dialog.choose_future().await;
                if result == "confirm" {
                    gio::spawn_blocking({
                        let stash = stash.clone();
                        let sender = sender.clone();
                        move || {
                            git_apply_stash(path.clone(), stash, sender.clone());
                        }
                    });
                    sender
                        .send_blocking(crate::Event::StashesPanel)
                        .expect("cant send through channel");
                }
            })
        });
    }
}

impl Default for OidRow {
    fn default() -> Self {
        Self::new()
    }
}

pub fn add_stash(
    path: OsString,
    window: &impl IsA<Gtk4Window>,
    prepend: impl FnOnce(StashData) + 'static,
    sender: Sender<Event>,
) {
    glib::spawn_future_local({
        clone!(@strong window as window,
        @strong sender as sender => async move {
            let lb = ListBox::builder()
                .selection_mode(SelectionMode::None)
                .css_classes(vec![
                    String::from("boxed-list"),
                ])
                .build();
            let input = EntryRow::builder()
                .title("Stash message:")
                .css_classes(vec!["input_field"])
                .build();
            let staged = SwitchRow::builder()
             .title("Include staged changes")
             .css_classes(vec!["input_field"])
             .active(true)
                .build();

            lb.append(&input);
            lb.append(&staged);

            let dialog = make_confirm_dialog(
                &window,
                Some(&lb),
                "Stash changes",
                "Stash changes"
            );
            input.connect_apply(clone!(@strong dialog as dialog => move |entry| {
                // someone pressed enter
                dialog.response("confirm");
                dialog.close();
            }));
            input.connect_entry_activated(clone!(@strong dialog as dialog => move |entry| {
                // someone pressed enter
                dialog.response("confirm");
                dialog.close();
            }));
            if "confirm" != dialog.choose_future().await {
                return;
            }
            let stash_message = format!("{}", input.text());
            let stash_staged = staged.is_active();
            debug!("+++++++++++++++++++++++++++ {:?} {:?}", stash_message, stash_staged);
            let result = gio::spawn_blocking(move || {
                stash_changes(path, stash_message, stash_staged, sender)
            }).await;
            if let Ok(stash_data) = result {
                prepend(stash_data);
            } else {
                display_error(&window, "cant create stash");
            }
        })
    });
}

pub fn factory(
    window: &impl IsA<Gtk4Window>,
    status: &Status,
) -> (ToolbarView, impl FnOnce()) {
    let scroll = ScrolledWindow::new();
    scroll.set_css_classes(&[&String::from("nocorners")]);
    let lb = ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .css_classes(vec![
            String::from("boxed-list"),
            String::from("nocorners"),
        ])
        .build();
    if let Some(data) = &status.stashes {
        for stash in &data.stashes {
            let row = OidRow::from_stash(&stash);
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
        .icon_name("edit-redo-symbolic")
        .build();
    let kill = Button::builder()
        .tooltip_text("Kill stash (K)")
        .icon_name("edit-delete-symbolic")
        .build();
    
    add.connect_clicked({
        let sender = status.sender.clone();
        let window = window.clone();
        let path = status.path.clone().expect("no path");
        let lb = lb.clone();
        move |_| {
            add_stash(
                path.clone(),
                &window,
                {
                    let lb = lb.clone();
                    move |stash_data| {
                        let row = OidRow::from_stash(&stash_data);
                        lb.prepend(&row);
                    }
                },
                sender.clone(),
            );
        }
    });
    apply.connect_clicked({
        let window = window.clone();
        let path = status.path.clone().expect("no path");
        let sender = status.sender.clone();
        let lb = lb.clone();
        move |_| {
            if let Some(row) = lb.selected_row() {
                let oid_row =
                    row.downcast_ref::<OidRow>().expect("cant get oid row");
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
                let oid_row =
                    row.downcast_ref::<OidRow>().expect("cant get oid row");
                oid_row.kill(
                    path.clone(),
                    {
                        let lb = lb.clone();
                        move |row| {
                            lb.remove(&row);
                        }
                    },
                    &window,
                    sender.clone()
                );          
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
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::Escape, _) => {
                    sender
                        .send_blocking(crate::Event::StashesPanel)
                        .expect("cant send through channel");
                }
                (gdk::Key::a|gdk::Key::Return, _) => {
                    if let Some(row) = lb.selected_row() {
                        let oid_row = row
                            .downcast_ref::<OidRow>()
                            .expect("cant get oid row");
                        oid_row.apply_stash(
                            path.clone(),
                            &window,
                            sender.clone(),
                        );
                    }
                }
                (gdk::Key::k|gdk::Key::d, _) => {
                    if let Some(row) = lb.selected_row() {
                        let oid_row = row
                            .downcast_ref::<OidRow>()
                            .expect("cant get oid row");
                        oid_row.kill(
                            path.clone(),
                            {
                                let lb = lb.clone();
                                move |row| {
                                    lb.remove(&row);
                                }
                            },
                            &window,
                            sender.clone(),
                        );
                    }
                }
                (gdk::Key::z|gdk::Key::c|gdk::Key::n, _) => {
                    add_stash(
                        path.clone(),
                        &window,
                        {
                            let lb = lb.clone();
                            move |stash_data| {
                                let row = OidRow::from_stash(&stash_data);
                                lb.prepend(&row);
                            }
                        },
                        sender.clone(),
                    );
                }
                (key, modifier) => {
                    debug!(
                        "key press in stashes view{:?} {:?}",
                        key.name(),
                        modifier
                    );
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
