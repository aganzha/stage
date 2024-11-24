// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use async_channel::Sender;
use glib::Object;
use libadwaita::prelude::*;
use libadwaita::{EntryRow, HeaderBar, StyleManager, SwitchRow, ToolbarView, Window};

use core::time::Duration;
use git2::Oid;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, Box, Button, EventControllerKey, GestureClick, Label, ListBox, ListItem,
    ListView, Orientation, PositionType, ScrolledWindow, SearchBar, SearchEntry, SelectionMode,
    SignalListItemFactory, SingleSelection, TextView, Widget, Window as Gtk4Window, WrapMode,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::dialogs::{alert, confirm_dialog_factory, DangerDialog, YES};
use crate::git::{remote, tag};
use crate::{DARK_CLASS, LIGHT_CLASS};
use log::trace;
use std::cell::Cell;

glib::wrapper! {
    pub struct TagItem(ObjectSubclass<tag_item::TagItem>);
}

mod tag_item {
    use crate::git::tag;
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::TagItem)]
    pub struct TagItem {
        pub tag: RefCell<tag::Tag>,

        #[property(get = Self::get_commit_oid)]
        pub commit_oid: String,

        #[property(get = Self::get_name)]
        pub name: String,

        #[property(get = Self::get_message)]
        pub message: String,

        #[property(get = Self::get_commit_message)]
        pub commit_message: String,

        #[property(get = Self::get_author)]
        pub author: String,

        #[property(get = Self::get_dt)]
        pub dt: String,

        #[property(get, set)]
        pub initial_focus: RefCell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TagItem {
        const NAME: &'static str = "StageTagItem";
        type Type = super::TagItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for TagItem {}

    impl TagItem {
        pub fn get_commit_oid(&self) -> String {
            format!(
                "<span color=\"#1C71D8\"> {}</span>",
                self.tag.borrow().commit.oid
            )
        }

        pub fn get_name(&self) -> String {
            self.tag.borrow().name.clone()
        }

        pub fn get_author(&self) -> String {
            self.tag.borrow().commit.author.to_string()
        }

        pub fn get_message(&self) -> String {
            self.tag.borrow().message.to_string()
        }

        pub fn get_commit_message(&self) -> String {
            self.tag.borrow().commit.message.to_string()
        }

        pub fn get_dt(&self) -> String {
            self.tag.borrow().commit.commit_dt.to_string()
        }
    }
}

impl TagItem {
    pub fn new(tag: tag::Tag) -> Self {
        let ob = Object::builder::<TagItem>().build();
        ob.imp().tag.replace(tag.clone());
        ob
    }
}

glib::wrapper! {
    pub struct TagList(ObjectSubclass<tag_list::TagList>)
        @implements gio::ListModel;
}

mod tag_list {

    use glib::Properties;
    use gtk4::gio;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::TagList)]
    pub struct TagList {
        pub list: RefCell<Vec<super::TagItem>>,
        pub original_list: RefCell<Vec<super::tag::Tag>>,
        pub search_term: RefCell<(String, usize)>,

        // does not used for now
        #[property(get, set)]
        pub selected_pos: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TagList {
        const NAME: &'static str = "StageTagList";
        type Type = super::TagList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for TagList {}

    impl ListModelImpl for TagList {
        fn item_type(&self) -> glib::Type {
            super::TagItem::static_type()
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
            // why clone???
            Some(list[position as usize].clone().into())
        }
    }
}

impl Default for TagList {
    fn default() -> Self {
        Self::new()
    }
}

impl TagList {
    pub fn new() -> Self {
        Object::builder().build()
    }
    pub fn get_tags_inside(
        &self,
        repo_path: PathBuf,
        mut start_oid: Option<Oid>,
        widget: &impl IsA<Widget>,
    ) {
        glib::spawn_future_local({
            let tag_list = self.clone();
            let repo_path = repo_path.clone();
            let widget = widget.clone();
            // let (ref term, term_count) = *self.imp().search_term.borrow();
            let (term, term_count) = self.imp().search_term.take();
            let search_term = {
                if term.is_empty() {
                    None
                } else {
                    // search pull tags 1 by 1. inc counter
                    // to stop that iteration when page size is reached
                    // self.imp().search_term.borrow_mut().1 = term_count + 1;
                    // Some(String::from(term))
                    self.imp()
                        .search_term
                        .replace((term.clone(), term_count + 1));
                    Some(term)
                }
            };
            async move {
                let list_le = tag_list.imp().list.borrow().len() as u32;
                let mut append_to_existing = false;
                if list_le > 0 {
                    let item = tag_list.item(list_le - 1).unwrap();
                    let tag_item = item.downcast_ref::<TagItem>().unwrap();
                    let oid = tag_item.imp().tag.borrow().oid;
                    start_oid.replace(oid);
                    append_to_existing = true;
                }

                let tags = gio::spawn_blocking({
                    let search_term = search_term.clone();
                    let repo_path = repo_path.clone();
                    move || tag::get_tag_list(repo_path, start_oid, search_term)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(Some(&widget));
                    Ok(Vec::new())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(Some(&widget));
                    Vec::new()
                });
                if tags.is_empty() {
                    return;
                }
                let mut added = 0;
                let mut last_added_oid: Option<Oid> = None;
                for item in tags
                    .into_iter()
                    .map(|tag| {
                        if search_term.is_none() {
                            tag_list.imp().original_list.borrow_mut().push(tag.clone());
                        }
                        tag
                    })
                    .map(TagItem::new)
                {
                    if append_to_existing {
                        if let Some(oid) = start_oid {
                            if item.imp().tag.borrow().oid == oid {
                                continue;
                            }
                        }
                    }
                    last_added_oid.replace(item.imp().tag.borrow().oid);
                    tag_list.imp().list.borrow_mut().push(item);
                    added += 1;
                }
                if added > 0 {
                    tag_list.items_changed(0, 0, added);
                }
                if search_term.is_some()
                    && last_added_oid.is_some()
                    && term_count < tag::TAG_PAGE_SIZE
                {
                    trace!(
                        "go next loop with start >>>>>>>>   oid {:?}",
                        last_added_oid
                    );
                    tag_list.get_tags_inside(repo_path, last_added_oid, &widget);
                }
            }
        });
    }

    pub fn reset_search(&self) {
        self.imp().search_term.take();
        let orig_le = self.imp().original_list.borrow().len();
        if orig_le == 0 {
            // this is hack for the first triggered event.
            // for some reason it is triggered without
            return;
        }
        let searched = self.imp().list.take();
        self.items_changed(0, searched.len() as u32, 0);
        self.imp().list.replace(
            self.imp()
                .original_list
                .borrow()
                .iter()
                .cloned()
                .map(TagItem::new)
                .collect(),
        );
        self.items_changed(0, 0, self.imp().list.borrow().len() as u32);
    }

    pub fn search(&self, term: String, repo_path: PathBuf, widget: &impl IsA<Widget>) {
        self.imp().search_term.replace((term, 0));
        let current_length = self.imp().list.borrow().len();
        self.imp().list.borrow_mut().clear();
        self.items_changed(0, current_length as u32, 0);
        self.get_tags_inside(repo_path, None, widget);
    }

    pub fn get_selected_oid(&self) -> Oid {
        let pos = self.selected_pos();
        let item = self.item(pos).unwrap();
        let tag_item = item.downcast_ref::<TagItem>().unwrap();
        let oid = tag_item.imp().tag.borrow().oid;
        oid
    }

    pub fn get_selected_commit_oid(&self) -> Oid {
        let pos = self.selected_pos();
        let item = self.item(pos).unwrap();
        let tag_item = item.downcast_ref::<TagItem>().unwrap();
        let oid = tag_item.imp().tag.borrow().commit.oid;
        oid
    }

    pub fn get_selected_tag(&self) -> (String, u32) {
        let pos = self.selected_pos();
        let item = self.item(pos).unwrap();
        let tag_item = item.downcast_ref::<TagItem>().unwrap();
        let name = tag_item.imp().tag.borrow().name.clone();
        (name, pos)
    }

    pub fn push_tag(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        let (tag_name, _) = self.get_selected_tag();
        let window = window.clone();
        glib::spawn_future_local({
            async move {
                gio::spawn_blocking({
                    let sender = sender.clone();
                    let tag_name = tag_name.clone();
                    move || remote::push(repo_path, tag_name, false, true, sender, None)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(Some(&window));
                    Ok(())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(Some(&window));
                });
                sender
                    .send_blocking(crate::Event::Toast(format!("Pushed tag {:?}", tag_name)))
                    .expect("cant send through sender");
            }
        });
    }

    pub fn kill_tag(&self, repo_path: PathBuf, window: &Window, sender: Sender<crate::Event>) {
        glib::spawn_future_local({
            let tags_list = self.clone();
            let window = window.clone();
            async move {
                let (tag_name, selected_pos) = tags_list.get_selected_tag();
                let tg_name = tag_name.clone();
                let result = gio::spawn_blocking(move || tag::kill_tag(repo_path, tg_name, sender))
                    .await
                    .unwrap_or_else(|e| {
                        alert(format!("{:?}", e)).present(Some(&window));
                        Ok(None)
                    })
                    .unwrap_or_else(|e| {
                        alert(e).present(Some(&window));
                        None
                    });
                if result.is_none() {
                    return;
                }
                tags_list
                    .imp()
                    .list
                    .borrow_mut()
                    .remove(selected_pos as usize);
                tags_list
                    .imp()
                    .original_list
                    .borrow_mut()
                    .retain(|tag| tag.name != tag_name);
                tags_list.items_changed(selected_pos, 1, 0);
                let mut pos = selected_pos;
                loop {
                    if let Some(item) = tags_list.item(pos) {
                        let item = item.downcast_ref::<TagItem>().unwrap();
                        item.set_initial_focus(true);
                        tags_list.set_selected_pos(0);
                        tags_list.set_selected_pos(pos);
                        break;
                    }
                    pos -= 1;
                    if pos <= 0 {
                        break;
                    }
                }
            }
        });
    }

    pub fn create_tag(
        &self,
        repo_path: PathBuf,
        target_oid: git2::Oid,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        glib::spawn_future_local({
            let tag_list = self.clone();
            let window = window.clone();
            async move {
                let lb = ListBox::builder()
                    .selection_mode(SelectionMode::None)
                    .css_classes(vec![String::from("boxed-list")])
                    .build();
                let input = EntryRow::builder()
                    .title("New tag name:")
                    .show_apply_button(false)
                    .css_classes(vec!["input_field"])
                    .build();
                let txt = TextView::builder()
                    .margin_start(12)
                    .margin_end(12)
                    .margin_top(12)
                    .margin_bottom(12)
                    .wrap_mode(WrapMode::Word)
                    .build();
                let scroll = ScrolledWindow::builder()
                    .vexpand(true)
                    .vexpand_set(true)
                    .hexpand(true)
                    .visible(false)
                    .hexpand_set(true)
                    .min_content_width(480)
                    .min_content_height(320)
                    .build();

                scroll.set_child(Some(&txt));

                let lightweight = SwitchRow::builder()
                    .title("Lightweight")
                    .css_classes(vec!["input_field"])
                    .active(true)
                    .build();
                lightweight.connect_active_notify({
                    let scroll = scroll.clone();
                    move |sw| {
                        scroll.set_visible(!sw.is_active());
                    }
                });
                lb.append(&input);
                lb.append(&scroll);
                let row = lb.last_child().unwrap();
                row.set_css_classes(&["hidden_row"]);
                row.set_focusable(false);
                lb.append(&lightweight);

                let dialog = confirm_dialog_factory(Some(&lb), "Create new tag", "Create");
                dialog.connect_realize({
                    let input = input.clone();
                    move |_| {
                        input.grab_focus();
                    }
                });

                let enter_pressed = Rc::new(Cell::new(false));
                input.connect_apply({
                    let dialog = dialog.clone();
                    let enter_pressed = enter_pressed.clone();
                    move |_entry| {
                        // someone pressed enter
                        enter_pressed.replace(true);
                        dialog.close();
                    }
                });
                input.connect_entry_activated({
                    let dialog = dialog.clone();
                    let enter_pressed = enter_pressed.clone();
                    move |_entry| {
                        // someone pressed enter
                        enter_pressed.replace(true);
                        dialog.close();
                    }
                });

                let response = dialog.choose_future(&window).await;
                if !("confirm" == response || enter_pressed.get()) {
                    return;
                }
                let new_tag_name = String::from(input.text());
                let buffer = txt.buffer();
                let start_iter = buffer.iter_at_offset(0);
                let eof_iter = buffer.end_iter();
                let tag_message = buffer
                    .text(&start_iter, &eof_iter, true)
                    .to_string()
                    .to_string();
                let lightweight = lightweight.is_active();
                let created_tag = gio::spawn_blocking(move || {
                    tag::create_tag(
                        repo_path,
                        new_tag_name,
                        target_oid,
                        tag_message,
                        lightweight,
                        sender,
                    )
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
                if let Some(created_tag) = created_tag {
                    tag_list.add_new_tag(created_tag);
                }
            }
        });
    }

    pub fn add_new_tag(&self, created_tag: tag::Tag) {
        self.imp()
            .original_list
            .borrow_mut()
            .insert(0, created_tag.clone());
        self.imp()
            .list
            .borrow_mut()
            .insert(0, TagItem::new(created_tag));
        self.items_changed(0, 0, 1);
        let item = self.item(0).unwrap();
        let item = item.downcast_ref::<TagItem>().unwrap();
        item.set_initial_focus(true);
        self.set_selected_pos(0);
    }

    pub fn reset_hard(
        &self,
        repo_path: PathBuf,
        window: &impl IsA<Widget>,
        sender: Sender<crate::Event>,
    ) {
        let oid = self.get_selected_commit_oid();
        glib::spawn_future_local({
            let window = window.clone();
            let sender = sender.clone();
            let commit_list = self.clone();
            async move {
                let response = alert(DangerDialog(
                    String::from("Reset"),
                    format!("Hard reset to {}", oid),
                ))
                .choose_future(&window)
                .await;
                if response != YES {
                    return;
                }
                let result = gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = repo_path.clone();
                    move || crate::reset_hard(path, Some(oid), sender)
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
                if result {
                    loop {
                        // let original = *commit_list.imp().original_list.borrow_mut();
                        let first_oid = commit_list.imp().original_list.borrow()[0].oid;
                        commit_list.imp().original_list.borrow_mut().remove(0);
                        if first_oid == oid {
                            break;
                        }
                    }
                    if commit_list.imp().search_term.borrow().0.is_empty() {
                        // remove from visual list only if it is not in search
                        let mut removed = 0;
                        loop {
                            // let original = *commit_list.imp().original_list.borrow_mut();
                            let first_oid = {
                                let first_item = &commit_list.imp().list.borrow()[0];
                                let first_oid = first_item.imp().tag.borrow().commit.oid;
                                first_oid
                            };
                            if first_oid == oid {
                                break;
                            }
                            commit_list.imp().list.borrow_mut().remove(0);
                            removed += 1;
                        }
                        if removed > 0 {
                            commit_list.items_changed(0, removed, 0);
                        }
                    }
                }
            }
        });
    }
}

pub fn item_factory(sender: Sender<crate::Event>) -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, list_item| {
        let oid_label = Label::builder()
            .label("")
            .use_markup(true)
            .width_chars(12)
            .max_width_chars(12)
            .xalign(0.0)
            .cursor(&gdk::Cursor::from_name("pointer", None).unwrap())
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        let gesture_controller = GestureClick::new();
        gesture_controller.connect_released({
            let list_item = list_item.clone();
            let sender = sender.clone();
            move |_gesture, _some, _wx, _wy| {
                let list_item = list_item.downcast_ref::<ListItem>().unwrap();
                let tag_item = list_item.item().unwrap();
                let tag_item = tag_item.downcast_ref::<TagItem>().unwrap();
                let oid = tag_item.imp().tag.borrow().commit.oid;
                sender
                    .send_blocking(crate::Event::ShowOid(oid, None))
                    .expect("cant send through sender");
            }
        });
        oid_label.add_controller(gesture_controller);

        let author_label = Label::builder()
            .label("")
            .width_chars(18)
            .max_width_chars(18)
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .build();

        let label_name = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(16)
            .max_width_chars(16)
            .ellipsize(pango::EllipsizeMode::End)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();

        let label_message = Label::builder()
            .label("")
            .use_markup(true)
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(16)
            .max_width_chars(16)
            .ellipsize(pango::EllipsizeMode::End)
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();

        let label_commit_message = Label::builder()
            .label("")
            .use_markup(true)
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(16)
            .max_width_chars(16)
            .ellipsize(pango::EllipsizeMode::End)
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
            .use_markup(true)
            .can_focus(true)
            .can_target(true)
            .build();

        let bx = Box::builder()
            .orientation(Orientation::Horizontal)
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(2)
            .margin_end(2)
            .spacing(12)
            .can_focus(true)
            .focusable(true)
            .build();

        bx.append(&oid_label);
        bx.append(&label_name);
        bx.append(&label_message);
        bx.append(&label_commit_message);
        bx.append(&author_label);
        bx.append(&label_dt);

        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));
        list_item.set_selectable(true);
        list_item.set_activatable(true);
        list_item.set_focusable(true);

        let item = list_item.property_expression("item");
        item.chain_property::<TagItem>("commit_oid")
            .bind(&oid_label, "label", Widget::NONE);

        item.chain_property::<TagItem>("author")
            .bind(&author_label, "label", Widget::NONE);
        item.chain_property::<TagItem>("name")
            .bind(&label_name, "label", Widget::NONE);
        item.chain_property::<TagItem>("message")
            .bind(&label_message, "label", Widget::NONE);
        item.chain_property::<TagItem>("commit_message").bind(
            &label_commit_message,
            "label",
            Widget::NONE,
        );
        item.chain_property::<TagItem>("dt")
            .bind(&label_dt, "label", Widget::NONE);
        list_item.connect_selected_notify(move |li: &ListItem| {
            if let Some(item) = li.item() {
                let tag_item = item.downcast_ref::<TagItem>().unwrap();
                if tag_item.initial_focus() {
                    li.child().unwrap().grab_focus();
                    tag_item.set_initial_focus(false)
                }
            }
        });
    });

    factory
}

