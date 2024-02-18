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
use glib::{Object};
use log::{debug, error, info, log_enabled, trace};


glib::wrapper! {
    pub struct BranchItem(ObjectSubclass<branch_item::BranchItem>);
}

impl BranchItem {
    pub fn new(ref_kind: String, branch_title: String, last_commit: String) -> Self {
        let result: BranchItem = Object::builder()
            .property("ref-kind", ref_kind)
            .property("branch-title", branch_title)
            .property("last-commit", last_commit)
            .build();
        return result;
    }
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

        #[property(get, set)]
        pub ref_kind: RefCell<String>,

        #[property(get, set)]
        pub branch_title: RefCell<String>,

        #[property(get, set)]
        pub last_commit: RefCell<String>
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BranchItem {
        const NAME: &'static str = "StageBranchItem";
        type Type = super::BranchItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for BranchItem {}

}

glib::wrapper! {
    pub struct BranchList(ObjectSubclass<branch_list::BranchList>)
        @implements gio::ListModel, SectionModel;
}

mod branch_list {
    use crate::debug;
    use std::cell::RefCell;
    use glib::Properties;
    use gtk4::glib;
    use gtk4::gio;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::BranchList)]
    pub struct BranchList {
        pub list: RefCell<Vec<super::BranchItem>>,

        #[property(get, set)]
        pub li: RefCell<String>
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BranchList {
        const NAME: &'static str = "StageBranchList";
        type Type = super::BranchList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel, gtk4::SectionModel);
    }

    #[glib::derived_properties]
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

    pub fn make_list(&mut self) {
        let fake_branches: Vec<BranchItem> = (0..=20).map(
            |number| {
                BranchItem::new(
                    match number {
                        0..=9 => "Branches".into(),
                        _ => "Remotes".into()
                    },
                    format!("title {}", number),
                    format!("commit {}", number)
                )
            }
        ).collect();
        for b in fake_branches {
            self.imp().list.borrow_mut().push(b);
        }
    }
}

pub fn show_branches_window(app_window: &ApplicationWindow) {
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
    header_factory.connect_setup(move |_, list_item| {
        let label = Label::new(Some("section"));
        let list_item = list_item
            .downcast_ref::<ListHeader>()
            .expect("Needs to be ListHeader");
        list_item.set_child(Some(&label));

        // this works
        let item = list_item
            .property_expression("item");
        item.chain_property::<BranchItem>("ref-kind")
            .bind(&label, "label", Widget::NONE);

        // this works
        // list_item.property_expression("start")
        //     .bind(&label, "label", Widget::NONE);

        // this does not work. because it is not expression!
        // it works once when list is constructed, and 
        // start property is not set
        // list_item.bind_property("start", &label, "label");
        debug!("---------------inside header factory {:?}", list_item);


    });


    let factory = SignalListItemFactory::new();
    factory.connect_setup(move |_, list_item| {

        let label = Label::new(None);
        let label1 = Label::new(None);

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
        let list_item = list_item
            .downcast_ref::<ListItem>()
            .expect("Needs to be ListItem");
        list_item.set_child(Some(&bx));

        let item = list_item
            .property_expression("item");
        item.chain_property::<BranchItem>("branch-title")
            .bind(&label, "label", Widget::NONE);
        item.chain_property::<BranchItem>("last-commit")
            .bind(&label1, "label", Widget::NONE);

    });


    let mut model = BranchList::new();

    model.make_list();


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
