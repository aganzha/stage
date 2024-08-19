// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::dialogs::{alert, ConfirmDialog, YES};
use crate::git::{commit, stash};
use crate::status_view::context::{StatusRenderContext, TextViewWidth};
use crate::status_view::{
    render::{ViewContainer},
    stage_view::StageView,
    view::View,
    Label as TextViewLabel,
};
use crate::Event;
use async_channel::Sender;
use git2::Oid;
use std::cell::RefCell;

use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ScrolledWindow,
    TextBuffer, TextIter, Widget, Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use log::{debug, info, trace};

use std::path::PathBuf;
use std::rc::Rc;

async fn git_oid_op<F>(dialog: ConfirmDialog, window: impl IsA<Widget>, op: F)
where
    F: FnOnce() -> Result<(), git2::Error> + Send + 'static,
{
    let response = alert(dialog).choose_future(&window).await;
    if response != YES {
        return;
    }
    gio::spawn_blocking(op)
        .await
        .unwrap_or_else(|e| {
            alert(format!("{:?}", e)).present(&window);
            Ok(())
        })
        .unwrap_or_else(|e| {
            alert(e).present(&window);
        });
}

pub fn headerbar_factory(
    repo_path: PathBuf,
    window: &impl IsA<Widget>,
    sender: Sender<Event>,
    oid: Oid,
    stash_num: Option<usize>,
) -> HeaderBar {
    let hb = HeaderBar::builder().build();
    let (btn_tooltip, title) = if stash_num.is_some() {
        ("Apply stash", "Stash")
    } else {
        ("Cherry pick", "Commit")
    };

    let lbl = Label::builder().label(title).single_line_mode(true).build();

    hb.set_title_widget(Some(&lbl));

    let cherry_pick_btn = Button::builder()
        .icon_name("emblem-shared-symbolic")
        .can_shrink(true)
        .tooltip_text(btn_tooltip)
        .sensitive(true)
        .use_underline(true)
        .build();

    cherry_pick_btn.connect_clicked({
        let sender = sender.clone();
        let path = repo_path.clone();
        let window = window.clone();
        move |_| {
            let sender = sender.clone();
            let path = path.clone();
            let window = window.clone();
            if let Some(num) = stash_num {
                glib::spawn_future_local({
                    git_oid_op(
                        ConfirmDialog(
                            "Apply stash?".to_string(),
                            "".to_string(),
                        ),
                        window,
                        move || stash::apply(path, num, None, None, sender),
                    )
                });
            } else {
                glib::spawn_future_local({
                    git_oid_op(
                        ConfirmDialog(
                            "Cherry pick commit?".to_string(),
                            "".to_string(),
                        ),
                        window,
                        move || {
                            commit::cherry_pick(path, oid, None, None, sender)
                        },
                    )
                });
            }
        }
    });
    hb.pack_end(&cherry_pick_btn);
    if stash_num.is_none() {
        let revert_btn = Button::builder()
            .icon_name("edit-undo-symbolic")
            .can_shrink(true)
            .tooltip_text("Revert")
            .sensitive(true)
            .use_underline(true)
            .build();

        revert_btn.connect_clicked({
            let window = window.clone();
            move |_| {
                let sender = sender.clone();
                let path = repo_path.clone();
                let window = window.clone();
                glib::spawn_future_local({
                    git_oid_op(
                        ConfirmDialog(
                            "Revert commit?".to_string(),
                            "".to_string(),
                        ),
                        window,
                        move || commit::revert(path, oid, None, None, sender),
                    )
                });
            }
        });
        hb.pack_end(&revert_btn);
    }
    hb
}

#[derive(Debug, Clone)]
pub struct MultiLineLabel {
    pub content: String,
    pub labels: Vec<TextViewLabel>,
    pub view: View,
}

impl MultiLineLabel {
    pub fn new(content: &str, context: &mut StatusRenderContext) -> Self {
        let mut mll = MultiLineLabel {
            content: content.to_string(),
            labels: Vec::new(),
            view: View::new(),
        };
        mll.update_content(content, context);
        mll
    }