pub fn headerbar_factory(
    list_view: &ListView,
    window: &Window,
    sender: Sender<crate::Event>,
    repo_path: PathBuf,
    target_oid: git2::Oid,
) -> HeaderBar {
    let entry = SearchEntry::builder()
        .search_delay(300)
        .width_chars(22)
        .placeholder_text("hit s for search")
        .build();
    entry.connect_stop_search(|e| {
        e.stop_signal_emission_by_name("stop-search");
    });
    let tag_list = get_tags_list(list_view);

    let search = SearchBar::builder()
        .tooltip_text("search tags")
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(false)
        .child(&entry)
        .build();
    let very_first_search = Rc::new(Cell::new(true));
    let threshold = Rc::new(RefCell::new(String::from("")));
    entry.connect_search_changed({
        let tag_list = tag_list.clone();
        let list_view = list_view.clone();
        let very_first_search = very_first_search.clone();
        let entry = entry.clone();
        let repo_path = repo_path.clone();

        move |e| {
            let term = e.text().to_lowercase();
            if !term.is_empty() && term.len() < 3 {
                return;
            }
            if term.is_empty() {
                let selection_model = list_view.model().unwrap();
                let single_selection = selection_model.downcast_ref::<SingleSelection>().unwrap();
                single_selection.set_can_unselect(true);
                if very_first_search.get() {
                    very_first_search.replace(false);
                } else {
                    tag_list.reset_search();
                    single_selection.set_can_unselect(false);
                }
            } else {
                threshold.replace(term);
                glib::source::timeout_add_local(Duration::from_millis(200), {
                    let entry = entry.clone();
                    let repo_path = repo_path.clone();
                    let threshold = threshold.clone();
                    let list_view = list_view.clone();
                    let tag_list = tag_list.clone();
                    move || {
                        let term = entry.text().to_lowercase();
                        if term == *threshold.borrow() {
                            tag_list.search(term, repo_path.clone(), &list_view);
                        }
                        glib::ControlFlow::Break
                    }
                });
            }
        }
    });
    let title = Label::builder()
        .margin_start(12)
        .use_markup(true)
        .label("Tags")
        .build();
    let hb = HeaderBar::builder().build();
    hb.set_title_widget(Some(&search));
    hb.pack_start(&title);

    let cherry_pick_btn = Button::builder()
        .icon_name("emblem-shared-symbolic")
        .can_shrink(true)
        .tooltip_text("Cherry-pick")
        .sensitive(true)
        .use_underline(true)
        .build();
    cherry_pick_btn.connect_clicked({
        let sender = sender.clone();
        let tag_list = tag_list.clone();
        move |_btn| {
            sender
                .send_blocking(crate::Event::CherryPick(
                    tag_list.get_selected_commit_oid(),
                    false,
                    None,
                    None,
                ))
                .expect("cant send through channel");
        }
    });

    let revert_btn = Button::builder()
        .icon_name("edit-undo-symbolic")
        .can_shrink(true)
        .tooltip_text("Revert")
        .sensitive(true)
        .use_underline(true)
        .build();
    revert_btn.connect_clicked({
        let sender = sender.clone();
        let tag_list = tag_list.clone();
        move |_btn| {
            sender
                .send_blocking(crate::Event::CherryPick(
                    tag_list.get_selected_commit_oid(),
                    true,
                    None,
                    None,
                ))
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
        let window = window.clone();
        let repo_path = repo_path.clone();
        let tag_list = tag_list.clone();
        move |_| {
            tag_list.reset_hard(repo_path.clone(), &window, sender.clone());
        }
    });

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
        let tag_list = tag_list.clone();
        let repo_path = repo_path.clone();
        move |_| {
            tag_list.create_tag(repo_path.clone(), target_oid, &window, sender.clone());
        }
    });
    let kill_btn = Button::builder()
        .icon_name("user-trash-symbolic")
        .use_underline(true)
        .tooltip_text("Delete tag (K)")
        .sensitive(true)
        .can_shrink(true)
        .build();
    kill_btn.connect_clicked({
        let sender = sender.clone();
        let window = window.clone();
        let tag_list = tag_list.clone();
        let repo_path = repo_path.clone();
        move |_| {
            tag_list.kill_tag(repo_path.clone(), &window, sender.clone());
        }
    });

    let push_btn = Button::builder()
        .label("Push")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Push")
        .icon_name("send-to-symbolic")
        .can_shrink(true)
        //.sensitive(false)
        .build();
    push_btn.connect_clicked({
        let sender = sender.clone();
        let window = window.clone();
        let tag_list = tag_list.clone();
        let repo_path = repo_path.clone();
        move |_| {
            tag_list.push_tag(repo_path.clone(), &window, sender.clone());
        }
    });

    hb.pack_end(&new_btn);
    hb.pack_end(&kill_btn);
    hb.pack_end(&reset_btn);
    hb.pack_end(&cherry_pick_btn);
    hb.pack_end(&revert_btn);
    hb.pack_end(&push_btn);
    hb
}

