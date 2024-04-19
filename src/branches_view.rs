use async_channel::Sender;
use git2::BranchType;
use glib::{clone, closure, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, AlertDialog, Box, Button, CheckButton,
    EventControllerKey, Image, Label, ListBox, ListHeader, ListItem,
    ListScrollFlags, ListView, Orientation, ScrolledWindow, SectionModel,
    SelectionMode, SignalListItemFactory, SingleSelection, Spinner, Widget,
    SearchBar, SearchEntry, FilterListModel
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

        #[property(get, set)]
        pub progress: RefCell<bool>,

        #[property(get, set)]
        pub no_progress: RefCell<bool>,

        #[property(get, set)]
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
    impl ObjectImpl for BranchItem {}
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
            .property("progress", false)
            .property("no-progress", true)
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

        #[property(get, set)]
        pub current_pos: RefCell<u32>,
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
                if position <= pos {
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
        // it need to keep 1 list in item for sure,
        // because transform_to bindingsin this file will fail:
        // closures will got None instead of BranchItem
        // lets remain head branch in list
        let deleted_amount = self.imp().list.borrow().len() - 1;
        self.imp().list.borrow_mut().retain(|item: &BranchItem| {
            item.is_head()
        });

        let mut le = 0;
        for item in self.imp().original_list.borrow().iter() {
            if item.is_head() {
                continue
            }
            if item.imp().branch.borrow().name.contains(&term) {
                self.imp().list.borrow_mut().push(item.clone());
            }
            le += 1;
        }
        self.items_changed(1, 0, le as u32);
    }
    
    pub fn reset_search(&self) {
        if self.imp().list.borrow().len() == self.imp().original_list.borrow().len() {
            return;
        }
        debug!("reset search in list");
        self.imp().list.borrow_mut().retain(|item: &BranchItem| {
            item.is_head()
        });
        let mut le = 0;
        for item in self.imp().original_list.borrow().iter() {
            if item.is_head() {
                continue
            }
            self.imp().list.borrow_mut().push(item.clone());
            le += 1;
        }
        self.items_changed(1, 0, le as u32);
    }
    
    pub fn make_list(&self, repo_path: std::ffi::OsString) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list => async move {
                let branches: Vec<crate::BranchData> = gio::spawn_blocking(move || {
                    crate::get_refs(repo_path)
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
                branch_list.set_current_pos(selected as u32);
            })
        });
    }

    pub fn checkout(
        &self,
        repo_path: std::ffi::OsString,
        selected_item: &BranchItem,
        current_item: &BranchItem, // TODO refactor. get this item from branch_list itself
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window, @weak selected_item, @weak current_item => async move {
                let branch_data = selected_item.imp().branch.borrow().clone();
                let was_local = branch_data.branch_type == BranchType::Local;
                let maybe_new_branch_data = gio::spawn_blocking(move || {
                    crate::checkout(repo_path, branch_data, sender)
                }).await;
                selected_item.set_progress(false);
                if let Ok(new_branch_data) = maybe_new_branch_data {
                    if was_local {
                        // just replace with new data
                        selected_item.imp().branch.replace(new_branch_data);
                        selected_item.set_is_head(true);
                        selected_item.set_no_progress(true);
                        current_item.set_is_head(false);
                        branch_list.set_current_pos(branch_list.selected_pos());
                    } else {
                        // create new branch
                        branch_list.add_new_branch_item(new_branch_data);
                    }
                } else {
                    crate::display_error(&window, "can't checkout branch");
                }
            })
        });
    }

    pub fn update_current_item(&self, branch_data: crate::BranchData) {
        let current_item = self.item(self.current_pos()).unwrap();
        let branch_item = current_item.downcast_ref::<BranchItem>().unwrap();
        branch_item.imp().branch.replace(branch_data);
    }

    pub fn get_selected_branch(&self) -> crate::BranchData {
        let pos = self.selected_pos();
        let item = self.item(pos).unwrap();
        let branch_item = item.downcast_ref::<BranchItem>().unwrap();
        let data = branch_item.imp().branch.borrow().clone();
        data
    }

    pub fn get_current_branch(&self) -> crate::BranchData {
        let pos = self.current_pos();
        let item = self.item(pos).unwrap();
        let branch_item = item.downcast_ref::<BranchItem>().unwrap();
        let data = branch_item.imp().branch.borrow().clone();
        data
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
                            debug!("just cherry picked and this is branch data {:?}", branch_data);
                            branch_list.update_current_item(branch_data);
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                crate::display_error(&window, &err_message);
            })
        });
    }

    pub fn merge(
        &self,
        repo_path: std::ffi::OsString,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        // let btns = vec!["Cancel", "Merge"];
        let current_branch = self.get_current_branch();
        let selected_branch = self.get_selected_branch();
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
                    debug!("just merged and this is branch data {:?}", branch_data);
                    branch_list.update_current_item(branch_data);
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
                                        trace!("branches.replace rem pos {:?} {:?}", rem_pos, pos);
                                    }
                                }
                            }
                            let shifted_item = branch_list.item(pos);
                            trace!("branches. removed item at pos {:?}", pos);
                            let mut new_pos = pos;
                            if let Some(item) = shifted_item {
                                trace!("branches.shift item");
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
                                trace!("branches.got last item. decrement pos {:?}", new_pos);
                                let prev_item = branch_list.item(new_pos).unwrap();
                                let branch_item = prev_item.downcast_ref::<BranchItem>().unwrap();
                                branch_item.set_initial_focus(true);
                                branch_list.set_selected_pos(new_pos);
                            }
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
                    branch_list.add_new_branch_item(branch_data);
                } else {
                    crate::display_error(&window, "cant create branch");
                }
            })
        });
    }
    fn add_new_branch_item(&self, branch_data: crate::BranchData) {
        let new_head = branch_data.is_head;
        if new_head {
            let old_item = self.item(self.current_pos()).unwrap();
            let old_branch_item =
                old_item.downcast_ref::<BranchItem>().unwrap();
            let mut old_data = old_branch_item.imp().branch.borrow_mut();
            old_data.is_head = false;
            old_branch_item.set_is_head(false);
        }
        let new_item = BranchItem::new(branch_data);
        if new_head {
            let new_branch_item =
                new_item.downcast_ref::<BranchItem>().unwrap();
            new_branch_item.set_initial_focus(true);
            new_branch_item.set_is_head(true);
        }
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
        self.set_selected_pos(0);
        self.set_current_pos(0);
    }
}

