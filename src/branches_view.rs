use async_channel::Sender;
use core::time::Duration;
use git2::BranchType;
use glib::{clone, closure, ControlFlow, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, AlertDialog, Box, Button, CheckButton,
    EventControllerKey, FilterListModel, Image, Label, ListBox, ListHeader,
    ListItem, ListScrollFlags, ListView, Orientation, Revealer,
    ScrolledWindow, SearchBar, SearchEntry, SectionModel, SelectionMode,
    SignalListItemFactory, SingleSelection, Spinner, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{
    ActionRow, ApplicationWindow, EntryRow, HeaderBar, SwitchRow, ToolbarView,
    Window,
};

use log::{debug, info, trace};

glib::wrapper! {
    pub struct BranchItem(ObjectSubclass<branch_item::BranchItem>);
}

mod branch_item {
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BranchItem)]
    pub struct BranchItem {

        pub branch: RefCell<crate::BranchData>,

        #[property(get, set)]
        pub initial_focus: RefCell<bool>,

        #[property(get = Self::get_branch_is_head, set = Self::set_branch_is_head)]
        pub is_head: RefCell<bool>,

        #[property(get, set)]
        pub ref_kind: RefCell<String>,

        #[property(get, set)]
        pub title: RefCell<String>,

        #[property(get, set)]
        pub last_commit: RefCell<String>,

        #[property(get, set)]
        pub dt: RefCell<String>,

    }

    #[glib::object_subclass]
    impl ObjectSubclass for BranchItem {
        const NAME: &'static str = "StageBranchItem";
        type Type = super::BranchItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for BranchItem {
    }

    impl BranchItem {
        pub fn set_branch_is_head(&self, value: bool) -> bool {
            // fake property. it need to set it, to trigger
            // avatar icon render via binding
            value
        }

        pub fn get_branch_is_head(&self) -> bool {
            let branch = self.branch.borrow();
            branch.is_head
        }
    }

}

impl BranchItem {
    pub fn new(branch: crate::BranchData) -> Self {
        let ref_kind = {
            match branch.branch_type {
                BranchType::Local => String::from("Branches"),
                BranchType::Remote => String::from("Remote"),
            }
        };
        let ob = Object::builder::<BranchItem>()
            .property("is-head", branch.is_head)
            .property("ref-kind", ref_kind)
            .property(
                "title",
                format!("<span color=\"#4a708b\">{}</span>", &branch.name),
            )
            .property("last-commit", &branch.commit_string)
            .property("dt", branch.commit_dt.to_string())
            .property("initial-focus", false)
            .build();
        ob.imp().branch.replace(branch);
        ob
    }
}

glib::wrapper! {
    pub struct BranchList(ObjectSubclass<branch_list::BranchList>)
        @implements gio::ListModel, SectionModel; // , FilterListModel
}

mod branch_list {

    use glib::Properties;
    use gtk4::gio;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BranchList)]
    pub struct BranchList {
        pub original_list: RefCell<Vec<super::BranchItem>>,
        pub list: RefCell<Vec<super::BranchItem>>,
        pub remote_start_pos: RefCell<Option<u32>>,

        #[property(get, set)]
        pub selected_pos: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BranchList {
        const NAME: &'static str = "StageBranchList";
        type Type = super::BranchList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel, gtk4::SectionModel);
    }

    #[glib::derived_properties]
    impl ObjectImpl for BranchList {}

    impl ListModelImpl for BranchList {
        fn item_type(&self) -> glib::Type {
            super::BranchItem::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            let list = self.list.borrow();
            if list.is_empty() {
                return None;
            }
            if position as usize >= list.len() {
                return None;
            }
            // ??? clone ???
            Some(list[position as usize].clone().into())
        }
    }

    impl SectionModelImpl for BranchList {
        fn section(&self, position: u32) -> (u32, u32) {
            if let Some(pos) = *self.remote_start_pos.borrow() {
                if position < pos {
                    // IMPORTANT was <=
                    return (0, pos);
                } else {
                    return (pos, self.list.borrow().len() as u32);
                }
            }
            (0, self.list.borrow().len() as u32)
        }
    }
}

impl BranchList {
    pub fn new(_sender: Sender<crate::Event>) -> Self {
        Object::builder().build()
    }