    pub fn update_content(
        &mut self,
        content: &str,
        context: &mut StatusRenderContext,
    ) {
        self.labels = Vec::new();
        let mut acc = String::from("");

        debug!("........................... {}", content);
        // split first by new lines. each new line in commit must go
        // on its own, separate label. BUT!
        // also split long lines to different labels also!
        let user_split = content.split('\n');

        for line in user_split {
            trace!("split {:?}", line);
            let mut split = line.split(' ');
            let mut mx = 0;

            if let Some(width) = &context.screen_width {
                let pixels = width.borrow().pixels;
                let mut chars = width.borrow().chars;
                let visible_chars = width.borrow().visible_chars;
                if visible_chars > 0 && visible_chars < chars {
                    chars = visible_chars;
                }
                let visible_chars = width.borrow().visible_chars;
                trace!(
                    "..........looop words acc {} chars {} visible_chars {}",
                    pixels,
                    chars,
                    visible_chars
                );
                'words: loop {
                    mx += 1;
                    if mx > 20 {
                        break 'words;
                    }
                    while acc.len() < chars as usize {
                        if let Some(word) = split.next() {
                            trace!("got word > {} <", word);
                            if acc.len() + word.len() > chars as usize {
                                self.labels.push(TextViewLabel::from_string(
                                    &acc.replace('\n', ""),
                                ));
                                trace!(
                                    "init new acc after width end {:?}",
                                    acc
                                );
                                acc = String::from(word);
                            } else {
                                trace!("just push word {:?}", word);
                                acc.push_str(word);
                                acc.push(' ');
                            }
                        } else {
                            trace!(
                                "words are over! push last label! {:?}",
                                acc
                            );
                            self.labels.push(TextViewLabel::from_string(
                                &acc.replace('\n', ""),
                            ));
                            acc = String::from("");
                            break 'words;
                        }
                    }
                    trace!("reach line end. push label! {:?}", acc);
                    self.labels.push(TextViewLabel::from_string(
                        &acc.replace('\n', ""),
                    ));
                    acc = String::from("");
                }
            }
        }
        // space for following diff
        self.labels.push(TextViewLabel::from_string(""));
        self.view.expand(true) // expanded = true;
    }
}

impl ViewContainer for MultiLineLabel {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        self.labels.is_empty()
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        self.labels
            .iter()
            .map(|vh| vh as &dyn ViewContainer)
            .collect()
    }

    fn write_content(
        &self,
        _iter: &mut TextIter,
        _buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
    }
}

impl commit::CommitDiff {
    fn render<'a>(
        &'a mut self,
        txt: &StageView,
        ctx: &mut StatusRenderContext<'a>,
        labels: &'a mut [TextViewLabel],
        body_label: &'a mut MultiLineLabel,
    ) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);

        for l in labels {
            l.render(&buffer, &mut iter, ctx)
        }
        let offset_before_erase = iter.offset();
        for l in &mut body_label.labels {
            l.erase(&buffer, ctx);
        }
        iter = buffer.iter_at_offset(offset_before_erase);
        // ??? why it was commented out?
        body_label.update_content(&self.message, ctx);
        body_label.render(&buffer, &mut iter, ctx);

        if !self.diff.files.is_empty() {
            self.diff.files[0].view.make_current(true);
        }

        self.diff.render(&buffer, &mut iter, ctx);

        if !self.diff.files.is_empty() {
            let buffer = txt.buffer();
            let iter = buffer
                .iter_at_line(self.diff.files[0].view.line_no.get())
                .unwrap();
            buffer.place_cursor(&iter);
        }
    }
}

