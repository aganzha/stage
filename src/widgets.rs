use libadwaita::prelude::*;
use libadwaita::{HeaderBar, MessageDialog, ResponseAppearance, Window, SplitButton, ButtonContent};
use std::ffi::OsString;
// use glib::Sender;
// use std::sync::mpsc::Sender;
use async_channel::Sender;
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::{
    glib, gio, AlertDialog, Button, EventControllerKey, TextView, Widget,
    Window as Gtk4Window, Label, ArrowType, Popover, Box, Orientation, Align
};
use log::{debug, info, trace};

pub fn display_error(
    w: &impl IsA<Gtk4Window>, // Application
    message: &str,
) {
    let d = AlertDialog::builder().message(message).build();
    d.show(Some(w));
}

pub fn make_confirm_dialog(
    window: &impl IsA<Gtk4Window>,
    child: Option<&impl IsA<Widget>>,
    heading: &str,
    confirm_title: &str,
) -> MessageDialog {
    let cancel_response = "cancel";
    let confirm_response = "confirm";

    let dialog = MessageDialog::builder()
        .heading(heading)
        .transient_for(window)
        .modal(true)
        .destroy_with_parent(true)
        .close_response(cancel_response)
        .default_response(confirm_response)
        .default_width(720)
        .default_height(120)
        .build();

    dialog.set_extra_child(child);
    dialog.add_responses(&[
        (cancel_response, "Cancel"),
        (confirm_response, confirm_title),
    ]);

    dialog.set_response_appearance(
        confirm_response,
        ResponseAppearance::Suggested,
    );
    dialog
}

pub fn make_header_bar(sender: Sender<crate::Event>) -> (HeaderBar, impl Fn(OsString)) {
    let stashes_btn = Button::builder()
        .label("Stashes")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Stashes")
        .icon_name("sidebar-show-symbolic")
        .can_shrink(true)
        .build();
    stashes_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::StashesPanel)
                .expect("cant send through channel");
        }
    });
    let refresh_btn = Button::builder()
        .label("Refresh")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Refresh")
        .icon_name("view-refresh-symbolic")
        .can_shrink(true)
        .build();
    refresh_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Refresh)
                .expect("Could not send through channel");
        }
    });
    let zoom_out_btn = Button::builder()
        .label("Zoom out")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Zoom out")
        .icon_name("zoom-out-symbolic")
        .can_shrink(true)
        .margin_start(24)
        .margin_end(0)
        .build();
    zoom_out_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Zoom(false))
                .expect("cant send through channel");
        }
    });

    let zoom_in_btn = Button::builder()
        .label("Zoom in")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Zoom in")
        .icon_name("zoom-in-symbolic")
        .can_shrink(true)
        .margin_start(0)
        .build();
    zoom_in_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Zoom(true))
                .expect("cant send through channel");
        }
    });

    let branches_btn = Button::builder()
        .label("Branches")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Branches")
        .icon_name("org.gtk.gtk4.NodeEditor-symbolic")
        .can_shrink(true)
        .build();
    branches_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Branches)
                .expect("cant send through channel");
        }
    });

    let push_btn = Button::builder()
        .label("Push")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Push")
        .icon_name("send-to-symbolic")
        .can_shrink(true)
        .build();
    push_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Push)
                .expect("cant send through channel");
        }
    });
    let reset_btn = Button::builder()
        .label("Reset hard")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Reset hard")
        .icon_name("software-update-urgent-symbolic")
        .can_shrink(true)
        .build();
    reset_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::ResetHard)
                .expect("cant send through channel");
        }
    });

    let pull_btn = Button::builder()
        .label("Pull")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Pull")
        .icon_name("document-save-symbolic")
        .can_shrink(true)
        .build();
    pull_btn.connect_clicked({
        let sender = sender.clone();
        move |_| {
            sender
                .send_blocking(crate::Event::Pull)
                .expect("cant send through channel");
        }
    });

    let menu_items = Box::new(Orientation::Vertical, 0);
    // let item1 = Button::with_label("item 1");
    // let item2 = Button::with_label("item 2");
    // let item3 = Button::with_label("item 3");
    // menu_items.append(&item1);
    // menu_items.append(&item2);
    // menu_items.append(&item3);

    let popover = Popover::builder()
        .child(&menu_items)        
        .build();

    let opener = ButtonContent::builder()
        .icon_name("document-open-symbolic")
        // .label("/home/aganzha/stage/")
        .use_underline(true)
        .valign(Align::Baseline)
        .build();
    

    // let opener_label_var = Rc::new(RefCell::new(opener_label));
    debug!("zooooooooooooooooooooooooo {:p}", &opener);
    // let path_updater = glib::clone!(@weak opener as opener => move |path: OsString| {
    //     let opener_label = opener.last_child()
    //         .unwrap();
    //     let opener_label = opener_label.downcast_ref::<Label>()
    //         .unwrap();
    //     debug!("uuuuuuuuuuuuuuuupdate ----------------- path {:?}", path);
    //     opener_label.set_markup(&format!("<span weight=\"normal\">{:?}</span>", path));
    //     debug!("oooooooooooooooooooo {:p} {:?}", &opener, opener_label.label());
    // });
    let path_updater = {
        let opener = opener.clone();
        move |path: OsString| {
            debug!("uuuuuuuuuuuuuuuupdate ----------------- path {:?}", path);
            let opener_label = opener.last_child()
                .unwrap();
            let opener_label = opener_label.downcast_ref::<Label>()
                .unwrap();            
            opener_label.set_markup(&format!("<span weight=\"normal\">{:?}</span>", path.into_string().unwrap()));
            opener_label.set_visible(true);
            debug!("oooooooooooooooooooo {:p}", &opener);
        }
    };
    
    let repo_selector = SplitButton::new();    
    repo_selector.set_child(Some(&opener));
    repo_selector.set_popover(Some(&popover));
    
    let hb = HeaderBar::new();
    hb.pack_start(&stashes_btn);
    hb.pack_start(&refresh_btn);
    hb.pack_start(&zoom_out_btn);
    hb.pack_start(&zoom_in_btn);
    hb.set_title_widget(Some(&repo_selector));
    hb.pack_end(&branches_btn);
    hb.pack_end(&push_btn);
    hb.pack_end(&pull_btn);
    hb.pack_end(&reset_btn);
    (hb, path_updater)
}