pub fn listview_factory(sender: Sender<crate::Event>) -> ListView {
    let tag_list = TagList::new();
    let selection_model = SingleSelection::new(Some(tag_list));

    // model IS tag_list actually
    let model = selection_model.model().unwrap();
    let bind = selection_model.bind_property("selected", &model, "selected_pos");
    let _ = bind.bidirectional().build();

    let factory = item_factory(sender.clone());
    let mut classes = glib::collections::strv::StrV::new();
    classes.extend_from_slice(if StyleManager::default().is_dark() {
        &[DARK_CLASS]
    } else {
        &[LIGHT_CLASS]
    });
    let list_view = ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .show_separators(true)
        .css_classes(classes)
        .build();
    list_view.connect_activate({
        let sender = sender.clone();
        move |lv: &ListView, _pos: u32| {
            let selection_model = lv.model().unwrap();
            let single_selection = selection_model.downcast_ref::<SingleSelection>().unwrap();
            let list_item = single_selection.selected_item().unwrap();
            let tag_item = list_item.downcast_ref::<TagItem>().unwrap();
            let oid = tag_item.imp().tag.borrow().commit.oid;
            sender
                .send_blocking(crate::Event::ShowOid(oid, None))
                .expect("cant send through sender");
        }
    });
    list_view
}

