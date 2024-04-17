use async_channel::Sender;
use std::collections::HashMap;
use git2::Oid;
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
    stash_changes, Event, StashData, Status, Stashes
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
        
        #[property(get, set)]
        pub num: RefCell<i32>,
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

    pub fn from_stash(stash: &StashData, sender: Sender<Event>) -> Self {
        let row = Self::new();
        row.set_property("title", &stash.title);
        row.set_oid(stash.oid.to_string());
        row.set_num(stash.num as i32);
        
        let commit_button = Button::builder()
            .label("View stash")
            .tooltip_text("View stash")
            .has_frame(false)
            .icon_name("emblem-documents-symbolic")
            .build();
        commit_button.connect_clicked({
            let oid = stash.oid.clone();
            move |_| {
                sender.send_blocking(Event::ShowOid(oid))
                    .expect("cant send through channel");
            }
        });
        row.add_suffix(&commit_button);
        row.set_subtitle(&format!("stash@{}", &stash.num));
        row.bind_property("num", &row, "subtitle")
            .transform_to( |_, num: i32| {
                Some(format!("stash@{}", &num))
            }).build();
        row.set_can_focus(true);
        row.set_css_classes(&[&String::from("nocorners")]);
        row.imp().stash.replace(stash.clone());
        row
    }

    pub fn kill(
        &self,
        path: OsString,
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
                            drop_stash(path.clone(), stash, sender.clone())
                        }
                    }).await;
                    if let Ok(stashes) = result {
                        let pa = row.parent().unwrap();
                        let lb = pa.downcast_ref::<ListBox>().unwrap();
                        let mut ind = row.num() - 1;
                        if ind < 0 {
                            ind = 0;
                        }
                        lb.remove(&row);
                        adopt_stashes(&lb, stashes, sender, Some(ind));
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
        trace!("...........apply stash {:?}", self.imp().stash);
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
                        .send_blocking(Event::StashesPanel)
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
    stashes_box: &ListBox,
    sender: Sender<Event>,
) {
    glib::spawn_future_local({
        clone!(@strong window as window,
               @strong stashes_box as stashes_box,
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
            input.connect_apply(clone!(@strong dialog as dialog => move |_| {
                // someone pressed enter
                dialog.response("confirm");
                dialog.close();
            }));
            input.connect_entry_activated(clone!(@strong dialog as dialog => move |_| {
                // someone pressed enter
                dialog.response("confirm");
                dialog.close();
            }));
            if "confirm" != dialog.choose_future().await {
                return;
            }
            let stash_message = format!("{}", input.text());
                   let stash_staged = staged.is_active();
                   let result = gio::spawn_blocking({
                       let sender = sender.clone();
                       move || {
                           stash_changes(path, stash_message, stash_staged, sender)
                       }
                   }).await;
                   if let Ok(stashes) = result {
                       adopt_stashes(&stashes_box, stashes, sender, None);
                   } else {
                       display_error(&window, "cant create stash");
                   }
        })
    });
}

pub fn adopt_stashes(lb: &ListBox, stashes: Stashes, sender: Sender<Event>, o_row_ind: Option<i32>) {
    let mut ind = 0;
    let mut map: HashMap<Oid, StashData> = HashMap::new();
    stashes.stashes.iter().for_each(|stash| {
        map.insert(stash.oid, stash.clone());
    });
    while let Some(row) = lb.row_at_index(ind) {
        let oid_row =
            row.downcast_ref::<OidRow>().expect("cant get oid row");
        let oid = oid_row.imp().stash.borrow().oid;
        let new_stash = map.remove(&oid).unwrap();
        oid_row.set_num(new_stash.num as i32);
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
    for (_, stash_data) in map.iter_mut(){
        lb.prepend(&OidRow::from_stash(&stash_data, sender.clone()))
    }
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
            let row = OidRow::from_stash(&stash, status.sender.clone());
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
        .icon_name("user-trash-symbolic")// process-stop-symbolic
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
                &lb,
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
                            &window,
                            sender.clone(),
                        );
                    }
                }
                (gdk::Key::z|gdk::Key::c|gdk::Key::n, _) => {
                    add_stash(
                        path.clone(),
                        &window,
                        &lb.clone(),
                        sender.clone(),
                    );
                }
                (gdk::Key::v, _) => {
                    if let Some(row) = lb.selected_row() {
                        let oid_row = row
                            .downcast_ref::<OidRow>()
                            .expect("cant get oid row");
                        let oid = oid_row.imp().stash.borrow().oid.clone();
                        sender.send_blocking(Event::ShowOid(oid))
                            .expect("cant send through channel");
                    }
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
