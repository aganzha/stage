use libadwaita::prelude::*;
use gtk4::prelude::*;
use libadwaita::{
    ApplicationWindow, Window, HeaderBar, ToolbarView
};
use gtk4::{
    gio, glib, gdk,
    ScrolledWindow, EventControllerKey, ListView,
    ListItem, ListHeader, StringList, NoSelection, SignalListItemFactory,
    StringObject, Widget, Label, SectionModel, Box, Orientation
};
use glib::{Object};
use log::{debug, error, info, log_enabled, trace};

glib::wrapper! {
    pub struct BranchItem(ObjectSubclass<branch_item::BranchItem>);
}

impl BranchItem {
    pub fn new(branchtitle: String, lastcommit: String) -> Self {
        let result: BranchItem = Object::builder()
            .property("branchtitle", branchtitle)
            .property("lastcommit", lastcommit)
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
        pub branchtitle: RefCell<String>,
        
        #[property(get, set)]
        pub lastcommit: RefCell<String>
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BranchItem {
        const NAME: &'static str = "StageBranchItem";
        type Type = super::BranchItem;
    }
    #[glib::derived_properties]
    impl ObjectImpl for BranchItem {}
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
        item.chain_property::<BranchItem>("branchtitle")
            .bind(&label, "label", Widget::NONE);
        item.chain_property::<BranchItem>("lastcommit")
            .bind(&label1, "label", Widget::NONE);
        
    });
    let header_factory = SignalListItemFactory::new();
    header_factory.connect_setup(move |_, list_item| {        
        let label = Label::new(Some("section"));
        debug!("======================> {:?}", list_item);
        let list_item = list_item
            .downcast_ref::<ListHeader>()
            .expect("Needs to be ListHeader");
        list_item.set_child(Some(&label));
    });

    // let model: StringList = (0..=20).map(|number| number.to_string()).collect();
    let model = gio::ListStore::new::<BranchItem>();
    let fake_branches: Vec<BranchItem> = (0..=20).map(
        |number| {
            BranchItem::new(
                format!("title {}", number),
                format!("commit {}", number)
            )
        }
    ).collect();
    model.extend_from_slice(&fake_branches);
    let selection_model = NoSelection::new(Some(model));
    debug!("++++++++++++++++++++=> {:?}", selection_model.section(1));
    debug!("++++++++++++++++++++=> {:?}", selection_model.section(10));
    // let list_view = ListView::new(Some(selection_model), Some(factory));
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
