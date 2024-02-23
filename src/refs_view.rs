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
use std::thread;
use std::time::Duration;
use std::rc::Rc;


glib::wrapper! {
    pub struct RefItem(ObjectSubclass<ref_item::RefItem>);
}

impl RefItem {
    pub fn new(ref_kind: String, title: String, last_commit: String) -> Self {
        let result: RefItem = Object::builder()
            .property("ref-kind", ref_kind)
            .property("title", title)
            .property("last-commit", last_commit)
            .build();
        return result;
    }
}

mod ref_item {
    use std::cell::RefCell;
    use glib::Properties;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::RefItem)]
    pub struct RefItem {

        #[property(get, set)]
        pub ref_kind: RefCell<String>,

        #[property(get, set)]
        pub title: RefCell<String>,

        #[property(get, set)]
        pub last_commit: RefCell<String>
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RefItem {
        const NAME: &'static str = "StageRefItem";
        type Type = super::RefItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for RefItem {}

}

glib::wrapper! {
    pub struct RefList(ObjectSubclass<ref_list::RefList>)
        @implements gio::ListModel, SectionModel;
}

mod ref_list {
    use crate::debug;
    use std::cell::RefCell;
    use gtk4::glib;
    use gtk4::gio;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    #[derive(Default)]
    pub struct RefList {
        pub list: RefCell<Vec<super::RefItem>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RefList {
        const NAME: &'static str = "StageRefList";
        type Type = super::RefList;
        type ParentType = glib::Object;
        type Interfaces = (gio::ListModel, gtk4::SectionModel);
    }

    impl ObjectImpl for RefList {
    }

    impl ListModelImpl for RefList {
        fn item_type(&self) -> glib::Type {
            super::RefItem::static_type()
        }

        fn n_items(&self) -> u32 {
            debug!("calling reflist n_items................ {:?}", self.list.borrow().len() as u32);
            self.list.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            // ??? clone ???
            Some(self.list.borrow()[position as usize].clone().into())
        }
    }

    impl SectionModelImpl for RefList {
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

impl RefList {

    pub fn new() -> Self {
        Object::builder().build()
    }

    pub fn make_list(&self) {
        // let fake_branches: Vec<RefItem> = (0..=20).map(
        //     |number| {
        //         RefItem::new(
        //             match number {
        //                 0..=9 => "Refes".into(),
        //                 _ => "Remotes".into()
        //             },
        //             format!("title {}", number),
        //             format!("commit {}", number)
        //         )
        //     }
        // ).collect();

        glib::spawn_future_local({
            debug!("before spawn future {:?}", self);
            clone!(@weak self as ref_list=> async move {
                // Deactivate the button until the operation is done
                debug!("INSIDE future {:?}", ref_list);
                gio::spawn_blocking(move || {
                    let five_seconds = Duration::from_secs(5);
                    thread::sleep(five_seconds);
                    true
                })
                    .await
                    .expect("Task needs to finish successfully.");
                let fake_branches: Vec<RefItem> = (0..=20).map(
                    |number| {
                        RefItem::new(
                            match number {
                                0..=9 => "Refes".into(),
                                _ => "Remotes".into()
                            },
                            format!("title {}", number),
                            format!("commit {}", number)
                        )
                    }
                ).collect();
                debug!("fake branches! {:?}", fake_branches);
                let le = fake_branches.len() as u32;
                for b in fake_branches {
                    ref_list.imp().list.borrow_mut().push(b);
                }
                ref_list.items_changed(0, 0, le);
            })});
        
        // for b in fake_branches {
        //     self.imp().list.borrow_mut().push(b);
        // }
    }
}

pub fn show_refs_window(app_window: &ApplicationWindow) {
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
        item.chain_property::<RefItem>("ref-kind")
            .bind(&label, "label", Widget::NONE);

        debug!("---------------inside header factory {:?}", list_header);

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
        item.chain_property::<RefItem>("title")
            .bind(&label, "label", Widget::NONE);
        item.chain_property::<RefItem>("last-commit")
            .bind(&label1, "label", Widget::NONE);
    });

    let model = RefList::new();

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
