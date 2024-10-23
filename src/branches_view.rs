// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use async_channel::Sender;

use crate::dialogs::alert;
use crate::git::{branch, merge, rebase, remote};
use crate::{DARK_CLASS, LIGHT_CLASS};
use git2::BranchType;
use glib::{clone, closure, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, Align, Box, Button, EventControllerKey, Image, Label, ListBox,
    ListHeader, ListItem, ListView, Orientation, ScrolledWindow, SearchBar, SearchEntry,
    SectionModel, SelectionMode, SignalListItemFactory, SingleSelection, Spinner, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, EntryRow, HeaderBar, StyleManager, SwitchRow, ToolbarView, Window,
};

use log::{debug, info, trace};
use std::path::PathBuf;

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
        pub branch: RefCell<super::branch::BranchData>,

        #[property(get, set)]
        pub initial_focus: RefCell<bool>,

        #[property(get = Self::get_branch_is_head, set = Self::set_branch_is_head)]
        pub is_head: RefCell<bool>,

        #[property(get = Self::get_branch_is_local)]
        pub is_local: RefCell<bool>,

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

        pub fn get_branch_is_local(&self) -> bool {
            self.branch.borrow().branch_type == git2::BranchType::Local
        }
    }
}

impl BranchItem {
    pub fn new(branch: &branch::BranchData, is_dark: bool) -> Self {
        let color = if StyleManager::default().is_dark() {
            "#839daf"
        } else {
            "#4a708b"
        };
        let ob = Object::builder::<BranchItem>()
            .property(
                "title",
                format!("<span color=\"{}\">{}</span>", color, &branch.name.to_str()),
            )
            .property("last-commit", &branch.log_message)
            .property("dt", branch.commit_dt.to_string())
            .property("initial-focus", false)
            .build();
        ob.imp().branch.replace(branch.clone());
        ob
    }
}

glib::wrapper! {
    pub struct BranchList(ObjectSubclass<branch_list::BranchList>)
        @implements gio::ListModel, SectionModel; // , FilterListModel
}

pub struct SpinnerWrapper {
    spin: std::boxed::Box<dyn FnMut()>,
}

impl Default for SpinnerWrapper {
    fn default() -> Self {
        SpinnerWrapper {
            spin: std::boxed::Box::new(|| {}),
        }
    }
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
        pub original_list: RefCell<Vec<super::branch::BranchData>>,
        pub list: RefCell<Vec<super::BranchItem>>,

        #[property(get, set)]
        pub selected_pos: RefCell<u32>,

        pub spinner: RefCell<super::SpinnerWrapper>,
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
            let remote_pos = self.list.borrow().iter().fold(0, |acc, bi| {
                if bi.is_local() {
                    return acc + 1;
                }
                acc
            });
            if position < remote_pos {
                (0, remote_pos)
            } else {
                return (remote_pos, self.list.borrow().len() as u32);
            }
        }
    }
}

impl BranchList {
    pub fn new(_sender: Sender<crate::Event>) -> Self {
        Object::builder().build()
    }

    pub fn set_spinner(&self, spinner: SpinnerWrapper) {
        self.imp().spinner.replace(spinner);
    }
    pub fn toggle_spinner(&self) {
        (self.imp().spinner.borrow_mut().spin)();
    }

    pub fn search_new(&self, term: String) {
        let orig_le = self.imp().list.take().len();
        self.items_changed(0, orig_le as u32, 0);
        let is_dark = StyleManager::default().is_dark();
        self.imp().list.replace(
            self.imp()
                .original_list
                .borrow()
                .iter()
                .filter(|bd| bd.name.to_str().contains(&term))
                .map(|b| BranchItem::new(b, is_dark))
                .collect(),
        );
        self.items_changed(0, 0, self.imp().list.borrow().len() as u32);
    }

