use async_channel::Sender;
use glib::{clone, closure, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, AlertDialog, Box, Button, EventControllerKey,
    Image, Label, ListBox, ListHeader, ListItem, ListScrollFlags, ListView,
    Orientation, ScrolledWindow, SearchBar, SearchEntry, SectionModel,
    SelectionMode, SignalListItemFactory, SingleSelection, Spinner, Widget,
    PositionType
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, EntryRow, HeaderBar, SwitchRow, ToolbarView, Window,
};
use git2::Oid;
use log::{debug, trace};

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

        #[property(get, set)]
        pub title: RefCell<String>,

        #[property(get, set)]
        pub dt: RefCell<String>,
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
            format!("{}", self.commit.borrow().oid)
        }
        pub fn get_author(&self) -> String {
            format!("{}", self.commit.borrow().author)
        }
    }
}

impl CommitItem {
    pub fn new(commit: crate::CommitDiff) -> Self {
        let ob = Object::builder::<CommitItem>()
            .property("title", &commit.commit_string)
            .property("dt", commit.commit_dt.to_string())
            .build();
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
                    let commit_item = item.downcast_ref::<CommitItem>().unwrap();
                    let oid = commit_item.imp().commit.borrow().oid;
                    start_oid.replace(oid);
                }                
                let commits = gio::spawn_blocking(move || {
                    crate::revwalk(repo_path, start_oid)
                }).await.expect("cant get commits");
                let commits_le = commits.len() as u32;
                for item in commits.into_iter().map(CommitItem::new) {
                    commit_list.imp().list.borrow_mut().push(item.clone());
                }
                commit_list.items_changed(if list_le > 0 {list_le} else {0}, 0, commits_le);
            }
        });
    }
}

pub fn make_item_factory() -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, list_item| {

        let oid_label = Label::new(Some(""));
        let author_label = Label::new(Some(""));
        
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
            Widget::NONE
        );
        item.chain_property::<CommitItem>("author").bind(
            &author_label,
            "label",
            Widget::NONE
        );  
        item.chain_property::<CommitItem>("title").bind(
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

pub fn make_list_view() -> ListView {
    let commit_list = CommitList::new();
    let selection_model = SingleSelection::new(Some(commit_list));
    let factory = make_item_factory();
    ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .show_separators(true)
        .build()
}

pub fn get_commit_list(list_view: &ListView) -> CommitList {
    let selection_model = list_view.model().unwrap();
    let single_selection =
        selection_model.downcast_ref::<SingleSelection>().unwrap();
    let list_model = single_selection.model().unwrap();
    let commit_list = list_model.downcast_ref::<CommitList>().unwrap();
    commit_list.to_owned()
}

pub fn show_log_window(
    repo_path: std::ffi::OsString,
    app_window: &ApplicationWindow,
    head: String,
    main_sender: Sender<crate::Event>,
) {
    // let (sender, receiver) = async_channel::unbounded();
    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let list_view = make_list_view();
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
            get_commit_list(&list_view).get_commits_inside(repo_path.clone());
        }});
    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();

    let title = Label::builder()
        .label(head)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    let hb = HeaderBar::builder().build();
    hb.set_title_widget(Some(&title));

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
    get_commit_list(&list_view).get_commits_inside(repo_path);

}
