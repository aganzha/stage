// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use async_channel::Sender;
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use glib::{Object, clone};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, Box, Button, EventControllerKey, GestureClick,
    Image, Label, ListItem, ListView, Orientation, PositionType,
    ScrolledWindow, SearchBar, SearchEntry, SignalListItemFactory,
    SingleSelection, Widget, Window as Gtk4Window,
};
use std::path::PathBuf;
use std::rc::Rc;
use std::cell::RefCell;
use git2::Oid;
use core::time::Duration;

use log::{trace, debug};

use crate::git::tag;
use crate::dialogs::{alert};

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

        // #[property(get = Self::get_author)]
        // pub author: String,

        #[property(get = Self::get_oid)]
        pub oid: String,

        // #[property(get = Self::get_from)]
        // pub from: String,

        // #[property(get = Self::get_from_tooltip)]
        // pub from_tooltip: String,

        #[property(get = Self::get_name)]
        pub name: String,

        // #[property(get = Self::get_dt)]
        // pub dt: String,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TagItem {
        const NAME: &'static str = "StageTagItem";
        type Type = super::TagItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for TagItem {}

    impl TagItem {
        pub fn get_oid(&self) -> String {
            format!(
                "<span color=\"#1C71D8\"> {}</span>",
                self.tag.borrow().oid
            )
        }

        // pub fn get_author(&self) -> String {
        //     self.tag.borrow().author.to_string()
        // }

        pub fn get_name(&self) -> String {
            self.tag.borrow().name.to_string()
        }
        // pub fn get_dt(&self) -> String {
        //     self.tag.borrow().tag_dt.to_string()
        // }
    }
}

impl TagItem {
    pub fn new(tag: tag::Tag) -> Self {
        let ob = Object::builder::<TagItem>().build();
        ob.imp().tag.replace(tag);
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
                    let commit_item =
                        item.downcast_ref::<TagItem>().unwrap();
                    let oid = commit_item.imp().tag.borrow().oid;
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
                    alert(format!("{:?}", e)).present(&widget);
                    Ok(Vec::new())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&widget);
                    Vec::new()
                });
                // trace!("tags in response {:?}", tags.len());
                if tags.is_empty() {
                    return;
                }
                let mut added = 0;
                let mut last_added_oid: Option<Oid> = None;
                for item in tags
                    .into_iter()
                    .map(|tag| {
                        if search_term.is_none() {
                            tag_list
                                .imp()
                                .original_list
                                .borrow_mut()
                                .push(tag.clone());
                        }
                        tag
                    })
                    .map(TagItem::new)
                {
                    if append_to_existing {
                        if let Some(oid) = start_oid {
                            if item.imp().tag.borrow().oid == oid {
                                // trace!("skip previously found commit {:?}", oid);
                                continue;
                            }
                        }
                    }
                    last_added_oid.replace(item.imp().tag.borrow().oid);
                    tag_list.imp().list.borrow_mut().push(item);                    
                    added += 1;
                }
                debug!("added some tags {:?}", added);
                if added > 0 {
                    tag_list.items_changed(
                        0,
                        0,
                        added,
                    );
                }
                if search_term.is_some()
                    && last_added_oid.is_some()
                    && term_count < tag::TAG_PAGE_SIZE
                {
                    trace!(
                        "go next loop with start >>>>>>>>   oid {:?}",
                        last_added_oid
                    );
                    tag_list.get_tags_inside(
                        repo_path,
                        last_added_oid,
                        &widget,
                    );
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

    pub fn search(
        &self,
        term: String,
        repo_path: PathBuf,
        widget: &impl IsA<Widget>,
    ) {
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
}

pub fn item_factory(sender: Sender<crate::Event>) -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    let focus = Rc::new(RefCell::new(false));
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
                let tag_item =
                    tag_item.downcast_ref::<TagItem>().unwrap();
                let oid = tag_item.imp().tag.borrow().oid;
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

        let label_tag = Label::builder()
            .label("")
            .lines(1)
            .single_line_mode(true)
            .xalign(0.0)
            .width_chars(36)
            .max_width_chars(36)
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
            // .css_classes(vec![String::from("branch_row")])
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(2)
            .margin_end(2)
            .spacing(12)
            .can_focus(true)
            .focusable(true)
            .build();

        bx.append(&oid_label);
        // bx.append(&author_label);
        bx.append(&label_tag);
        // bx.append(&label_dt);

        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));
        list_item.set_selectable(true);
        list_item.set_activatable(true);
        list_item.set_focusable(true);

        let item = list_item.property_expression("item");
        item.chain_property::<TagItem>("oid").bind(
            &oid_label,
            "label",
            Widget::NONE,
        );

        // item.chain_property::<TagItem>("author").bind(
        //     &author_label,
        //     "label",
        //     Widget::NONE,
        // );
        item.chain_property::<TagItem>("name").bind(
            &label_tag,
            "label",
            Widget::NONE,
        );

        // item.chain_property::<TagItem>("dt").bind(
        //     &label_dt,
        //     "label",
        //     Widget::NONE,
        // );
        let focus = focus.clone();
        list_item.connect_selected_notify(move |li: &ListItem| {
            glib::source::timeout_add_local(Duration::from_millis(300), {
                let focus = focus.clone();
                let li = li.clone();
                move || {
                    if !*focus.borrow() {
                        let first_child = li.child().unwrap();
                        let first_child =
                            first_child.downcast_ref::<Widget>().unwrap();
                        let row = first_child.parent().unwrap();
                        row.grab_focus();
                        *focus.borrow_mut() = true;
                    }
                    glib::ControlFlow::Break
                }
            });
        });
    });

    factory
}

