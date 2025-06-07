// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::dialogs::{alert, ConfirmDialog, YES};
use crate::git::commit;
use crate::status_view::context::StatusRenderContext;
use crate::status_view::{
    render::ViewContainer, stage_view::StageView, view::View, CursorPosition,
    Label as TextViewLabel,
};
use crate::{ApplyOp, CurrentWindow, Event, StageOp};
use async_channel::Sender;
use git2::Oid;

use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ScrolledWindow, TextBuffer, TextIter, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use log::{debug, info, trace};

use std::path::PathBuf;

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
            alert(format!("{:?}", e)).present(Some(&window));
            Ok(())
        })
        .unwrap_or_else(|e| {
            alert(e).present(Some(&window));
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
        move |_| {
            let sender = sender.clone();
            let apply_op = if let Some(stash_num) = stash_num {
                ApplyOp::Stash(oid, stash_num, None, None)
            } else {
                ApplyOp::CherryPick(oid, None, None)
            };
            sender
                .send_blocking(crate::Event::Apply(apply_op))
                .expect("cant send through channel");
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
            move |_| {
                sender
                    .send_blocking(crate::Event::Apply(ApplyOp::Revert(oid, None, None)))
                    .expect("cant send through channel");
            }
        });
        hb.pack_end(&revert_btn);
    }
    hb
}

#[derive(Debug, Clone)]
pub struct MultiLineLabel {
    pub labels: Vec<TextViewLabel>,
    pub view: View,
}

impl MultiLineLabel {
    pub fn new(content: &str, visible_chars: i32) -> Self {
        let mut mll = MultiLineLabel {
            labels: Vec::new(),
            view: View::new(),
        };
        mll.make_labels(content, visible_chars);
        mll
    }

    pub fn make_labels(&mut self, content: &str, visible_chars: i32) {
        self.labels = Vec::new();
        let mut acc = String::from("");

        // split first by new lines. each new line in commit must go
        // on its own, separate label. BUT!
        // also split long lines to different labels also!
        for line in content.split('\n') {
            trace!("split {:?}", line);
            let mut split = line.split(' ');
            let mut mx = 0;

            let chars = visible_chars;
            'words: loop {
                mx += 1;
                if mx > 20 {
                    break 'words;
                }
                while acc.len() < chars as usize {
                    if let Some(word) = split.next() {
                        trace!("got word > {} <", word);
                        if acc.len() + word.len() > chars as usize {
                            self.labels
                                .push(TextViewLabel::from_string(&acc.replace('\n', "")));
                            trace!("init new acc after width end {:?}", acc);
                            acc = String::from(word);
                        } else {
                            trace!("just push word {:?}", word);
                            acc.push_str(word);
                            acc.push(' ');
                        }
                    } else {
                        trace!("completed internal loop. acc: {:?}", acc);
                        self.labels
                            .push(TextViewLabel::from_string(&acc.replace('\n', "")));
                        acc = String::from("");
                        break 'words;
                    }
                }
                trace!("reach line end. push label! {:?}", acc);
                self.labels
                    .push(TextViewLabel::from_string(&acc.replace('\n', "")));
                acc = String::from("");
            }
        }
        // space for following diff
        self.labels.push(TextViewLabel::from_string(""));
        self.view.expand(true)
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
        // body_label.update_content(&self.message, txt.calc_max_char_width());
        body_label.render(&buffer, &mut iter, ctx);

        if !self.diff.files.is_empty() {
            self.diff.files[0].view.make_current(true);
        }

        self.diff.render(&buffer, &mut iter, ctx);

        if !self.diff.files.is_empty() {
            let buffer = txt.buffer();
            iter = buffer
                .iter_at_line(self.diff.files[0].view.line_no.get())
                .unwrap();
            buffer.place_cursor(&iter);
        }
        self.diff.cursor(&txt.buffer(), iter.line(), true, ctx);
        txt.bind_highlights(ctx);
    }
}

