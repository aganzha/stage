use log::debug;
use std::cell::RefCell;
use std::sync::RwLock;
use std::sync::Mutex;
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ScrolledWindow,
    TextView, TextWindowType, Widget, Window as Gtk4Window, Box, Orientation, PopoverMenu,
    MenuButton, CheckButton, GestureClick, ToggleButton, Align
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window, StyleManager, ColorScheme};

use crate::gio::MenuModel;
use async_channel::Sender;

pub fn debug(app_window: &impl IsA<Gtk4Window>, mut stored_theme: String, sender: Sender<crate::Event>) {
    debug!("-------------------> {:?}", stored_theme);
    let window = Window::builder()
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();

    let bx = Box::builder()
        .orientation(Orientation::Vertical)
        .margin_top(2)
        .margin_bottom(2)
        .margin_start(2)
        .margin_end(2)
        .spacing(12)
        .build();
    let label = Label::builder().label("hey!").build();

    // let popover_menu = PopoverMenu::builder()
    //     .build();

    let menu_model = gio::Menu::new();
    let menu_item = gio::MenuItem::new(Some("theme_label"), Some("win.menu::1"));

    let theme_id = "theme".to_variant();


    menu_item.set_attribute_value("custom", Some(&theme_id));

    menu_model.insert_item(0, &menu_item);

    let menu_item = gio::MenuItem::new(Some("just_label"), Some("win.menu::2"));
    let label_id = "label".to_variant();

    menu_item.set_attribute_value("custom", Some(&label_id));
    // menu_item.set_attribute_value("label", Some(&label_id));
    menu_model.insert_item(1, &menu_item);


    let popover_menu = PopoverMenu::from_model(Some(&menu_model));

    let selector = Label::new(Some("selector"));
    // https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/struct.PopoverMenu.html#method.add_child
    popover_menu.add_child(&selector, "theme");

    let la = Label::builder().label("muyto").build();
    // https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/struct.PopoverMenu.html#method.add_child
    popover_menu.add_child(&la, "label");


    if stored_theme.is_empty() {
        stored_theme = "follow".to_string();
    }

    let burger = MenuButton::builder()
        .popover(&popover_menu)
        .icon_name("open-menu-symbolic")
        .build();

    let mine_selector = Box::builder()
        .orientation(Orientation::Horizontal)
        .css_name("mine_selector")
        .build();

    let mut first_toggle: Option<ToggleButton> = None;
    for id in ["follow", "light", "dark"] {
        let toggle = ToggleButton::builder()
            .active(false)
            .icon_name("")
            .name(id)
            .css_classes(vec![id])
            .margin_end(10)
            .build();
        if stored_theme == id {
            toggle.set_icon_name("object-select-symbolic");
            toggle.set_active(true);
        }
        toggle.last_child().unwrap().set_halign(Align::Center);
        toggle.bind_property("active", &toggle, "icon_name").transform_to({
            let sender = sender.clone();
            move |_, is_active: bool| {
                debug!("TTTTTTTTTTTTTTTTTTT {} {}", id, is_active);
                if is_active {
                    let theme = match id {
                        "follow" => ColorScheme::Default,
                        "light" => ColorScheme::ForceLight,
                        "dark" => ColorScheme::ForceDark,
                        n => todo!("whats the name? {:?}", n)
                    };
                    let manager = StyleManager::default();
                    manager.set_color_scheme(theme);
                    sender.send_blocking(
                        crate::Event::StoreSettings("theme".to_string(),
                                                    id.to_string())
                    ).expect("cant send through sender");
                    debug!("seeeeeeeeeeeeeeeeet icon {}", id);
                    Some("object-select-symbolic")
                        
                } else {
                    debug!("uuuuuuuuuuuuuuuuuuuunset icon {}", id);
                    Some("")
                }
            }}).build();
        mine_selector.append(&toggle);
        if let Some(ref ft) = first_toggle {
            toggle.set_group(Some(ft));
        } else {
            first_toggle.replace(toggle);
        }

    };


    // let follow = ToggleButton::builder()
    //     .active(true)
    //     .icon_name("")
    //     .name("follow")
    //     .css_classes(vec!["follow"])
    //     .margin_end(10)
    //     .build();
    // if stored_theme == "default" {
    //     follow.set_icon_name("object-select-symbolic");
    //     follow.set_active(true);
    // }


    // follow.last_child().unwrap().set_halign(Align::Center);

    // let light = ToggleButton::builder()
    //     .icon_name("")
    //     .name("light")
    //     .css_classes(vec!["light"])
    //     .margin_end(10)
    //     .group(&follow).build();
    // if stored_theme == "light" {
    //     light.set_icon_name("object-select-symbolic");
    //     light.set_active(true);
    // }

    // light.last_child().unwrap().set_halign(Align::Center);

    // let dark = ToggleButton::builder()
    //     .icon_name("")
    //     .name("dark")
    //     .css_classes(vec!["dark"])
    //     .group(&follow).build();
    // if stored_theme == "light" {
    //     light.set_icon_name("object-select-symbolic");
    //     light.set_active(true);
    // }
    // dark.last_child().unwrap().set_halign(Align::Center);

    // for w in [&follow, &light, &dark] {
    //     w.bind_property("active", w, "icon_name").transform_to({
    //         let sender = sender.clone();
    //         move |b: &glib::Binding, is_active: bool| {
    //             if is_active {
    //                 let tb = b.source().unwrap();
    //                 let tb = tb.downcast_ref::<Widget>().expect("cant get widget");
    //                 let chosen = tb.widget_name();
    //                 let theme = match chosen.as_str() {
    //                     "follow" => ColorScheme::Default,
    //                     "light" => ColorScheme::ForceLight,
    //                     "dark" => ColorScheme::ForceDark,
    //                     n => todo!("whats the name? {:?}", n)
    //                 };
    //                 let manager = StyleManager::default();
    //                 manager.set_color_scheme(theme);
    //                 sender.send_blocking(
    //                     crate::Event::StoreSettings("theme".to_string(),
    //                                                 chosen.to_string())
    //                 ).expect("cant send through sender");
    //                 Some("object-select-symbolic")
                        
    //             } else {
    //                 Some("")
    //             }
    //         }}).build();
    // }
    
    // mine_selector.append(&follow);
    // mine_selector.append(&light);
    // mine_selector.append(&dark);



    bx.append(&label);

    bx.append(&mine_selector);

    let hb = HeaderBar::builder().build();
    hb.pack_end(&burger);

    let tb = ToolbarView::builder().content(&bx).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));


    window.present();
}