pub fn headerbar_factory(
    list_view: &ListView,
    window: &impl IsA<Widget>,
    sender: Sender<crate::Event>,
    repo_path: PathBuf,
) -> HeaderBar {
    let entry = SearchEntry::builder()
        .search_delay(300)
        .width_chars(22)
        .placeholder_text("hit s for search")
        .build();
    entry.connect_stop_search(|e| {
        e.stop_signal_emission_by_name("stop-search");
    });
    let commit_list = get_tags_list(list_view);

    let search = SearchBar::builder()
        .tooltip_text("search commits")
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(false)
        .child(&entry)
        .build();
    let very_first_search = Rc::new(RefCell::new(true));
    let threshold = Rc::new(RefCell::new(String::from("")));
    entry.connect_search_changed(
        clone!(@weak commit_list, @weak list_view, @strong very_first_search, @weak entry, @strong repo_path => move |e| {
            let term = e.text().to_lowercase();
            if !term.is_empty() && term.len() < 3 {
                return;
            }
            if term.is_empty() {
                let selection_model = list_view.model().unwrap();
                let single_selection =
                    selection_model.downcast_ref::<SingleSelection>().unwrap();
                single_selection.set_can_unselect(true);
                if *very_first_search.borrow() {
                    very_first_search.replace(false);
                } else {
                    commit_list.reset_search();
                    single_selection.set_can_unselect(false);
                }
            } else {
                threshold.replace(term);
                glib::source::timeout_add_local(Duration::from_millis(200), {
                    let entry = entry.clone();
                    let repo_path = repo_path.clone();
                    let threshold = threshold.clone();
                    move || {
                        let term = entry.text().to_lowercase();
                        if term == *threshold.borrow() {
                            commit_list.search(term, repo_path.clone(), &list_view);
                        }
                        glib::ControlFlow::Break
                    }
                });
            }
        }),
    );
    let title = Label::builder()
        .margin_start(12)
        .use_markup(true)
        .label("Tags")
        .build();
    let hb = HeaderBar::builder().build();
    hb.set_title_widget(Some(&search));
    hb.pack_start(&title);
    hb
}

pub fn listview_factory(sender: Sender<crate::Event>) -> ListView {
    let tag_list = TagList::new();
    let selection_model = SingleSelection::new(Some(tag_list));

    // model IS tag_list actually
    let model = selection_model.model().unwrap();
    let bind =
        selection_model.bind_property("selected", &model, "selected_pos");
    let _ = bind.bidirectional().build();

    let factory = item_factory(sender.clone());
    let list_view = ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .show_separators(true)
        .build();
    list_view.connect_activate({
        let sender = sender.clone();
        move |lv: &ListView, _pos: u32| {
            let selection_model = lv.model().unwrap();
            let single_selection =
                selection_model.downcast_ref::<SingleSelection>().unwrap();
            let list_item = single_selection.selected_item().unwrap();
            let tag_item = list_item.downcast_ref::<TagItem>().unwrap();
            let oid = tag_item.imp().tag.borrow().oid;
            sender
                .send_blocking(crate::Event::ShowOid(oid, None))
                .expect("cant send through sender");
        }
    });
    list_view
}

pub fn get_tags_list(list_view: &ListView) -> TagList {
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let tag_list = list_model.downcast_ref::<TagList>().unwrap();
    tag_list.to_owned()
}

pub fn show_tags_window(
    repo_path: PathBuf,
    app_window: &impl IsA<Gtk4Window>,
    main_sender: Sender<crate::Event>,
) -> Window {
    // let (sender, receiver) = async_channel::unbounded();

    let window = Window::builder()
        //.application(&app_window.application().unwrap())// panic!
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
                    let search_bar =
                        search_bar.downcast_ref::<SearchBar>().unwrap();
                    let search_entry = search_bar.child().unwrap();
                    let search_entry =
                        search_entry.downcast_ref::<SearchEntry>().unwrap();
                    trace!("enter search");
                    search_entry.grab_focus();
                }
                (gdk::Key::c | gdk::Key::n, _) => {
                    debug!("create new tag");
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
    get_tags_list(&list_view).get_tags_inside(
        repo_path.clone(),
        None,
        &list_view,
    );
    window
}
