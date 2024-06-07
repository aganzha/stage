use log::debug;
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ScrolledWindow,
    TextView, TextWindowType, Widget, Window as Gtk4Window, Box, Orientation, PopoverMenu,
    MenuButton
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use libpanel::ThemeSelector;
use crate::gio::MenuModel;
    
pub fn debug(app_window: &impl IsA<Gtk4Window>,) {
    debug!("------------------->");
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
    
    let selector = ThemeSelector::new();
    // https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/struct.PopoverMenu.html#method.add_child
    popover_menu.add_child(&selector, "theme");

    let la = Label::builder().label("muyto").build();
    // https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/struct.PopoverMenu.html#method.add_child
    popover_menu.add_child(&la, "label");

    let burger = MenuButton::builder()
        .popover(&popover_menu)
        .icon_name("open-menu-symbolic")
        .build();

    bx.append(&label);

    let hb = HeaderBar::builder().build();
    hb.pack_end(&burger);
    
    let tb = ToolbarView::builder().content(&bx).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    window.present();
}