    pub fn get_branches(
        &self,
        repo_path: PathBuf,
        branches: Option<Vec<branch::BranchData>>,
        window: &Window,
    ) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window => async move {
                let branches = branches.unwrap_or(gio::spawn_blocking(move || {
                    branch::get_branches(repo_path)
                }).await.unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(Vec::new())
                }).unwrap_or_else(|e| {
                    alert(e).present(&window);
                    Vec::new()
                }));
                if branches.is_empty() {
                    return;
                }
                branch_list.imp().original_list.replace(branches);
                let is_dark = StyleManager::default().is_dark();
                branch_list.imp().list.replace(
                    branch_list.imp().original_list.borrow()
                        .iter()
                        .map(|b|BranchItem::new(b, is_dark))
                        .collect()
                );
                branch_list.items_changed(0, 0, branch_list.imp().list.borrow().len() as u32);

            })
        });
    }

    pub fn checkout(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move {
                let selected_pos = branch_list.selected_pos();
                let selected_item = branch_list.item(selected_pos).unwrap();
                let selected_item = selected_item.downcast_ref::<BranchItem>().unwrap();

                let branch_data = selected_item.imp().branch.borrow().clone();
                let new_branch_data = gio::spawn_blocking(move || {
                    branch::checkout_branch(repo_path, branch_data, sender)
                }).await.unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(None)
                }).unwrap_or_else(|e| {
                    alert(e).present(&window);
                    None
                });

                if new_branch_data.is_none() {
                    info!("branch. exit after error");
                    return;
                }
                let new_branch_data = new_branch_data.unwrap();
                if branch_list.imp().original_list.borrow().iter().find(|b| {
                    b.name == new_branch_data.name
                }).is_some() {
                    branch_list.update_head_branch(new_branch_data);
                } else {
                    branch_list.add_new_branch_item(new_branch_data, true);
                };
            })
        });
    }

    pub fn update_head_branch(&self, branch_data: branch::BranchData) {
        // replace original head branch
        let new_original_list = self
            .imp()
            .original_list
            .borrow()
            .clone()
            .into_iter()
            .map(|mut bd| {
                if bd.name == branch_data.name {
                    branch_data.clone()
                } else {
                    bd.is_head = false;
                    bd
                }
            })
            .collect();
        self.imp().original_list.replace(new_original_list);
        self.imp().list.borrow().iter().for_each(|bi| {
            if bi.imp().branch.borrow().name == branch_data.name {
                bi.imp().branch.replace(branch_data.clone());
            } else {
                bi.imp().branch.borrow_mut().is_head = false;
            }
            // trigger avatars via fake property
            bi.set_is_head(bi.is_head());
        });
    }

    pub fn get_selected_branch(&self) -> branch::BranchData {
        let pos = self.selected_pos();
        // TODO! got panic here while opening large
        // list of branches and clicking create
        // got it twice!
        let item = self.item(pos).unwrap();
        let branch_item = item.downcast_ref::<BranchItem>().unwrap();
        let data = branch_item.imp().branch.borrow().clone();
        data
    }

    pub fn get_head_branch(&self) -> Option<branch::BranchData> {
        if let Some(head_branch) = self
            .imp()
            .original_list
            .borrow()
            .iter()
            .max_by_key(|i| i.is_head)
        {
            return Some(head_branch.clone());
        }
        None
    }

    pub fn update_remote(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        trace!("update remote!");
        self.toggle_spinner();
        let le = self.imp().list.borrow().len();
        self.imp().list.borrow_mut().clear();
        self.imp().original_list.borrow_mut().clear();
        self.items_changed(0, le as u32, 0);
        glib::spawn_future_local({
            let path = repo_path.clone();
            clone!(@weak self as branch_list, @weak window as window => async move {
                let _ = gio::spawn_blocking(move || {
                    remote::update_remote(repo_path, sender, None)
                }).await;
                branch_list.toggle_spinner();
                branch_list.get_branches(path, None, &window);
            })
        });
    }

    pub fn rebase(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        let current_branch = self.get_head_branch().expect("cant get current branch");
        let selected_branch = self.get_selected_branch();
        if selected_branch.is_head {
            return;
        }
        let title = format!(
            "rebase branch {} onto {}",
            current_branch.name.to_str(),
            selected_branch.name.to_str()
        );

        glib::spawn_future_local({
            clone!(@weak self as branch_list,
            @weak window as window,
            @strong selected_branch as branch_data => async move {
                let dialog = crate::confirm_dialog_factory(
                    &window,
                    Some(&Label::new(Some(&title))),
                    "Rebase",
                    "Rebase"
                );
                let result = dialog.choose_future().await;
                if "confirm" != result {
                    return;
                }
                gio::spawn_blocking(move || {
                    rebase(repo_path, branch_data.oid, None, sender)
                }).await.unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(false)
                }).unwrap_or_else(|e| {
                    alert(e).present(&window);
                    false
                });
                window.close();
            })
        });
    }

    pub fn merge(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        let current_branch = self.get_head_branch().expect("cant get current branch");
        let selected_branch = self.get_selected_branch();
        if selected_branch.is_head {
            return;
        }
        let title = format!(
            "merge branch {} into {}",
            selected_branch.name.to_str(),
            current_branch.name.to_str()
        );

        glib::spawn_future_local({
            clone!(@weak self as branch_list,
            @weak window as window,
            @strong selected_branch as branch_data => async move {
                let dialog = crate::confirm_dialog_factory(
                    &window,
                    Some(&Label::new(Some(&title))),
                    "Merge",
                    "Merge"
                );
                let result = dialog.choose_future().await;
                if "confirm" != result {
                    return;
                }
                let branch_data = gio::spawn_blocking(move || {
                    merge::branch(repo_path, branch_data, sender, None)
                }).await.unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(None)
                }).unwrap_or_else(|e| {
                    alert(e).present(&window);
                    None
                });
                if let Some(branch_data) = branch_data {
                    debug!("just merged and this is branch data {:?}", branch_data);
                    branch_list.update_head_branch(branch_data);
                }
                window.close();
            })
        });
    }

    pub fn kill_branch(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window => async move {
                let pos = branch_list.selected_pos();
                let branch_data = branch_list.get_selected_branch();
                if branch_data.is_head {
                    return
                }
                let name = branch_data.name.clone();
                let result = gio::spawn_blocking(move || {
                    branch::kill_branch(repo_path, branch_data, sender)
                }).await.unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(None)
                }).unwrap_or_else(|e| {
                    alert(e).present(&window);
                    None
                });
                if result.is_none() {
                    return
                }

                // put borrow in block
                branch_list.imp().list.borrow_mut().remove(pos as usize);
                branch_list.imp().original_list.borrow_mut().retain(|bd| {
                    bd.name != name
                });

                // --------------------------- very strange piece

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

                // --------------------------- very strange piece

            })
        });
    }

    pub fn create_branch(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        let selected_branch = self.get_selected_branch();
        let title = format!(
            "create new branch starting at {}",
            selected_branch.name.to_str()
        );

        glib::spawn_future_local({
            clone!(@weak self as branch_list,
            @strong selected_branch as branch_data,
            @weak window as window => async move {

                let lb = ListBox::builder()
                    .selection_mode(SelectionMode::None)
                    .css_classes(vec![String::from("boxed-list")])
                    .build();
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
                let dialog = crate::confirm_dialog_factory(
                    &window,
                    Some(&lb),
                    &title,
                    "Create"
                );
                input.connect_apply(clone!(@strong dialog as dialog => move |_entry| {
                    // someone pressed enter
                    dialog.response("confirm");
                    dialog.close();
                }));
                input.connect_entry_activated(clone!(@strong dialog as dialog => move |_entry| {
                    // someone pressed enter
                    dialog.response("confirm");
                    dialog.close();
                }));

                if "confirm" != dialog.choose_future().await {
                    return;
                }
                let new_branch_name = format!("{}", input.text());
                let need_checkout = checkout.is_active();
                let branch_data = gio::spawn_blocking(move || {
                    branch::create_branch(repo_path, new_branch_name, need_checkout, branch_data, sender)
                }).await.unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(None)
                }).unwrap_or_else(|e| {
                    alert(e).present(&window);
                    None
                });
                if let Some(branch_data) = branch_data {
                    // aganzha what about optional checkout?
                    branch_list.add_new_branch_item(branch_data, need_checkout);
                }
            })
        });
    }

    fn add_new_branch_item(&self, branch_data: branch::BranchData, need_checkout: bool) {
        debug!(
            "add_new_branch_item {:?} {:?}",
            branch_data.is_head, branch_data.name
        );
        self.imp()
            .original_list
            .borrow_mut()
            .insert(0, branch_data.clone());
        debug!("inserted in original list!");
        self.imp().list.borrow_mut().insert(
            0,
            BranchItem::new(
                &self.imp().original_list.borrow()[0],
                StyleManager::default().is_dark(),
            ),
        );

        if !need_checkout {
            self.items_changed(0, 0, 1);
            return;
        }

        self.update_head_branch(branch_data);

        self.items_changed(0, 0, 1);

        // set focus on new item
        let item = self.item(0).unwrap();
        let item = item.downcast_ref::<BranchItem>().unwrap();
        item.set_initial_focus(true);

        // works via bind to single_selection selected
        self.set_selected_pos(0);
    }
}