pub fn get_tags_list(list_view: &ListView) -> TagList {
    let selection_model = list_view.model().unwrap();
    let single_selection = selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let tag_list = list_model.downcast_ref::<TagList>().unwrap();
    tag_list.to_owned()
}

pub fn show_tags_window(
    repo_path: PathBuf,
    app_window: &impl IsA<Gtk4Window>,
    target_oid: git2::Oid,
    main_sender: Sender<crate::Event>,
) -> Window {
    let window = Window::builder()
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let list_view = listview_factory(main_sender.clone());

    let scroll = ScrolledWindow::new();

    // reached works with pagedown instead of overshot
    scroll.connect_edge_reached({
        let repo_path = repo_path.clone();
        move |scroll, position| {
            if position != PositionType::Bottom {
                return;
            }
            let list_view = scroll.child().unwrap();
            let list_view = list_view.downcast_ref::<ListView>().unwrap();
            let tags_list = get_tags_list(list_view);
            // when doing search, it need to reset search term count
            // because tags pull 1 by 1. it need to reset counter
            let (term, _) = tags_list.imp().search_term.take();
            tags_list.imp().search_term.replace((term, 0));
            tags_list.get_tags_inside(repo_path.clone(), None, list_view);
        }
    });
    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();

    let hb = headerbar_factory(
        &list_view,
        &window,
        main_sender.clone(),
        repo_path.clone(),
        target_oid,
    );

    tb.add_top_bar(&hb);
    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        let list_view = list_view.clone();
        let main_sender = main_sender.clone();
        let repo_path = repo_path.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::Escape, _) => {
                    window.close();
                }
                (gdk::Key::s, _) => {
                    let search_bar = hb.title_widget().unwrap();
                    let search_bar = search_bar.downcast_ref::<SearchBar>().unwrap();
                    let search_entry = search_bar.child().unwrap();
                    let search_entry = search_entry.downcast_ref::<SearchEntry>().unwrap();
                    trace!("enter search");
                    search_entry.grab_focus();
                }
                (gdk::Key::c | gdk::Key::n, _) => {
                    let tag_list = get_tags_list(&list_view);
                    tag_list.create_tag(
                        repo_path.clone(),
                        target_oid,
                        &window,
                        main_sender.clone(),
                    );
                }
                (gdk::Key::k | gdk::Key::d, _) => {
                    let tag_list = get_tags_list(&list_view);
                    tag_list.kill_tag(repo_path.clone(), &window, main_sender.clone());
                }
                (gdk::Key::p, _) => {
                    let tag_list = get_tags_list(&list_view);
                    tag_list.push_tag(repo_path.clone(), &window, main_sender.clone());
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
    get_tags_list(&list_view).get_tags_inside(repo_path.clone(), None, &list_view);
    window
}