    pub fn search(&self, term: String) {
        let selected_name = self.get_selected_branch().name;
        let mut current_position = 0;
        let mut deleted = 0;
        // while deleting item section does not called at all
        loop {
            let name = self.imp().list.borrow()[current_position]
                .imp()
                .branch
                .borrow()
                .name
                .clone();
            match name {
                n if n == selected_name => {
                    trace!("this branch is selected!. pos {:?}. will call items changed. deleted {:?}", current_position, deleted);
                    if deleted > 0 {
                        self.items_changed(
                            current_position as u32,
                            deleted,
                            0,
                        );
                    }
                    current_position += 1;
                    deleted = 0;
                }
                n if n.contains(&term) => {
                    trace!("this branch is found!. pos {:?}. will call items changed. deleted {:?}", current_position, deleted);
                    if deleted > 0 {
                        self.items_changed(
                            current_position as u32,
                            deleted,
                            0,
                        );
                    }
                    current_position += 1;
                    deleted = 0;
                }
                n => {
                    self.imp().list.borrow_mut().remove(current_position);
                    deleted += 1;
                    trace!(
                        "remove from list {:?}. tot deleted {:?}",
                        n,
                        deleted
                    );
                }
            }
            if current_position == self.imp().list.borrow().len() {
                trace!(
                    "looop is over! ============= {:?}",
                    self.imp().remote_start_pos
                );
                if deleted > 0 {
                    trace!(
                        "call final delete. pos {:?} deleted {:?}",
                        current_position,
                        deleted
                    );
                    self.items_changed(current_position as u32, deleted, 0);
                }
                break;
            }
            trace!("");
        }
    }

    pub fn reset_search(&self) {
        if self.imp().list.borrow().len()
            == self.imp().original_list.borrow().len()
        {
            return;
        }
        let names: Vec<String> = self
            .imp()
            .list
            .borrow()
            .iter()
            .map(|bi| bi.imp().branch.borrow().name.clone())
            .collect();
        trace!("reset search in list. items in current list {:?}", names);
        let mut added = 0;
        let mut changed_pos = 0;
        let mut current_pos = 0;
        // it need to set remote section properly
        let mut pos = 0;
        for item in self.imp().list.borrow().iter() {
            if item.imp().branch.borrow().branch_type == BranchType::Remote {
                self.imp().remote_start_pos.replace(Some(pos));
                break;
            }
            pos += 1;
        }

        let mut remote_tracked: bool = false;
        for item in self.imp().original_list.borrow().iter() {
            trace!(
                "looooooooop current_pos {:?} changed_pos {:?}",
                current_pos,
                changed_pos
            );
            let name = &item.imp().branch.borrow().name;
            if names.contains(name) {
                // do not add this item. will step over it"
                // but it need to signal about all added items before it
                // since last changed.
                trace!("!!!!!!!!!!!!!!existing item name {:?} cureent pos {:?} changed pos {:?} added {:?}",
                       name,
                       current_pos,
                       changed_pos,
                       added);
                self.items_changed(changed_pos, 0, added);
                added = 0;
                // next changed position wil be after this item
                changed_pos = current_pos + 1;
            } else {
                let is_remote = item.imp().branch.borrow().branch_type
                    == BranchType::Remote;
                if is_remote {
                    if !remote_tracked && is_remote {
                        trace!(
                            "track remote position prev {:?}, new {:?}",
                            self.imp().remote_start_pos,
                            current_pos
                        );
                        self.imp().remote_start_pos.replace(Some(current_pos));
                        remote_tracked = true;
                        // lets try update items here
                        // but it does now work also :(
                        self.items_changed(changed_pos, 0, added);
                        changed_pos = current_pos;
                        added = 0;
                    }
                } else {
                    trace!(
                        "insert local. remote start pos {:?}",
                        self.imp().remote_start_pos
                    );
                    if self.imp().remote_start_pos.borrow().is_some() {
                        let current =
                            self.imp().remote_start_pos.borrow().unwrap();
                        trace!("................increment remote!");
                        self.imp().remote_start_pos.replace(Some(current + 1));
                    }
                }
                self.imp()
                    .list
                    .borrow_mut()
                    .insert(current_pos as usize, item.clone());
                added += 1;
                trace!(
                    "just inserted new item {:?}. total added {:?}",
                    name,
                    added
                );
            }
            current_pos += 1;
            trace!("");
        }
        if added > 0 {
            trace!("fiiiiiiiiiiiiiiiiinal add {:?} {:?}", changed_pos, added);
            self.items_changed(changed_pos, 0, added);
        }
        // trace!("1");
        // self.sections_changed(8, 1);
        trace!("++++++++++++++++++++++++++++++++++++++++");
        // trace!("2");
        // self.sections_changed(6, 1;)

        // self.imp().list.borrow_mut().retian(|item: &BranchItem| {
        //     item.is_head()
        // });
        // let mut le = 0;
        // for item in self.imp().original_list.borrow().iter() {
        //     if item.is_head() {
        //         continue
        //     }
        //     self.imp().list.borrow_mut().push(item.clone());
        //     le += 1;
        // }
        // self.items_changed(1, 0, le as u32);
    }

