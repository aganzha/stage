use async_channel::Sender;
use git2::Oid;
use glib::{clone, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, Box, EventControllerKey,
    GestureClick, Label, ListItem, ListView, Orientation, PositionType, ScrolledWindow,
    SearchBar, SearchEntry,
    SignalListItemFactory, SingleSelection, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, HeaderBar, ToolbarView, Window,
};
use log::{debug, info, trace};

glib::wrapper! {
    pub struct CommitItem(ObjectSubclass<commit_item::CommitItem>);
}

mod commit_item {
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::CommitItem)]
    pub struct CommitItem {
        pub commit: RefCell<crate::CommitDiff>,

        #[property(get = Self::get_author)]
        pub author: String,

        #[property(get = Self::get_oid)]
        pub oid: String,

        #[property(get = Self::get_message)]
        pub message: String,

        #[property(get = Self::get_dt)]
        pub dt: String,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CommitItem {
        const NAME: &'static str = "StageCommitItem";
        type Type = super::CommitItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for CommitItem {}

    impl CommitItem {
        pub fn get_oid(&self) -> String {
            format!(
                "<span color=\"#1C71D8\"> {}</span>",
                self.commit.borrow().oid
            )
        }
        pub fn get_author(&self) -> String {
            self.commit.borrow().author.to_string()
        }
        pub fn get_message(&self) -> String {
            let mut encoded = String::from("");
            html_escape::encode_safe_to_string(
                self.commit.borrow().message.trim(),
                &mut encoded,
            );
            encoded
        }
        pub fn get_dt(&self) -> String {
            format!("{}", self.commit.borrow().commit_dt)
        }
    }
}

impl CommitItem {
    pub fn new(commit: crate::CommitDiff) -> Self {
        let ob = Object::builder::<CommitItem>().build();
        ob.imp().commit.replace(commit);
        ob
    }
}

glib::wrapper! {
    pub struct CommitList(ObjectSubclass<commit_list::CommitList>)
        @implements gio::ListModel;
}

mod commit_list {

    use glib::Properties;
    use gtk4::gio;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::CommitList)]
    pub struct CommitList {
        pub list: RefCell<Vec<super::CommitItem>>,
        pub original_list: RefCell<Vec<super::CommitItem>>,

        #[property(get, set)]
        pub selected_pos: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CommitList {
        const NAME: &'static str = "StageCommitList";
        type Type = super::CommitList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel,);
    }

    #[glib::derived_properties]
    impl ObjectImpl for CommitList {}

    impl ListModelImpl for CommitList {
        fn item_type(&self) -> glib::Type {
            super::CommitItem::static_type()
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

impl Default for CommitList {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitList {
    pub fn new() -> Self {
        Object::builder().build()
    }
    pub fn get_commits_inside(&self, repo_path: std::ffi::OsString) {
        glib::spawn_future_local({
            let commit_list = self.clone();
            let repo_path = repo_path.clone();
            async move {
                let list_le = commit_list.imp().list.borrow().len() as u32;
                let mut start_oid: Option<Oid> = None;
                if list_le > 0 {
                    let item = commit_list.item(list_le - 1).unwrap();
                    let commit_item =
                        item.downcast_ref::<CommitItem>().unwrap();
                    let oid = commit_item.imp().commit.borrow().oid;
                    start_oid.replace(oid);
                }

                let commits = gio::spawn_blocking(move || {
                    crate::revwalk(repo_path, start_oid, None)
                })
                .await
                .expect("cant get commits");
                let mut added = 0;
                for item in commits.into_iter().map(CommitItem::new) {
                    if let Some(oid) = start_oid {
                        if item.imp().commit.borrow().oid == oid {
                            break;
                        }
                    }
                    commit_list.imp().list.borrow_mut().push(item.clone());
                    added += 1;
                }
                if added > 0 {
                    commit_list.items_changed(
                        if list_le > 0 { list_le } else { 0 },
                        0,
                        added,
                    );
                }
            }
        });
    }

    pub fn reset_search(&self) {
        let orig_le = self.imp().original_list.borrow().len();
        if orig_le == 0 {
            // this is hack for the first triggered event.
            // for some reason it is triggered without
            return;
        }
        let searched = self.imp().list.take();
        self.items_changed(0, searched.len() as u32, 0);
        self.imp().list.replace(self.imp().original_list.take());
        self.items_changed(0, 0, self.imp().list.borrow().len() as u32);
    }

    pub fn search(&self, term: String, repo_path: std::ffi::OsString) {
        glib::spawn_future_local({
            let commit_list = self.clone();
            let repo_path = repo_path.clone();
            async move {
                let commits = gio::spawn_blocking(move || {
                    crate::revwalk(repo_path, None, Some(term))
                })
                .await
                .expect("cant get commits");
                let orig_le = commit_list.imp().list.borrow().len();
                commit_list
                    .imp()
                    .original_list
                    .replace(commit_list.imp().list.take());
                commit_list.items_changed(0, orig_le as u32, 0);
                let mut added = 0;
                for item in commits.into_iter().map(CommitItem::new) {
                    commit_list.imp().list.borrow_mut().push(item.clone());
                    added += 1;
                }
                if added > 0 {
                    commit_list.items_changed(0, 0, added);
                }
            }
        });
    }
}

pub fn item_factory(sender: Sender<Event>) -> SignalListItemFactory {
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
                let commit_item = list_item.item().unwrap();
                let commit_item =
                    commit_item.downcast_ref::<CommitItem>().unwrap();
                let oid = commit_item.imp().commit.borrow().oid;
                sender
                    .send_blocking(Event::ShowOid(oid))
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

        let label_commit = Label::builder()
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
        bx.append(&author_label);
        bx.append(&label_commit);
        bx.append(&label_dt);

        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));
        list_item.set_selectable(true);
        list_item.set_activatable(true);
        list_item.set_focusable(true);

        let item = list_item.property_expression("item");
        item.chain_property::<CommitItem>("oid").bind(
            &oid_label,
            "label",
            Widget::NONE,
        );
        item.chain_property::<CommitItem>("author").bind(
            &author_label,
            "label",
            Widget::NONE,
        );
        item.chain_property::<CommitItem>("message").bind(
            &label_commit,
            "label",
            Widget::NONE,
        );

        item.chain_property::<CommitItem>("dt").bind(
            &label_dt,
            "label",
            Widget::NONE,
        );
    });

    factory
}