pub fn show_commit_window(
    repo_path: PathBuf,
    oid: Oid,
    stash_num: Option<usize>,
    app_window: CurrentWindow,
    main_sender: Sender<Event>, // i need that to trigger revert and cherry-pick.
) -> Window {
    let (sender, receiver) = async_channel::unbounded();

    let mut diff: Option<commit::CommitDiff> = None;

    const MAX_WIDTH: i32 = 1280;

    let mut builder = Window::builder()
        .default_width(MAX_WIDTH)
        .default_height(960);
    match app_window {
        CurrentWindow::Window(w) => {
            builder = builder.transient_for(&w);
        }
        CurrentWindow::ApplicationWindow(w) => {
            builder = builder.transient_for(&w);
        }
    }
    let window = builder.build();
    let scroll = ScrolledWindow::new();

    let hb = headerbar_factory(
        repo_path.clone(),
        &window.clone(),
        main_sender.clone(),
        oid,
        stash_num,
    );

    let txt = crate::stage_factory(sender.clone(), "commit_view");

    scroll.set_child(Some(&txt));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    let event_controller = EventControllerKey::new();
    event_controller.connect_key_pressed({
        let window = window.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) | (gdk::Key::Escape, _) => {
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

    let mut cursor_position: CursorPosition = CursorPosition::None;

    glib::spawn_future_local({
        let window = window.clone();
        let sender = sender.clone();
        async move {
            let diff = gio::spawn_blocking(move || commit::get_commit_diff(path, oid))
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(Some(&window));
                    Ok(commit::CommitDiff::default())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(Some(&window));
                    commit::CommitDiff::default()
                });
            sender
                .send_blocking(Event::CommitDiff(diff))
                .expect("Could not send through channel");
        }
    });

    let mut labels: [TextViewLabel; 3] = [
        TextViewLabel::from_string(&format!("commit: <span color=\"#4a708b\">{:?}</span>", oid)),
        TextViewLabel::from_string(""),
        TextViewLabel::from_string(""),
    ];

    glib::spawn_future_local({
        let window = window.clone();
        async move {
            while let Ok(event) = receiver.recv().await {
                let mut ctx = crate::StatusRenderContext::new(&txt);
                match event {
                    Event::CommitDiff(mut commit_diff) => {
                        info!("CommitDiff");

                        labels[1].content = format!(
                            "Author: <span color=\"#4a708b\">{}</span>",
                            commit_diff.author
                        );
                        labels[2].content = format!(
                            "Date: <span color=\"#4a708b\">{}</span>",
                            commit_diff.commit_dt
                        );
                        body_label.replace(MultiLineLabel::new(
                            &commit_diff.message,
                            txt.calc_max_char_width(MAX_WIDTH),
                        ));
                        commit_diff.render(
                            &txt,
                            &mut ctx,
                            &mut labels,
                            body_label.as_mut().unwrap(),
                        );
                        // it should be called after cursor in ViewContainer
                        diff.replace(commit_diff);
                    }
                    Event::Expand(_offset, line_no) => {
                        info!("Expand {}", line_no);
                        if let Some(d) = &mut diff {
                            if d.diff.expand(line_no, &mut ctx).is_some() {
                                let buffer = &txt.buffer();
                                let mut iter =
                                    buffer.iter_at_line(d.diff.view.line_no.get()).unwrap();
                                d.diff.render(buffer, &mut iter, &mut ctx);
                                let iter = buffer.iter_at_offset(buffer.cursor_position());
                                d.diff.cursor(buffer, iter.line(), true, &mut ctx);
                                txt.bind_highlights(&ctx);
                            }
                        }
                    }
                    Event::Cursor(_offset, line_no) => {
                        if let Some(d) = &mut diff {
                            let buffer = &txt.buffer();
                            d.diff.cursor(buffer, line_no, false, &mut ctx);
                            cursor_position = CursorPosition::from_context(&ctx);
                        }
                        // it should be called after cursor in ViewContainer !!!!!!!!
                        txt.bind_highlights(&ctx);
                    }
                    Event::TextViewResize(w) => {
                        info!("TextViewResize {} {:?}", w, ctx);
                    }
                    Event::TextCharVisibleWidth(w) => {
                        info!("TextCharVisibleWidth {}", w);
                        if let Some(d) = &mut diff {
                            d.render(&txt, &mut ctx, &mut labels, body_label.as_mut().unwrap());
                        }
                    }
                    Event::Debug => {
                        let buffer = txt.buffer();
                        let pos = buffer.cursor_position();
                        let iter = buffer.iter_at_offset(pos);
                        debug!("==========================");
                        for tag in iter.tags() {
                            println!("Tag: {}", tag.name().unwrap());
                        }
                    }
                    Event::Stage(op) if op != StageOp::Kill => {
                        info!("Stage/Unstage or r pressed {:?}", op);
                        if let Some(diff) = &diff {
                            let (file_path, hunk_header) = match cursor_position {
                                CursorPosition::CursorDiff(_) => (None, None),
                                CursorPosition::CursorFile(_, Some(file_idx)) => {
                                    let file = &diff.diff.files[file_idx];
                                    (Some(file.path.clone()), None)
                                }
                                CursorPosition::CursorHunk(_, Some(file_idx), Some(hunk_idx))
                                | CursorPosition::CursorLine(
                                    _,
                                    Some(file_idx),
                                    Some(hunk_idx),
                                    _,
                                ) => {
                                    let file = &diff.diff.files[file_idx];
                                    let hunk = &file.hunks[hunk_idx];
                                    (Some(file.path.clone()), Some(hunk.header.clone()))
                                }
                                _ => (None, None),
                            };
                            let apply_op = if let Some(stash_num) = stash_num {
                                ApplyOp::Stash(oid, stash_num, file_path, hunk_header)
                            } else {
                                match op {
                                    StageOp::Stage => {
                                        ApplyOp::CherryPick(oid, file_path, hunk_header)
                                    }
                                    StageOp::Unstage => {
                                        ApplyOp::Revert(oid, file_path, hunk_header)
                                    }
                                    _ => {
                                        unreachable!("no way")
                                    }
                                }
                            };
                            main_sender
                                .send_blocking(crate::Event::Apply(apply_op))
                                .expect("cant send through channel");
                        }
                    }
                    _ => {
                        trace!("unhandled event in commit_view {:?}", event);
                    }
                }
            }
        }
    });
    window
}
