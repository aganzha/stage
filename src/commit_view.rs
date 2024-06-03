use crate::context::{StatusRenderContext, TextViewWidth};
use crate::git::commit;
use crate::status_view::{container::ViewContainer, Label as TextViewLabel};
use crate::widgets::{alert, YesNoString, YES};
use crate::Event;
use async_channel::Sender;
use git2::Oid;
use std::cell::RefCell;


use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ScrolledWindow,
    TextView, Widget, Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use log::{debug, info, trace};

use std::path::PathBuf;
use std::rc::Rc;

pub fn headerbar_factory(
    repo_path: PathBuf,
    window: &impl IsA<Widget>,
    sender: Sender<Event>,
    oid: Oid,
    // _sender: Sender<Event>,
) -> HeaderBar {
    let hb = HeaderBar::builder().build();
    let lbl = Label::builder()
        .label("Commit")
        .single_line_mode(true)
        .build();

    hb.set_title_widget(Some(&lbl));

    let cherry_pick_btn = Button::builder()
        .icon_name("emblem-shared-symbolic")
        .can_shrink(true)
        .tooltip_text("Cherry-pick")
        .sensitive(true)
        .use_underline(true)
        .build();
    cherry_pick_btn.connect_clicked({
        let sender = sender.clone();
        let path = repo_path.clone();
        let window = window.clone();
        move |_btn| {
            glib::spawn_future_local({
                let sender = sender.clone();
                let path = path.clone();
                let window = window.clone();
                async move {
                    let response = alert(YesNoString(
                        "Cherry pick commit?".to_string(),
                        format!("{}", oid),
                    ))
                    .choose_future(&window)
                    .await;
                    if response != YES {
                        return;
                    }
                    gio::spawn_blocking({
                        let sender = sender.clone();
                        let path = path.clone();
                        move || commit::cherry_pick(path, oid, sender)
                    })
                        .await
                        .unwrap_or_else(|e| {
                            alert(format!("{:?}", e)).present(&window);
                            Ok(None)
                        })
                        .unwrap_or_else(
                            |e| {
                                alert(e).present(&window);
                                None
                            },
                        );
                }
            });
        }
    });

    let revert_btn = Button::builder()
        .icon_name("edit-undo-symbolic")
        .can_shrink(true)
        .tooltip_text("Revert")
        .sensitive(true)
        .use_underline(true)
        .build();
    hb.pack_end(&cherry_pick_btn);
    hb.pack_end(&revert_btn);
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
    main_sender: Sender<Event>, // i need that to trigger revert and cherry-pick.
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

    let hb = headerbar_factory(
        repo_path.clone(),
        &window.clone(),
        main_sender.clone(),
        oid,
    );

    let text_view_width =
        Rc::new(RefCell::<TextViewWidth>::new(TextViewWidth::default()));
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
                    trace!("key pressed {:?} {:?}", key, modifier);
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

    glib::spawn_future_local({
        let window = window.clone();
        async move {
            let commit_diff = gio::spawn_blocking(move || {
                commit::get_commit_diff(path, oid)
            })
            .await
            .unwrap_or_else(|e| {
                alert(format!("{:?}", e)).present(&window);
                Ok(commit::CommitDiff::default())
            })
            .unwrap_or_else(|e| {
                alert(e).present(&window);
                commit::CommitDiff::default()
            });
            if !commit_diff.is_empty() {
                sender
                    .send_blocking(Event::CommitDiff(commit_diff))
                    .expect("Could not send through channel");
            }
        }
    });

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
            ctx.screen_width.replace(text_view_width.clone());
            match event {
                Event::CommitDiff(mut commit_diff) => {
                    info!("commit_diff");
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
                    info!("expand");
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
                    trace!("cursor");
                    if let Some(d) = &mut main_diff {
                        if d.diff.cursor(line_no, false, &mut None) {
                            d.render(&txt, &mut Some(&mut ctx), &mut labels);
                            // let buffer = txt.buffer();
                            // let iter = &buffer.iter_at_offset(offset);
                            // buffer.place_cursor(iter);
                        }
                    }
                }
                Event::TextViewResize => {
                    info!("resize");
                    if let Some(d) = &mut main_diff {
                        let buffer = &txt.buffer();
                        // during resize some views are build up
                        // and cursor could move
                        let cursor_before = buffer.cursor_position();
                        d.diff.resize(buffer, &mut Some(&mut ctx));
                        // restore it
                        // TODO! perhaps move it to common render method???
                        buffer.place_cursor(
                            &buffer.iter_at_offset(cursor_before),
                        );
                    }
                }
                _ => {
                    debug!("unhandled event in commit_view {:?}", event);
                }
            }
        }
    });
}