pub fn header_factory() -> SignalListItemFactory {
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
            label.set_label(title);
        });
    });
    header_factory
}

pub fn item_factory() -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, list_item| {
        let image = Image::new();
        image.set_margin_top(4);
        // let spinner = Spinner::new();
        // spinner.set_visible(false);

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
        // bx.append(&spinner);
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
                // looks like it works only first time.
                // set_selected_pos from outside does not
                // trigger it
                trace!(
                    ".............item in connect selected {:?} {:?} {:?}",
                    branch_item.imp().branch.borrow().name,
                    branch_item.initial_focus(),
                    li.position()
                );
                if branch_item.initial_focus() {
                    debug!(".......................................");
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

        item.chain_property::<BranchItem>("is-head") // was "is_head"! it works also!
            .chain_closure::<String>(closure!(|_: Option<Object>, is_head: bool| {
                if is_head {
                    String::from("avatar-default-symbolic")
                } else {
                    String::from("")
                }
            }))
            .bind(&image, "icon-name", Widget::NONE);
        item.chain_property::<BranchItem>("title")
            .bind(&label_title, "label", Widget::NONE);

        item.chain_property::<BranchItem>("last-commit")
            .bind(&label_commit, "label", Widget::NONE);

        item.chain_property::<BranchItem>("dt")
            .bind(&label_dt, "label", Widget::NONE);
    });

    factory
}

