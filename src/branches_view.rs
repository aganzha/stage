use async_channel::Sender;
use git2::BranchType;
use glib::{clone, closure, types, Object};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, AlertDialog, Box, CheckButton, EventControllerKey,
    Label, ListHeader, ListItem, ListView, NoSelection, Orientation,
    PropertyExpression, ScrolledWindow, SectionModel, SelectionModel,
    SignalListItemFactory, SingleSelection, Spinner, StringList, StringObject,
    Widget,
};
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, HeaderBar, ToolbarView, Window};
use log::{debug, error, info, log_enabled, trace};
use std::thread;
use std::time::Duration;

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
        @implements gio::ListModel, SectionModel;
}

mod branch_list {
    use crate::debug;
    use glib::Properties;
    use gtk4::gio;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BranchList)]
    pub struct BranchList {
        pub list: RefCell<Vec<super::BranchItem>>,
        pub remote_start_pos: RefCell<Option<u32>>,

        #[property(get, set)]
        pub selected: RefCell<u32>,
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
            // ??? clone ???
            return Some(list[position as usize].clone().into());
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
    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn make_list(&self, repo_path: std::ffi::OsString) {
        glib::spawn_future_local({
            clone!(@weak self as branch_list => async move {
                let branches: Vec<crate::BranchData> = gio::spawn_blocking(move || {
                    crate::get_refs(repo_path)
                }).await.expect("Task needs to finish successfully.");

                let items: Vec<BranchItem> = branches.into_iter()
                    .map(|branch| BranchItem::new(branch))
                    .collect();

                let le = items.len() as u32;
                let mut pos = 0;
                let mut remote_start_pos: Option<u32> = None;
                let mut selected = 0;
                for item in items {
                    if remote_start_pos.is_none() && item.imp().branch.borrow().branch_type == BranchType::Remote {
                        remote_start_pos.replace(pos as u32);
                    }
                    if item.imp().branch.borrow().is_head {
                        selected = pos;
                        item.set_initial_focus(true)
                    }
                    branch_list.imp().list.borrow_mut().push(item);
                    pos += 1;
                }
                branch_list.imp().remote_start_pos.replace(remote_start_pos);
                branch_list.items_changed(0, 0, le);
                // works via bind to single_selection selected
                branch_list.set_selected(selected);
            })
        });
    }

    pub fn checkout(
        &self,
        repo_path: std::ffi::OsString,
        selected_item: &BranchItem,
        current_item: &BranchItem,
        window: &Window,
        sender: Sender<crate::Event>,
    ) {
        let branch_data = selected_item.imp().branch.borrow();
        let name = branch_data.refname.clone();
        let oid = branch_data.oid.clone();
        glib::spawn_future_local({
            clone!(@weak self as branch_list, @weak window as window, @weak selected_item, @weak current_item => async move {
                let result = gio::spawn_blocking(move || {
                    crate::checkout(repo_path, oid, &name, sender)
                }).await;
                let mut err_message = String::from("git error");
                if let Ok(git_result) = result {
                    selected_item.set_progress(false);
                    match git_result {
                        Ok(_) => {
                            selected_item.set_is_head(true);
                            selected_item.set_no_progress(true);
                            current_item.set_is_head(false);
                            return;
                        }
                        Err(err) => err_message = err
                    }
                }
                selected_item.set_no_progress(true);
                crate::display_error(&window, &err_message);
                debug!("result in set head {:?}", err_message);
            })
        });
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
        let fake_btn = CheckButton::new();
        let btn = CheckButton::new();
        btn.set_group(Some(&fake_btn));
        btn.set_sensitive(false);
        let spinner = Spinner::new();
        spinner.set_visible(false);
        // spinner.set_spinning(true);
        let label_title = Label::builder()
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
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(2)
            .margin_end(2)
            .spacing(12)
            .can_focus(true)
            .focusable(true)
            .build();
        bx.append(&btn);
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
        list_item.connect_selected_notify(|li: &ListItem| {
            // grab foxus only once on list init
            let ob = li.item().unwrap();
            let branch_item = ob.downcast_ref::<BranchItem>().unwrap();
            if branch_item.initial_focus() {
                li.child().unwrap().grab_focus();
                branch_item.set_initial_focus(false)
            }
        });
        
        let item = list_item.property_expression("item");

        item.chain_property::<BranchItem>("is_head").bind(
            &btn,
            "active",
            Widget::NONE,
        );
        item.chain_property::<BranchItem>("no-progress").bind(
            &btn,
            "visible",
            Widget::NONE,
        );
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
    sender: Sender<crate::Event>
) -> ListView {
    let header_factory = make_header_factory();
    let factory = make_item_factory();

    let branch_list = BranchList::new();

    let selection_model = SingleSelection::new(Some(branch_list));
    let model = selection_model.model().unwrap();
    let bind = selection_model.bind_property("selected", &model, "selected");
    let _ = bind.bidirectional().build();

    let branch_list = model.downcast_ref::<BranchList>().unwrap();
    branch_list.make_list(repo_path.clone());
    
    selection_model.set_autoselect(false);

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
            for branch_item in list.iter() {
                if branch_item.is_head() {
                    current_item.replace(branch_item);
                }
                branch_item.set_progress(false);
                branch_item.set_no_progress(true);
            }
            let branch_item = item.downcast_ref::<BranchItem>().unwrap();
            branch_item.set_progress(true);
            branch_item.set_no_progress(false);
            let root = lv.root().unwrap();
            let window = root.downcast_ref::<Window>().unwrap();
            debug!(
                "cheeeeeeckout! {:?} {:?}",
                single_selection.selected(),
                branch_list.selected()
            );
            branch_list.checkout(
                repo_path.clone(),
                &branch_item,
                current_item.unwrap(),
                &window,
                sender.clone(),
            );
        }
    });
    list_view.add_css_class("stage");
    list_view
}

pub fn show_branches_window(
    repo_path: std::ffi::OsString,
    app_window: &ApplicationWindow,
    sender: Sender<crate::Event>,
) {
    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let hb = HeaderBar::builder().build();

    let scroll = ScrolledWindow::new();

    let list_view = make_list_view(repo_path, sender);
    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);
    window.present();
    list_view.grab_focus();
}