pub fn make_header_factory() -> SignalListItemFactory {
    let section_title = std::cell::RefCell::new(String::from("Branches"));
    let header_factory = SignalListItemFactory::new();
    header_factory.connect_setup(move |_, list_header| {
        let label = Label::new(Some(&*section_title.borrow()));
        let list_header = list_header
            .downcast_ref::<ListHeader>()
            .expect("Needs to be ListHeader");
        list_header.set_child(Some(&label));
        section_title.replace(String::from("Remotes"));
        // does not work. it is always git first BranchItem
        // why???
        // list_header.connect_item_notify(move |lh| {
        //     let ob = lh.item().unwrap();
        //     let item: &BranchItem = ob
        //         .downcast_ref::<BranchItem>()
        //         .unwrap();
        //     // let title = match item.imp().branch.borrow().branch_type {
        //     //     BranchType::Local => "Branches",
        //     //     BranchType::Remote => "Remote"
        //     // };
        //     // label.set_label(title);
        // });
        // does not work also
        // let item = list_header
        //     .property_expression("item");
        // item.chain_property::<BranchItem>("ref-kind")
        //     .bind(&label, "label", Widget::NONE);
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

        // list_item.bind_property("selected", &bx, "css_classes")
        //     .transform_to(move |_, is_selected: bool| {
        //         if is_selected {
        //             Some(vec![String::from("branch_row")])
        //         } else {
        //             Some(vec![])
        //         }
        //     })
        //     .build();

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
        item.chain_property::<BranchItem>("is_head")
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
        item.chain_property::<BranchItem>("progress").bind(
            &spinner,
            "visible",
            Widget::NONE,
        );
        item.chain_property::<BranchItem>("progress").bind(
            &spinner,
            "spinning",
            Widget::NONE,
        );
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
    // selection_model.connect_selected_item_notify(|single_selection| {
    //     // let ss_pos = single_selection.selected();
    //     // debug!("selected_item_notify............. {:?}", ss_pos);
    //     // if let Some(item) = single_selection.selected_item() {
    //     //     let branch_item = item.downcast_ref::<BranchItem>().unwrap();
    //     //     branch_item.imp().add_css_class("selected");
    //     // }
    // });

    let branch_list = model.downcast_ref::<BranchList>().unwrap();
    branch_list.make_list(repo_path.clone());

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
        let selection_model = lv.model().unwrap();
        let single_selection =
            selection_model.downcast_ref::<SingleSelection>().unwrap();
        let list_model = single_selection.model().unwrap();
        let branch_list = list_model.downcast_ref::<BranchList>().unwrap();

        let item_ob = selection_model.item(pos);
        let mut current_item: Option<&BranchItem> = None;
        if let Some(item) = item_ob {
            let list = branch_list.imp().list.borrow();
            let activated_branch_item =
                item.downcast_ref::<BranchItem>().unwrap();
            if activated_branch_item.is_head() {
                return;
            }
            for branch_item in list.iter() {
                if branch_item.is_head() {
                    current_item.replace(branch_item);
                }
                branch_item.set_progress(false);
                branch_item.set_no_progress(true);
            }
            activated_branch_item.set_progress(true);
            activated_branch_item.set_no_progress(false);
            let root = lv.root().unwrap();
            let window = root.downcast_ref::<Window>().unwrap();
            trace!(
                "branches checkout! {:?} {:?}",
                single_selection.selected(),
                branch_list.selected_pos()
            );
            branch_list.checkout(
                repo_path.clone(),
                activated_branch_item,
                // got panic here!
                current_item.unwrap(),
                window,
                sender.clone(),
            );
        }
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
        .build();
    // entry.connect_stop_search(|e| {
    //     // does not work
    // });
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
        .child(&entry)
        .build();
    
    search.connect_entry(&entry);
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
        .icon_name("user-trash-symbolic")// process-stop-symbolic
        .use_underline(true)
        .tooltip_text("Delete branch (K)")
        .sensitive(false)
        .can_shrink(true)
        .build();

    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();

    let _ = single_selection
        .bind_property("selected-item", &kill_btn, "sensitive")
        .transform_to(move |_, item: BranchItem| {
            Some(!item.is_head())
        }).build();
    kill_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::Kill)
                .expect("Could not send through channel");
        }
    });
    let merge_btn = Button::builder()
        // .label("M")
        .icon_name("org.gtk.gtk4.NodeEditor-symbolic")
        .use_underline(true)
        .tooltip_text("Merge branch (M)")
        .sensitive(false)
        .can_shrink(true)
        .build();
    let _ = single_selection
        .bind_property("selected-item", &merge_btn, "sensitive")
        .transform_to(move |_, item: BranchItem| {
            Some(!item.is_head())
        }).build();
    merge_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(Event::Merge)
                .expect("Could not send through channel");
        }
    });

    // hb.set_title_widget(Some(&lbl));
    hb.set_title_widget(Some(&search));
    hb.pack_end(&new_btn);
    hb.pack_end(&merge_btn);
    hb.pack_end(&kill_btn);
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
        branch_list.get_current_branch(),
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
                (gdk::Key::n|gdk::Key::c, _) => {
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
                _ => {}
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