pub fn listview_factory(
    repo_path: PathBuf,
    branches: Option<Vec<branch::BranchData>>,
    sender: Sender<crate::Event>,
    window: &Window,
) -> ListView {
    let header_factory = header_factory();
    let factory = item_factory();

    let branch_list = BranchList::new(sender.clone());

    let selection_model = SingleSelection::new(Some(branch_list));
    // why it was needed?
    // selection_model.set_autoselect(false);

    let model = selection_model.model().unwrap();
    let bind = selection_model.bind_property("selected", &model, "selected_pos");
    let _ = bind.bidirectional().build();

    let branch_list = model.downcast_ref::<BranchList>().unwrap();

    let mut classes = glib::collections::strv::StrV::new();
    classes.extend_from_slice(if StyleManager::default().is_dark() {
        &[DARK_CLASS]
    } else {
        &[LIGHT_CLASS]
    });
    let list_view = ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .header_factory(&header_factory)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .show_separators(true)
        .css_classes(classes)
        .build();

    list_view.connect_activate({
        let repo_path = repo_path.clone();
        let window = window.clone();
        move |lv: &ListView, _pos: u32| {
            let branch_list = get_branch_list(lv);
            branch_list.checkout(repo_path.clone(), &window, sender.clone());
        }
    });

    branch_list.get_branches(repo_path.clone(), branches, window);

    list_view.add_css_class("stage");
    list_view
}

