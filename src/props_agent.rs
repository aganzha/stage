// use libadwaita::prelude::*;
// use libadwaita::{
//     HeaderBar
// };
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{glib};
use glib::{Object};

glib::wrapper! {
    pub struct PropsAgent(ObjectSubclass<stage_props_agent::PropsAgent>);
}

mod stage_props_agent {
    use gtk4::{gio, glib};
    use glib::Properties;
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use std::cell::RefCell;
    
    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::PropsAgent)]
    pub struct PropsAgent {
        #[property(get, set)]
        pub test_prop: RefCell<bool>,

    }
    
    #[glib::object_subclass]
    impl ObjectSubclass for PropsAgent {
        const NAME: &'static str = "PropsAgent";
        type Type = super::PropsAgent;

    }
    #[glib::derived_properties]
    impl ObjectImpl for PropsAgent {}

    // Trait shared by all widgets
    impl WidgetImpl for PropsAgent {}
}


impl PropsAgent {
   pub fn new() -> Self {
        Object::builder().build()
    }
}