    pub fn get_branches(&self, repo_path: std::ffi::OsString) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list => async move {
                let branches: Vec<crate::BranchData> = gio::spawn_blocking(move || {
                    crate::get_branches(repo_path)
                }).await.expect("Task needs to finish successfully.");

                let items: Vec<BranchItem> = branches.into_iter()
                    .map(BranchItem::new)
                    .collect();

                let le = items.len() as u32;
                let mut remote_start_pos: Option<u32> = None;
                let mut selected = 0;
                for (pos, item) in items.into_iter().enumerate() {
                    if remote_start_pos.is_none() && item.imp().branch.borrow().branch_type == BranchType::Remote {
                        remote_start_pos.replace(pos as u32);
                    }
                    if item.imp().branch.borrow().is_head {
                        selected = pos;
                        item.set_initial_focus(true)
                    }
                    branch_list.imp().list.borrow_mut().push(item.clone());
                    branch_list.imp().original_list.borrow_mut().push(item);
                }
                branch_list.imp().remote_start_pos.replace(remote_start_pos);
                branch_list.items_changed(0, 0, le);
                // works via bind to single_selection selected
                branch_list.set_selected_pos(selected as u32);
            })
        });
    }

    pub fn checkout(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move { // , @weak selected_item, @weak current_item
                let selected_pos = branch_list.selected_pos();
                let selected_item = branch_list.item(selected_pos).unwrap();
                let selected_item = selected_item.downcast_ref::<BranchItem>().unwrap();

                let branch_data = selected_item.imp().branch.borrow().clone();
                let local = branch_data.branch_type == BranchType::Local;
                let new_branch_data = gio::spawn_blocking(move || {
                    crate::checkout_branch(repo_path, branch_data, sender)
                }).await;
                if let Ok(new_branch_data) = new_branch_data {
                    if local {
                        branch_list.deactivate_current_branch();
                        selected_item.imp().branch.replace(new_branch_data);
                        selected_item.set_is_head(true);
                    } else {
                        // local branch already could be in list
                        assert!(new_branch_data.branch_type == BranchType::Local);
                        let new_name = &new_branch_data.name;
                        // lets check all items in list
                        for i in 0..branch_list.n_items() {
                            if let Some(item) = branch_list.item(i) {
                                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                                if &branch_item.imp().branch.borrow().name == new_name {

                                    if !branch_item.is_head() {
                                        // new head will be set
                                        branch_list.deactivate_current_branch();
                                    } else {
                                        // e.g. current branch is master and
                                        // user chekout origin master
                                    }
                                    branch_item.imp().branch.replace(new_branch_data);
                                    branch_item.set_initial_focus(true);
                                    branch_item.set_is_head(true);
                                    branch_list.set_selected_pos(i);
                                    return;
                                }
                            }
                        }
                        branch_list.deactivate_current_branch();
                        // create new branch
                        branch_list.add_new_branch_item(new_branch_data);
                    }
                } else {
                    crate::display_error(&window, "can't checkout branch");
                }
            })
        });
    }

    pub fn deactivate_current_branch(&self) {
        for branch_item in self.imp().list.borrow().iter() {
            if branch_item.is_head() {
                branch_item.imp().branch.borrow_mut().is_head = false;
                // to trigger render for avatar icon
                branch_item.set_is_head(false);
                return;
            }
        }
        panic!("cant update current branch");
    }

    pub fn update_current_branch(&self, branch_data: crate::BranchData) {
        for branch_item in self.imp().list.borrow().iter() {
            if branch_item.is_head() {
                branch_item.imp().branch.replace(branch_data.clone());
                // to trigger render for avatar icon
                branch_item.set_is_head(branch_item.is_head());
                return;
            }
        }
        panic!("cant update current branch");
    }

    pub fn get_selected_branch(&self) -> crate::BranchData {
        let pos = self.selected_pos();
        let item = self.item(pos).unwrap();
        let branch_item = item.downcast_ref::<BranchItem>().unwrap();
        let data = branch_item.imp().branch.borrow().clone();
        data
    }

    pub fn get_current_branch(&self) -> Option<crate::BranchData> {
        let mut result = None;
        for branch_item in self.imp().list.borrow().iter() {
            if branch_item.is_head() {
                result.replace(branch_item.imp().branch.borrow().clone());
            }
        }
        result
    }

    pub fn cherry_pick(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move {
                let branch_data = branch_list.get_selected_branch();
                let result = gio::spawn_blocking(move || {
                    crate::cherry_pick(repo_path, branch_data, sender)
                }).await;
                let mut err_message = String::from("git error");
                if let Ok(git_result) = result {
                    match git_result {
                        Ok(branch_data) => {
                            trace!("just cherry picked and this is branch data {:?}", branch_data);
                            branch_list.update_current_branch(branch_data);
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                crate::display_error(&window, &err_message);
            })
        });
    }

    pub fn update_remote(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        trace!("update remote!");
        let le = self.imp().list.borrow().len();
        self.imp().list.borrow_mut().clear();
        self.imp().original_list.borrow_mut().clear();
        self.items_changed(0, le as u32, 0);
        glib::spawn_future_local({
            let path = repo_path.clone();
            clone!(@weak self as branch_list, @weak window as window => async move {
                let _ = gio::spawn_blocking(move || {
                    crate::update_remote(repo_path, sender, None)
                }).await;
                branch_list.get_branches(path);
            })
        });
    }

    pub fn merge(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        let current_branch = self.get_current_branch().expect("cant get current branch");
        let selected_branch = self.get_selected_branch();
        if selected_branch.is_head {
            return
        }
        let title = format!(
            "merge branch {} into {}",
            selected_branch.name, current_branch.name
        );

        glib::spawn_future_local({
            clone!(@weak self as branch_list,
            @weak window as window,
            @strong selected_branch as branch_data => async move {
                let dialog = crate::make_confirm_dialog(
                    &window,
                    Some(&Label::new(Some(&title))),
                    "Merge",
                    "Merge"
                );
                let result = dialog.choose_future().await;
                if "confirm" != result {
                    return;
                }
                let result = gio::spawn_blocking(move || {
                    crate::merge(repo_path, branch_data, sender)
                }).await;

                if let Ok(branch_data) = result {
                    trace!("just merged and this is branch data {:?}", branch_data);
                    branch_list.update_current_branch(branch_data);
                    window.close();
                } else {
                    crate::display_error(&window, "error in merge");
                }
            })
        });
    }

    pub fn kill_branch(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move {
                let pos = branch_list.selected_pos();
                let branch_data = branch_list.get_selected_branch();
                if branch_data.is_head {
                    return
                }
                let kind = branch_data.branch_type;
                let result = gio::spawn_blocking(move || {
                    crate::kill_branch(repo_path, branch_data, sender)
                }).await;
                let mut err_message = String::from("git error");
                if let Ok(git_result) = result {
                    match git_result {
                        Ok(_) => {
                            {
                                // put borrow in block
                                branch_list.imp().list.borrow_mut().remove(pos as usize);
                                if kind == BranchType::Local {
                                    let mut pos = branch_list.imp().remote_start_pos.borrow_mut();
                                    if let Some(mut rem_pos) = *pos {
                                        rem_pos -= 1;
                                        pos.replace(rem_pos);
                                        debug!("branches.replace rem pos {:?} {:?}", rem_pos, pos);
                                    }
                                }
                            }
                            let shifted_item = branch_list.item(pos);
                            debug!("branches. removed item at pos {:?}", pos);
                            let mut new_pos = pos;
                            if let Some(item) = shifted_item {
                                debug!("branches.shift item");
                                // next item in list will shift to this position
                                // and must get focus
                                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                                branch_item.set_initial_focus(true);
                                // if not select new_pos there will be panic in transform_to
                                // there will be no value (no item) in selected-item
                                // during items_changed
                                branch_list.set_selected_pos(0);
                            } else {
                                new_pos = {
                                    if pos > 1 {
                                        pos - 1
                                    } else {
                                        0
                                    }
                                };
                                debug!("branches.got last item. decrement pos {:?}", new_pos);
                                let prev_item = branch_list.item(new_pos).unwrap();
                                let branch_item = prev_item.downcast_ref::<BranchItem>().unwrap();
                                branch_item.set_initial_focus(true);
                                branch_list.set_selected_pos(new_pos);
                            }
                            debug!("call items cganged with pos {:?}. new pos will be {:?}", pos, new_pos);
                            branch_list.items_changed(pos, 1, 0);
                            // restore selected position to next one
                            // will will get focus
                            // when delete LAST list item, next expr has no effect:
                            // there will be item with overflown position
                            // connect_selected_notify and cursor will jump
                            // to first position
                            branch_list.set_selected_pos(new_pos);
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                crate::display_error(&window, &err_message);
            })
        });
    }

    pub fn create_branch(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        branch_sender: Sender<Event>,
        sender: Sender<crate::Event>,
    ) {
        let selected_branch = self.get_selected_branch();
        let title =
            format!("create new branch starting at {}", selected_branch.name);

        glib::spawn_future_local({
            clone!(@weak self as branch_list,
            @strong selected_branch as branch_data,
            @weak window as window => async move {

                let lb = ListBox::builder()
                    .selection_mode(SelectionMode::None)
                    .css_classes(vec![String::from("boxed-list")])
                    .build();
                // let title = ActionRow::builder()
                //     .activatable(false)
                //     .selectable(false)
                //     .title(title)
                //     .build();
                let input = EntryRow::builder()
                    .title("New branch name:")
                    .css_classes(vec!["input_field"])
                    .build();
                let checkout = SwitchRow::builder()
                    .title("Checkout")
                    .css_classes(vec!["input_field"])
                    .active(true)
                    .build();
                lb.append(&input);
                lb.append(&checkout);
                let dialog = crate::make_confirm_dialog(
                    &window,
                    Some(&lb),
                    &title,
                    "Create"
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
                let new_branch_name = format!("{}", input.text());
                let need_checkout = checkout.is_active();
                let result = gio::spawn_blocking(move || {
                    crate::create_branch(repo_path, new_branch_name, need_checkout, branch_data, sender)
                }).await;
                if let Ok(branch_data) = result {
                    branch_list.deactivate_current_branch();
                    branch_list.add_new_branch_item(branch_data);
                } else {
                    crate::display_error(&window, "cant create branch");
                }
            })
        });
    }

    fn add_new_branch_item(&self, branch_data: crate::BranchData) {

        let new_item = BranchItem::new(branch_data);

        let new_branch_item =
            new_item.downcast_ref::<BranchItem>().unwrap();
        new_branch_item.set_initial_focus(true);

        {
            // put borrow in block
            self.imp().list.borrow_mut().insert(0, new_item);
            let mut pos = self.imp().remote_start_pos.borrow_mut();
            if let Some(mut rem_pos) = *pos {
                rem_pos += 1;
                pos.replace(rem_pos);
                trace!("branches. replace rem pos {:?} {:?}", rem_pos, pos);
            }
        }
        self.items_changed(0, 0, 1);
        // works via bind to single_selection selected ?????
        self.set_selected_pos(0);
    }
}