pub fn show_commit_window(
    repo_path: PathBuf,
    oid: Oid,
    stash_num: Option<usize>,
    app_window: &impl IsA<Gtk4Window>,
    main_sender: Sender<Event>, // i need that to trigger revert and cherry-pick.
) {
    let (sender, receiver) = async_channel::unbounded();

    let mut diff: Option<commit::CommitDiff> = None;

    let window = Window::builder()
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
        stash_num,
    );

    let text_view_width =
        Rc::new(RefCell::<TextViewWidth>::new(TextViewWidth::default()));
    let txt = crate::stage_factory(
        sender.clone(),
        "commit_view",
        text_view_width.clone(),
    );

    scroll.set_child(Some(&txt));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK)
                | (gdk::Key::Escape, _) => {
                    window.close();
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);

    window.present();

    let mut body_label: Option<MultiLineLabel> = None;

    let path = repo_path.clone();

    glib::spawn_future_local({
        let window = window.clone();
        let sender = sender.clone();
        async move {
            let diff = gio::spawn_blocking(move || {
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
            sender
                .send_blocking(Event::CommitDiff(diff))
                .expect("Could not send through channel");
        }
    });

    let mut labels: [TextViewLabel; 3] = [
        TextViewLabel::from_string(&format!(
            "commit: <span color=\"#4a708b\">{:?}</span>",
            oid
        )),
        TextViewLabel::from_string(""),
        TextViewLabel::from_string(""),
    ];

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            let mut ctx = crate::StatusRenderContext::new();
            ctx.screen_width.replace(text_view_width.clone());
            match event {
                Event::CommitDiff(mut commit_diff) => {
                    info!("CommitDiff");
                    // update it before render to get some width in chars
                    ctx.update_screen_line_width(
                        commit_diff.diff.max_line_len,
                    );
                    labels[1].content = format!(
                        "Author: <span color=\"#4a708b\">{}</span>",
                        commit_diff.author
                    );
                    labels[2].content = format!(
                        "Date: <span color=\"#4a708b\">{}</span>",
                        commit_diff.commit_dt
                    );

                    body_label.replace(MultiLineLabel::new("", &mut ctx));
                    commit_diff.render(
                        &txt,
                        &mut ctx,
                        &mut labels,
                        body_label.as_mut().unwrap(),
                    );
                    txt.bind_highlights(&ctx);
                    diff.replace(commit_diff);
                }
                Event::Expand(_offset, line_no) => {
                    info!("Expand {}", line_no);
                    if let Some(d) = &mut diff {
                        let mut need_render = false;
                        for file in &mut d.diff.files {
                            need_render = need_render
                                || file.expand(line_no, &mut ctx).is_some();
                            if need_render {
                                break;
                            }
                        }
                        let buffer = &txt.buffer();
                        let mut iter = buffer
                            .iter_at_line(d.diff.view.line_no.get())
                            .unwrap();
                        // let mut iter = buffer
                        //     .iter_at_line(d.diff.files[0].view.line_no.get())
                        //     .unwrap();
                        if need_render {
                            d.diff.render(buffer, &mut iter, &mut ctx);
                            txt.bind_highlights(&ctx);
                        }
                    }
                }
                Event::Cursor(_offset, line_no) => {
                    ctx.cursor = line_no;
                    if let Some(d) = &mut diff {
                        let buffer = &txt.buffer();
                        if d.diff.cursor(buffer, line_no, false, &mut ctx) {
                            let mut iter = buffer
                                .iter_at_line(d.diff.view.line_no.get())
                                .unwrap();
                            // will render diff whithout rendering
                            // preceeding elements!
                            // is it ok? perhaps yes, cause they are on top of it
                            d.diff.render(buffer, &mut iter, &mut ctx);
                        }
                    }
                    txt.bind_highlights(&ctx);
                }
                Event::TextViewResize(w) => {
                    info!("TextViewResize {} {:?}", w, ctx);
                    ctx.screen_width.replace(text_view_width.clone());
                }
                Event::TextCharVisibleWidth(w) => {
                    info!("TextCharVisibleWidth {}", w);
                    ctx.screen_width.replace(text_view_width.clone());
                    if let Some(d) = &mut diff {
                        d.render(
                            &txt,
                            &mut ctx,
                            &mut labels,
                            body_label.as_mut().unwrap(),
                        );
                    }
                }
                Event::Stage(_)
                | Event::RepoPopup => {
                    info!("Stage/Unstage ot r pressed");
                    if let Some(diff) = &diff {
                        let title = if stash_num.is_some() {
                            "Apply stash"
                        } else {
                            match event {
                                Event::Stage(_) => "Cherry pick",
                                _ => "Revert",
                            }
                        };

                        let (body, file_path, hunk_header) = match diff.diff.chosen_file_and_hunk() {
                            (Some(file), Some(hunk)) => {
                                (
                                    format!("File: {}\nApplying single hunks is not yet implemented :(", file.path.to_str().unwrap()),
                                    Some(file.path.clone()),
                                    Some(hunk.header.clone())
                                )
                            }
                            (Some(file), None) => {
                                (
                                    format!("File: {}", file.path.to_str().unwrap()),
                                    Some(file.path.clone()),
                                    None
                                )
                            }
                            (None, Some(hunk)) => {
                                panic!("hunk header without file {:?}", hunk.header);
                            }
                            (None, None) => {
                                (
                                    "".to_string(),
                                    None,
                                    None
                                )
                            }
                        };
                        let path = repo_path.clone();
                        let sender = main_sender.clone();
                        let window = window.clone();
                        glib::spawn_future_local({
                            git_oid_op(
                                ConfirmDialog(
                                    title.to_string(),
                                    body.to_string(),
                                ),
                                window,
                                move || match event {
                                    Event::Stage(_) => {
                                        if let Some(stash_num) = stash_num {
                                            stash::apply(
                                                path,
                                                stash_num,
                                                file_path,
                                                hunk_header,
                                                sender,
                                            )
                                        } else {
                                            commit::cherry_pick(
                                                path,
                                                oid,
                                                file_path,
                                                hunk_header,
                                                sender,
                                            )
                                        }
                                    }
                                    _ => commit::revert(
                                        path,
                                        oid,
                                        file_path,
                                        hunk_header,
                                        sender,
                                    ),
                                },
                            )
                        });
                        debug!(
                            "++++++++++++++++++++++++ {:?} {:?}",
                            title, body
                        );
                    }
                }
                _ => {
                    trace!("unhandled event in commit_view {:?}", event);
                }
            }
        }
    });
}
