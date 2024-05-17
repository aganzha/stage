use crate::status_view::container::ViewContainer;
use crate::{get_commit_diff, CommitDiff, Event, StatusRenderContext};
use async_channel::Sender;
use git2::Oid;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, EventControllerKey, Label, ScrolledWindow,
    Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use log::debug;
use std::cell::RefCell;
use std::rc::Rc;

pub fn headerbar_factory(
    _repo_path: std::ffi::OsString,
    _oid: Oid,
    _sender: Sender<Event>,
) -> HeaderBar {
    let hb = HeaderBar::builder().build();
    let lbl = Label::builder()
        .label("Commit")
        .single_line_mode(true)
        .build();

    hb.set_title_widget(Some(&lbl));
    hb.set_show_end_title_buttons(true);
    hb.set_show_back_button(true);
    hb
}

pub fn show_commit_window(
    repo_path: std::ffi::OsString,
    oid: Oid,
    app_window: &impl IsA<Gtk4Window>,
    _main_sender: Sender<Event>,
) {
    let (sender, receiver) = async_channel::unbounded();

    let window = Window::builder()
        .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let scroll = ScrolledWindow::new();

    let hb = headerbar_factory(repo_path.clone(), oid, sender.clone());

    let text_view_width = Rc::new(RefCell::<(i32, i32)>::new((0, 0)));
    let txt = crate::textview_factory(sender.clone(), text_view_width.clone());

    scroll.set_child(Some(&txt));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        let _sender = sender.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::Escape, _) => {
                    window.close();
                }
                (gdk::Key::a, _) => {
                    debug!("key pressed {:?} {:?}", key, modifier);
                    // sender
                    //     .send_blocking(Event::CherryPickRequest)
                    //     .expect("Could not send through channel");
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);

    window.present();

    let mut main_diff: Option<CommitDiff> = None;

    gio::spawn_blocking({
        let path = repo_path.clone();
        move || {
            get_commit_diff(path, oid, sender);
        }
    });

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            let mut ctx = StatusRenderContext::new();
            ctx.screen_width.replace(*text_view_width.borrow());
            match event {
                Event::CommitDiff(mut commit_diff) => {
                    if main_diff.is_none() {
                        main_diff.replace(commit_diff.clone());
                    }
                    if let Some(d) = &mut main_diff {
                        let buffer = txt.buffer();
                        let mut iter = buffer.iter_at_offset(0);
                        let ctx = &mut Some(&mut ctx);
                        commit_diff.diff.enrich_view(&mut d.diff, &txt, ctx);
                        d.diff.render(&buffer, &mut iter, ctx);
                    }
                }
                Event::Expand(_offset, line_no) => {
                    if let Some(d) = &mut main_diff {
                        let buffer = txt.buffer();
                        let mut iter = buffer.iter_at_offset(0);
                        let mut need_render = false;
                        for file in &mut d.diff.files {
                            if let Some(_expanded_line) = file.expand(line_no)
                            {
                                need_render = true;
                                break;
                            }
                        }
                        if need_render {
                            d.diff.render(
                                &buffer,
                                &mut iter,
                                &mut Some(&mut ctx),
                            );
                        }
                    }
                }
                Event::Cursor(_offset, line_no) => {
                    if let Some(d) = &mut main_diff {
                        if d.diff.cursor(line_no, false, &mut None) {
                            let buffer = txt.buffer();
                            let mut iter = buffer.iter_at_offset(0);
                            d.diff.render(
                                &buffer,
                                &mut iter,
                                &mut Some(&mut ctx),
                            );
                        }
                    }
                }
                Event::TextViewResize => {
                    if let Some(d) = &mut main_diff {
                        debug!("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
                        d.diff.resize(&txt, &mut Some(&mut ctx));
                    }
                }
                _ => {
                    debug!("meeeeeeeeeeeeeeeeeeeeeerr {:?}", event);
                }
            }
        }
    });
}