pub fn headerbar_factory(
    repo_path: PathBuf,
    list_view: &ListView,
    window: &Window,
    sender: Sender<crate::Event>,
) -> HeaderBar {
    let hb = HeaderBar::builder().build();

    let entry = SearchEntry::builder()
        .search_delay(300)
        .width_chars(22)
        .placeholder_text("hit s for search")
        .build();
    entry.connect_stop_search(|e| {
        e.stop_signal_emission_by_name("stop-search");
    });
    let branch_list = get_branch_list(list_view);

    entry.connect_search_changed(clone!(@weak branch_list, @weak list_view => move |e| {
        let term = e.text();
        if !term.is_empty() && term.len() < 3 {
            return;
        }
        let selection_model = list_view.model().unwrap();

        let single_selection =
            selection_model.downcast_ref::<SingleSelection>().unwrap();

        single_selection.set_can_unselect(false);
        branch_list.search_new(term.into());
        single_selection.set_can_unselect(false);
    }));
    let search = SearchBar::builder()
        .tooltip_text("search branches")
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(false)
        .child(&entry)
        .build();

    let new_btn = Button::builder()
        .icon_name("list-add-symbolic")
        .can_shrink(true)
        .tooltip_text("Create branch (N)")
        .sensitive(true)
        .use_underline(true)
        .build();
    new_btn.connect_clicked({
        let sender = sender.clone();
        let window = window.clone();
        let branch_list = branch_list.clone();
        let repo_path = repo_path.clone();
        move |_| {
            branch_list.create_branch(repo_path.clone(), &window, sender.clone());
        }
    });
    let kill_btn = Button::builder()
        .icon_name("user-trash-symbolic")
        .use_underline(true)
        .tooltip_text("Delete branch (K)")
        .sensitive(false)
        .can_shrink(true)
        .build();
    kill_btn.connect_clicked({
        let sender = sender.clone();
        let window = window.clone();
        let branch_list = branch_list.clone();
        let repo_path = repo_path.clone();
        move |_| {
            branch_list.kill_branch(repo_path.clone(), &window, sender.clone());
        }
    });

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

    let merge_btn = Button::builder()
        .icon_name("media-playlist-shuffle")
        .use_underline(true)
        .tooltip_text("Merge branch (M)")
        .sensitive(false)
        .can_shrink(true)
        .build();

    let _ = branch_list
        .bind_property("selected-pos", &merge_btn, "sensitive")
        .transform_to(set_sensitive)
        .build();

    merge_btn.connect_clicked({
        let sender = sender.clone();
        let window = window.clone();
        let branch_list = branch_list.clone();
        let repo_path = repo_path.clone();
        move |_| branch_list.merge(repo_path.clone(), &window, sender.clone())
    });

    let rebase_btn = Button::builder()
        .icon_name("media-playlist-repeat-song-symbolic") // system-switch-user-symbolic
        .use_underline(true)
        .tooltip_text("Rebase onto branch (R)")
        .sensitive(false)
        .can_shrink(true)
        .build();
    let _ = branch_list
        .bind_property("selected-pos", &rebase_btn, "sensitive")
        .transform_to(set_sensitive)
        .build();

    rebase_btn.connect_clicked({
        let sender = sender.clone();
        let window = window.clone();
        let branch_list = branch_list.clone();
        let repo_path = repo_path.clone();
        move |_| branch_list.rebase(repo_path.clone(), &window, sender.clone())
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
        let window = window.clone();
        let branch_list = branch_list.clone();
        let repo_path = repo_path.clone();
        move |_| branch_list.update_remote(repo_path.clone(), &window, sender.clone())
    });

    let log_btn = Button::builder()
        .label("Log")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Log")
        .icon_name("org.gnome.Logs-symbolic")
        .can_shrink(true)
        .build();
    log_btn.connect_clicked(clone!(@weak list_view => move |_e| {
        let (_current_branch, selected_branch) =
            branches_in_use(&list_view);
        let oid = selected_branch.oid;
        sender.send_blocking(crate::Event::Log(Some(oid), Some(selected_branch.name.to_string())))
            .expect("cant send through channel");
        }
    ));

    let _ = branch_list
        .bind_property("selected-pos", &log_btn, "sensitive")
        .transform_to(set_sensitive)
        .build();

    hb.set_title_widget(Some(&search));
    hb.pack_end(&new_btn);
    hb.pack_end(&merge_btn);
    hb.pack_end(&rebase_btn);
    hb.pack_end(&kill_btn);
    hb.pack_end(&log_btn);
    hb.pack_end(&refresh_btn);
    hb.set_show_end_title_buttons(true);
    hb.set_show_back_button(true);
    hb
}

pub fn get_branch_list(list_view: &ListView) -> BranchList {
    let selection_model = list_view.model().unwrap();
    let single_selection = selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let branch_list = list_model.downcast_ref::<BranchList>().unwrap();
    branch_list.to_owned()
}

