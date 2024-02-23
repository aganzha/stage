use libadwaita::prelude::*;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use libadwaita::{
    ApplicationWindow, Window, HeaderBar, ToolbarView
};
use gtk4::{
    gio, glib, gdk,
    ScrolledWindow, EventControllerKey, ListView,
    ListItem, ListHeader, StringList, NoSelection, SignalListItemFactory,
    StringObject, Widget, Label, SectionModel, Box, Orientation, SelectionModel
};
use glib::{Object, clone};
use log::{debug, error, info, log_enabled, trace};
use git2::{BranchType};


glib::wrapper! {
    pub struct BranchItem(ObjectSubclass<branch_item::BranchItem>);
}


mod branch_item {
    use std::cell::RefCell;
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BranchItem)]
    pub struct BranchItem {
        
        pub branch: RefCell<crate::BranchData>,
        
        #[property(get, set)]
        pub ref_kind: RefCell<String>,

        #[property(get, set)]
        pub title: RefCell<String>,

        #[property(get, set)]
        pub last_commit: RefCell<String>,

        #[property(get, set)]
        pub dt: RefCell<String>
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
                BranchType::Remote => String::from("Remote")
            }
        };
        let ob = Object::builder::<BranchItem>()
            .property("ref-kind", ref_kind)
            .property("title", &branch.name)            
            .property("last-commit", &branch.commit_string)
            .property("dt", branch.commit_dt.to_string())
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
    use std::cell::RefCell;
    use gtk4::glib;
    use gtk4::gio;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    #[derive(Default)]
    pub struct BranchList {
        pub list             : RefCell<Vec<super::BranchItem>>,
        pub remote_start_pos : Option<u32>
    }


    #[glib::object_subclass]
    impl ObjectSubclass for BranchList {
        const NAME: &'static str = "StageBranchList";
        type Type = super::BranchList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel, gtk4::SectionModel);
    }

    impl ObjectImpl for BranchList {
    }

    impl ListModelImpl for BranchList {
        fn item_type(&self) -> glib::Type {
            super::BranchItem::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            // ??? clone ???
            Some(self.list.borrow()[position as usize].clone().into())
        }
    }

    impl SectionModelImpl for BranchList {
        fn section(&self, position: u32) -> (u32, u32) {
            debug!("------------> section for {:?} {:?}", position, self.list.borrow().len());
            match position {
                0..= 4 => {
                    (0, 5)
                }
                5..=9 => {
                    (5, 10)
                }
                10..=14 => {
                    (10, 15)
                }
                _ => {
                    (15, 21)
                }
            }
        }
    }
}

impl BranchList {

    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn make_list(&self, repo_path: std::ffi::OsString) {

        glib::spawn_future_local({
            clone!(@weak self as branch_list=> async move {
                let branches: Vec<crate::BranchData> = gio::spawn_blocking(||crate::get_refs(repo_path))
                    .await.expect("Task needs to finish successfully.");

                let items: Vec<BranchItem> = branches.into_iter()
                    .map(|branch| BranchItem::new(branch))
                    .collect();

                let le = items.len() as u32;
                let mut remote_start_pos: Option<u32> = None; 
                for item in items {
                    if remote_start_pos.is_none() && item.imp().branch.borrow().branch_type == BranchType::Remote {
                        
                    }
                    branch_list.imp().list.borrow_mut().push(item);
                }
                branch_list.items_changed(0, 0, le);
            })});
    }
}

pub fn show_branches_window(app_window: &ApplicationWindow, repo_path: std::ffi::OsString) {
    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    let hb = HeaderBar::builder()
        .build();

    let scroll = ScrolledWindow::new();

    let header_factory = SignalListItemFactory::new();
    header_factory.connect_setup(move |_, list_header| {
        let label = Label::new(Some("section"));
        let list_header = list_header
            .downcast_ref::<ListHeader>()
            .expect("Needs to be ListHeader");
        list_header.set_child(Some(&label));

        let item = list_header
            .property_expression("item");
        item.chain_property::<BranchItem>("ref-kind")
            .bind(&label, "label", Widget::NONE);

        debug!("---------------inside header factory {:?}", list_header);

    });

    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, list_item| {

        let label = Label::new(None);
        let label1 = Label::new(None);
        let label2 = Label::new(None);
        
        let bx = Box::builder()
            .orientation(Orientation::Horizontal)
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(2)
            .margin_end(2)
            .spacing(2)
            .build();
        bx.append(&label);
        bx.append(&label1);
        bx.append(&label2);
        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));

        let item = list_item
            .property_expression("item");
        item.chain_property::<BranchItem>("title")
            .bind(&label, "label", Widget::NONE);
        item.chain_property::<BranchItem>("last-commit")
            .bind(&label1, "label", Widget::NONE);
        item.chain_property::<BranchItem>("dt")
            .bind(&label2, "label", Widget::NONE);
    });

    let model = BranchList::new();

    model.make_list(repo_path);

    let selection_model = NoSelection::new(Some(model));

    let list_view = ListView::builder()
        .model(&selection_model)
        .factory(&factory)
        .header_factory(&header_factory)
        .build();
    scroll.set_child(Some(&list_view));

    let tb = ToolbarView::builder()
        .content(&scroll)
        .build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller =
        EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                _ => {
                }
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);
    window.present();
}