pub fn listview_factory(sender: Sender<Event>) -> ListView {
    let commit_list = CommitList::new();
    let selection_model = SingleSelection::new(Some(commit_list));

    // model IS commit_list actually
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
            let commit_item = list_item.downcast_ref::<CommitItem>().unwrap();
            let oid = commit_item.imp().commit.borrow().oid;
            sender
                .send_blocking(Event::ShowOid(oid))
                .expect("cant send through sender");
        }
    });
    list_view
}

pub fn get_commit_list(list_view: &ListView) -> CommitList {
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let commit_list = list_model.downcast_ref::<CommitList>().unwrap();
    commit_list.to_owned()
}

pub enum Event {
    ShowOid(Oid),
}

pub fn headerbar_factory(
    list_view: &ListView,
    repo_path: std::ffi::OsString,
) -> HeaderBar {
    let entry = SearchEntry::builder()
        .search_delay(300)
        .width_chars(22)
        .placeholder_text("hit s for search")
        .build();
    entry.connect_stop_search(|e| {
        e.stop_signal_emission_by_name("stop-search");
    });
    let commit_list = get_commit_list(list_view);

    let search = SearchBar::builder()
        .tooltip_text("search commits")
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(false)
        .child(&entry)
        .build();
    entry.connect_search_changed(
        clone!(@weak commit_list, @weak list_view => move |e| {
            let term = e.text().to_lowercase();
            if !term.is_empty() && term.len() < 3 {
                return;
            }
            if term.is_empty() {
                let selection_model = list_view.model().unwrap();
                let single_selection =
                    selection_model.downcast_ref::<SingleSelection>().unwrap();
                single_selection.set_can_unselect(true);
                commit_list.reset_search();
                single_selection.set_can_unselect(false);
            } else {
                commit_list.search(term, repo_path.clone());
            }
        }),
    );
    let hb = HeaderBar::builder().build();
    hb.set_title_widget(Some(&search));
    hb
}

pub fn show_log_window(
    repo_path: std::ffi::OsString,
    app_window: &ApplicationWindow,
    _head: String,
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

    let list_view = listview_factory(sender.clone());

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
            get_commit_list(list_view).get_commits_inside(repo_path.clone());
        }
    });
    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();

    let hb = headerbar_factory(&list_view, repo_path.clone());

    tb.add_top_bar(&hb);
    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        // let sender = sender.clone();
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
    get_commit_list(&list_view).get_commits_inside(repo_path.clone());
    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            match event {
                Event::ShowOid(oid) => {
                    info!("show oid {:?}", oid);
                    crate::show_commit_window(
                        repo_path.clone(),
                        oid,
                        &window,
                        main_sender.clone(),
                    );
                }
            }
        }
    });
}