pub fn branches_in_use(list_view: &ListView) -> (branch::BranchData, branch::BranchData) {
    let selection_model = list_view.model().unwrap();
    let single_selection = selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let branch_list = list_model.downcast_ref::<BranchList>().unwrap();
    (
        branch_list
            .get_head_branch()
            .expect("cant get current branch"),
        branch_list.get_selected_branch(),
    )
}

pub fn show_branches_window(
    repo_path: PathBuf,
    branches: Option<Vec<branch::BranchData>>,
    app_window: &ApplicationWindow,
    sender: Sender<crate::Event>,
) -> Window {
    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let scroll = ScrolledWindow::new();

    let list_view = listview_factory(repo_path.clone(), branches, sender.clone(), &window);

    let hb = headerbar_factory(repo_path.clone(), &list_view, &window, sender.clone());

    let spinner = Spinner::builder()
        .hexpand(true)
        .vexpand(true)
        .vexpand_set(true)
        .hexpand_set(true)
        .halign(Align::Center)
        .valign(Align::Center)
        .margin_bottom(32)
        .height_request(128)
        .width_request(128)
        .build();

    let spinner_box = Box::builder()
        .hexpand(true)
        .vexpand(true)
        .vexpand_set(true)
        .hexpand_set(true)
        .halign(Align::Center)
        .valign(Align::Center)
        .orientation(Orientation::Vertical)
        .build();
    let spinner_label = Label::new(Some("Updating remote branches"));
    spinner_box.append(&spinner);
    spinner_box.append(&spinner_label);

    spinner.start();
    scroll.set_child(Some(&list_view));

    let spin = {
        let list_view = list_view.clone();
        let spinner = spinner.clone();
        let scroll = scroll.clone();
        let mut spinning = false;
        move || {
            spinning = !spinning;
            if spinning {
                spinner.start();
                scroll.set_child(Some(&spinner_box));
            } else {
                spinner.stop();
                scroll.set_child(Some(&list_view));
            }
        }
    };
    let branch_list = get_branch_list(&list_view);
    branch_list.set_spinner(SpinnerWrapper {
        spin: std::boxed::Box::new(spin),
    });

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        let list_view = list_view.clone();
        let repo_path = repo_path.clone();
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
                    let branch_list = get_branch_list(&list_view);
                    branch_list.create_branch(repo_path.clone(), &window, sender.clone());
                }
                (gdk::Key::k, _) => {
                    let branch_list = get_branch_list(&list_view);
                    branch_list.kill_branch(repo_path.clone(), &window, sender.clone());
                }
                (gdk::Key::m, _) => {
                    let branch_list = get_branch_list(&list_view);
                    branch_list.merge(repo_path.clone(), &window, sender.clone())
                }
                (gdk::Key::r, _) => {
                    let branch_list = get_branch_list(&list_view);
                    branch_list.rebase(repo_path.clone(), &window, sender.clone())
                }
                (gdk::Key::l, _) => {
                    let (_current_branch, selected_branch) = branches_in_use(&list_view);
                    let oid = selected_branch.oid;
                    sender
                        .send_blocking(crate::Event::Log(
                            Some(oid),
                            Some(selected_branch.name.to_string()),
                        ))
                        .expect("cant send through sender");
                }
                (gdk::Key::a, _) => {
                    let (_current_branch, selected_branch) = branches_in_use(&list_view);
                    let oid = selected_branch.oid;
                    sender
                        .send_blocking(crate::Event::CherryPick(oid, false, None, None))
                        .expect("cant send through sender");
                }
                (gdk::Key::u, _) => {
                    let branch_list = get_branch_list(&list_view);
                    branch_list.update_remote(repo_path.clone(), &window, sender.clone());
                }
                (gdk::Key::s, _) => {
                    let search_bar = hb.title_widget().unwrap();
                    let search_bar = search_bar.downcast_ref::<SearchBar>().unwrap();
                    let search_entry = search_bar.child().unwrap();
                    let search_entry = search_entry.downcast_ref::<SearchEntry>().unwrap();
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

    window
}
