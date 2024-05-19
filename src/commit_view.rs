use crate::context::StatusRenderContext;
use crate::git::commit;
use crate::status_view::{container::ViewContainer, Label as TextViewLabel};
use crate::{with_git2ui_error, Event};
use async_channel::Sender;
use git2::{Oid};
use std::cell::RefCell;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, EventControllerKey, Label, ScrolledWindow, TextView, Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{
    AlertDialog, HeaderBar, ResponseAppearance, ToolbarView, Window,
};
use log::debug;

use std::path::PathBuf;
use std::rc::Rc;

pub fn headerbar_factory(
    _repo_path: PathBuf,
    _oid: Oid,
    // _sender: Sender<Event>,
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

impl commit::CommitDiff {
    fn render(
        &mut self,
        txt: &TextView,
        ctx: &mut Option<&mut StatusRenderContext>,
        labels: &mut [TextViewLabel],
    ) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);
        for l in labels {
            l.render(&buffer, &mut iter, ctx)
        }
        self.diff.render(&buffer, &mut iter, ctx);
    }
}

pub fn show_commit_window(
    repo_path: PathBuf,
    oid: Oid,
    app_window: &impl IsA<Gtk4Window>,
    _main_sender: Sender<Event>, // i need that to trigger revert and cherry-pick.
) {
    let (sender, receiver) = async_channel::unbounded();

    let window = Window::builder()
        // .application(&app_window.application().unwrap())
        .transient_for(app_window)
        .default_width(640)
        .default_height(480)
        .build();
    window.set_default_size(1280, 960);

    let scroll = ScrolledWindow::new();

    let hb = headerbar_factory(repo_path.clone(), oid);

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

    let mut main_diff: Option<commit::CommitDiff> = None;

    let path = repo_path.clone();
    with_git2ui_error!(
        move || commit::get_commit_diff(path, oid),
        |diff| {
            sender
                .send_blocking(Event::CommitDiff(diff))
                .expect("Could not send through channel");
        },
        &window.clone()
    );

    let mut labels: [TextViewLabel; 6] = [
        TextViewLabel::from_string(&format!("commit: {:?}", oid)),
        TextViewLabel::from_string(""),
        TextViewLabel::from_string(""),
        TextViewLabel::from_string(""),
        TextViewLabel::from_string(""),
        TextViewLabel::from_string(""),
    ];
    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            let mut ctx = crate::StatusRenderContext::new();
            ctx.screen_width.replace(*text_view_width.borrow());
            match event {
                Event::CommitDiff(mut commit_diff) => {
                    labels[1].content =
                        format!("Author: {}", commit_diff.author);
                    labels[2].content =
                        format!("Date: {}", commit_diff.commit_dt);
                    labels[4].content = commit_diff.message.to_string();
                    // hack to setup cursor
                    if !commit_diff.diff.files.is_empty() {
                        commit_diff.diff.files[0].view.current = true;
                    }
                    commit_diff.render(&txt, &mut Some(&mut ctx), &mut labels);
                    if !commit_diff.diff.files.is_empty() {
                        let buffer = txt.buffer();
                        let iter =
                            buffer.iter_at_line(labels.len() as i32).unwrap();
                        buffer.place_cursor(&iter);
                    }
                    main_diff.replace(commit_diff);
                }
                Event::Expand(_offset, line_no) => {
                    if let Some(d) = &mut main_diff {
                        let mut need_render = false;
                        for file in &mut d.diff.files {
                            need_render =
                                need_render || file.expand(line_no).is_some();
                            if need_render {
                                break;
                            }
                        }
                        if need_render {
                            d.render(&txt, &mut Some(&mut ctx), &mut labels);
                        }
                    }
                }
                Event::Cursor(_offset, line_no) => {
                    if let Some(d) = &mut main_diff {
                        if d.diff.cursor(line_no, false, &mut None) {
                            d.render(&txt, &mut Some(&mut ctx), &mut labels);
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