pub fn make_header_factory() -> SignalListItemFactory {
    let header_factory = SignalListItemFactory::new();
    header_factory.connect_setup(move |_, list_header| {
        let label = Label::new(None);
        let list_header = list_header
            .downcast_ref::<ListHeader>()
            .expect("Needs to be ListHeader");
        list_header.set_child(Some(&label));
        list_header.connect_item_notify(move |lh| {
            if lh.item().is_none() {
                return;
            }
            let ob = lh.item().unwrap();
            let item: &BranchItem = ob.downcast_ref::<BranchItem>().unwrap();

            let title = match item.imp().branch.borrow().branch_type {
                BranchType::Local => "Branches",
                BranchType::Remote => "Remote branches",
            };
            label.set_label(&title);
        });
    });
    header_factory
}

pub fn make_item_factory() -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, list_item| {
        let image = Image::new();
        image.set_margin_top(4);
        let spinner = Spinner::new();
        spinner.set_visible(false);
        // spinner.set_spinning(true);
        let label_title = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(36)
            .max_width_chars(36)
            .ellipsize(pango::EllipsizeMode::End)
            //.selectable(true)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();
        let label_commit = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(36)
            .max_width_chars(36)
            .ellipsize(pango::EllipsizeMode::End)
            //.selectable(true)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();
        let label_dt = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(24)
            .max_width_chars(24)
            .ellipsize(pango::EllipsizeMode::End)
            //.selectable(true)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();

        let bx = Box::builder()
            .orientation(Orientation::Horizontal)
            // .css_classes(vec![String::from("branch_row")])
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(2)
            .margin_end(2)
            .spacing(12)
            .can_focus(true)
            .focusable(true)
            .build();
        bx.append(&image);
        bx.append(&spinner);
        bx.append(&label_title);
        bx.append(&label_commit);
        bx.append(&label_dt);

        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));
        list_item.set_selectable(true);
        list_item.set_activatable(true);
        list_item.set_focusable(true);

        list_item
            .bind_property("selected", &bx, "css_classes")
            .transform_to(move |_, is_selected: bool| {
                if is_selected {
                    Some(vec![String::from("branch_row")])
                } else {
                    Some(vec![])
                }
            })
            .build();

        list_item.connect_selected_notify(|li: &ListItem| {
            // grab focus only once on list init
            if let Some(item) = li.item() {
                // li.child().expect("no child").set_css_classes(&vec!["branch_row"]);
                let branch_item = item.downcast_ref::<BranchItem>().unwrap();
                trace!(
                    "item in connect selected {:?} {:?} {:?}",
                    branch_item.title(),
                    branch_item.initial_focus(),
                    li.position()
                );
                if branch_item.initial_focus() {
                    li.child().unwrap().grab_focus();
                    branch_item.set_initial_focus(false)
                }
            } else {
                trace!(
                    "branches. no item in connect_selected_notify {:?}",
                    li.position()
                );
            }
        });

        let item = list_item.property_expression("item");

        item.chain_property::<BranchItem>("is-head")// was "is_head"! it works also!
            .chain_closure::<String>(closure!(
                |_: Option<Object>, is_head: bool| {
                    if is_head {
                        String::from("avatar-default-symbolic")
                    } else {
                        String::from("")
                    }
                }
            ))
            .bind(&image, "icon-name", Widget::NONE);
        item.chain_property::<BranchItem>("title").bind(
            &label_title,
            "label",
            Widget::NONE,
        );

        item.chain_property::<BranchItem>("last-commit").bind(
            &label_commit,
            "label",
            Widget::NONE,
        );

        item.chain_property::<BranchItem>("dt").bind(
            &label_dt,
            "label",
            Widget::NONE,
        );
    });

    factory
}

pub fn make_list_view(
    repo_path: std::ffi::OsString,
    sender: Sender<crate::Event>,
) -> ListView {
    let header_factory = make_header_factory();
    let factory = make_item_factory();

    let branch_list = BranchList::new(sender.clone());

    let selection_model = SingleSelection::new(Some(branch_list));
    selection_model.set_autoselect(false);

    let model = selection_model.model().unwrap();
    let bind =
        selection_model.bind_property("selected", &model, "selected_pos");
    let _ = bind.bidirectional().build();

    let branch_list = model.downcast_ref::<BranchList>().unwrap();
    branch_list.get_branches(repo_path.clone());

    let list_view = ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .header_factory(&header_factory)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .show_separators(true)
        .build();

    list_view.connect_activate(move |lv: &ListView, pos: u32| {
        let root = lv.root().unwrap();
        let window = root.downcast_ref::<Window>().unwrap();
        let selection_model = lv.model().unwrap();
        let single_selection =
            selection_model.downcast_ref::<SingleSelection>().unwrap();
        let list_model = single_selection.model().unwrap();
        let branch_list = list_model.downcast_ref::<BranchList>().unwrap();
        branch_list.checkout(
            repo_path.clone(),
            window,
            sender.clone(),
        );
    });
    list_view.add_css_class("stage");
    list_view
}

pub fn make_headerbar(
    _repo_path: std::ffi::OsString,
    list_view: &ListView,
    sender: Sender<Event>,
) -> HeaderBar {
    let hb = HeaderBar::builder().build();

    let entry = SearchEntry::builder()
        .search_delay(300)
        .placeholder_text("hit s for search")
        .build();
    entry.connect_stop_search(|e| {
        e.stop_signal_emission_by_name("stop-search");
    });
    let branch_list = get_branch_list(list_view);

    entry.connect_search_changed(clone!(@weak branch_list => move |e| {
        let term = e.text();
        if !term.is_empty() && term.len() < 3 {
            return;
        }
        if term.is_empty() {
            branch_list.reset_search();
        } else {
            branch_list.search(term.into());
        }

    }));
    let search = SearchBar::builder()
        .tooltip_text("search branches")
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(false)
        .child(&entry)
        .build();

    // search.connect_entry(&entry);
    // search.set_child(Some(&entry));

    let new_btn = Button::builder()
        // .label("N")
        .icon_name("list-add-symbolic")
        .can_shrink(true)
        .tooltip_text("Create branch (N)")
        .sensitive(true)
        .use_underline(true)
        // .action_name("branches.new")
        .build();
    new_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::Create)
                .expect("Could not send through channel");
        }
    });
    let kill_btn = Button::builder()
        .icon_name("user-trash-symbolic") // process-stop-symbolic
        .use_underline(true)
        .tooltip_text("Delete branch (K)")
        .sensitive(false)
        .can_shrink(true)
        .build();

    let set_sensitive = |bind: &glib::Binding, position: u32| {
        let src = bind.source().unwrap();
        let li: &BranchList = src.downcast_ref().unwrap();
        let item = li.item(position);
        if let Some(item) = item {
            let branch_item = item.downcast_ref::<BranchItem>().unwrap();
            Some(!branch_item.is_head())
        } else {
            Some(false)
        }
    };
    let _ = branch_list
        .bind_property("selected-pos", &kill_btn, "sensitive")
        .transform_to(set_sensitive)
        .build();
    // let _ = branch_list
    //     .bind_property("current-pos", &kill_btn, "sensitive")
    //     .transform_to(set_sensitive)
    //     .build();
    kill_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::Kill)
                .expect("Could not send through channel");
        }
    });
    let merge_btn = Button::builder()
        .icon_name("org.gtk.gtk4.NodeEditor-symbolic")
        .use_underline(true)
        .tooltip_text("Merge branch (M)")
        .sensitive(false)
        .can_shrink(true)
        .build();
    let _ = branch_list
        .bind_property("selected-pos", &merge_btn, "sensitive")
        .transform_to(set_sensitive)
        .build();
    // let _ = branch_list
    //     .bind_property("current-pos", &merge_btn, "sensitive")
    //     .transform_to(set_sensitive)
    //     .build();
    merge_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::Merge)
                .expect("Could not send through channel");
        }
    });

    let refresh_btn = Button::builder()
        .label("Update remote")
        .icon_name("view-refresh-symbolic")
        .use_underline(true)
        .tooltip_text("Update remote")
        .sensitive(true)
        .can_shrink(true)
        .build();

    refresh_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::UpdateRemote)
                .expect("Could not send through channel");
        }
    });

    hb.set_title_widget(Some(&search));
    hb.pack_end(&new_btn);
    hb.pack_end(&merge_btn);
    hb.pack_end(&kill_btn);
    hb.pack_end(&refresh_btn);
    hb.set_show_end_title_buttons(true);
    hb.set_show_back_button(true);
    hb
}

pub enum Event {
    Create,
    Scroll(u32),
    Kill,
    Merge,
    CherryPickRequest,
    UpdateRemote
}

pub fn get_branch_list(list_view: &ListView) -> BranchList {
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let branch_list = list_model.downcast_ref::<BranchList>().unwrap();
    branch_list.to_owned()
}

pub fn branches_in_use(
    list_view: &ListView,
) -> (crate::BranchData, crate::BranchData) {
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let branch_list = list_model.downcast_ref::<BranchList>().unwrap();
    (
        branch_list.get_current_branch().expect("cant get current branch"),
        branch_list.get_selected_branch(),
    )
}

pub fn show_branches_window(
    repo_path: std::ffi::OsString,
    app_window: &ApplicationWindow,
    main_sender: Sender<crate::Event>,
) {
    let (sender, receiver) = async_channel::unbounded();

    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let scroll = ScrolledWindow::new();

    let list_view = make_list_view(repo_path.clone(), main_sender.clone());

    let hb = make_headerbar(repo_path.clone(), &list_view, sender.clone());

    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        let sender = sender.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::Escape, _) => {
                    window.close();
                }
                (gdk::Key::n | gdk::Key::c, _) => {
                    sender
                        .send_blocking(Event::Create)
                        .expect("Could not send through channel");
                }
                (gdk::Key::k, _) => {
                    sender
                        .send_blocking(Event::Kill)
                        .expect("Could not send through channel");
                }
                (gdk::Key::m, _) => {
                    sender
                        .send_blocking(Event::Merge)
                        .expect("Could not send through channel");
                }
                (gdk::Key::a, _) => {
                    sender
                        .send_blocking(Event::CherryPickRequest)
                        .expect("Could not send through channel");
                }
                (gdk::Key::r, _) => {
                    sender
                        .send_blocking(Event::UpdateRemote)
                        .expect("Could not send through channel");
                }
                (gdk::Key::s, _) => {
                    let search_bar = hb.title_widget().unwrap();
                    let search_bar =
                        search_bar.downcast_ref::<SearchBar>().unwrap();
                    let search_entry = search_bar.child().unwrap();
                    let search_entry =
                        search_entry.downcast_ref::<SearchEntry>().unwrap();
                    trace!("enter search");
                    search_entry.grab_focus();
                }
                (key, modifier) => {
                    trace!("key pressed {:?} {:?}", key, modifier);
                }
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);

    window.present();
    list_view.grab_focus();

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            match event {
                Event::Create => {
                    trace!("branches. got new branch name");
                    let branch_list = get_branch_list(&list_view);
                    branch_list.create_branch(
                        repo_path.clone(),
                        &window,
                        sender.clone(),
                        main_sender.clone(),
                    );
                }
                Event::Scroll(pos) => {
                    trace!("branches. scroll {:?}", pos);
                    list_view.scroll_to(pos, ListScrollFlags::empty(), None);
                }
                Event::Kill => {
                    trace!("branches. kill request");
                    let branch_list = get_branch_list(&list_view);
                    branch_list.kill_branch(
                        repo_path.clone(),
                        &window,
                        main_sender.clone(),
                    );
                }
                Event::Merge => {
                    trace!("branches. merge");
                    let branch_list = get_branch_list(&list_view);
                    branch_list.merge(
                        repo_path.clone(),
                        &window,
                        main_sender.clone(),
                    )
                }
                Event::UpdateRemote => {
                    trace!("branches. update remote");
                    let branch_list = get_branch_list(&list_view);
                    branch_list.update_remote(
                        repo_path.clone(),
                        &window,
                        main_sender.clone(),
                    )
                }
                Event::CherryPickRequest => {
                    trace!("branches. cherry-pick request");
                    let (current_branch, selected_branch) =
                        branches_in_use(&list_view);
                    let btns = vec!["Cancel", "Cherry-pick"];
                    let alert = AlertDialog::builder()
                        .buttons(btns)
                        .message("Cherry picking")
                        .detail(format!(
                            "Cherry picing commit {:?} from branch {:?} onto branch {:?}",
                            selected_branch.commit_string, selected_branch.name, current_branch.name
                        ))
                        .build();
                    let branch_list = get_branch_list(&list_view);
                    alert.choose(Some(&window), None::<&gio::Cancellable>, {
                        let path = repo_path.clone();
                        let window = window.clone();
                        let sender = main_sender.clone();
                        clone!(@weak branch_list => move |result| {
                            if let Ok(ind) = result {
                                if ind == 1 {
                                    branch_list.cherry_pick(
                                        path,
                                        &window,
                                        sender
                                    )
                                }
                            }
                        })
                    });
                }
            }
        }
    });
}
