// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::dialogs::alert;
use crate::git::{blame, commit, stash::StashNum};
use crate::status_view::context::StatusRenderContext;
use crate::status_view::{
    render::ViewContainer, stage_view::StageView, view::View, CursorPosition,
    Label as TextViewLabel,
};
use crate::{ApplyOp, BlameLine, CurrentWindow, Event, HunkLineNo, StageOp};
use async_channel::Sender;
use git2::Oid;

use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ScrolledWindow, TextBuffer, TextIter,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use log::{debug, info, trace};

use std::path::PathBuf;

pub fn headerbar_factory(
    sender: Sender<Event>,
    oid: Oid,
    stash_num: Option<StashNum>,
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
        blame_line: Option<BlameLine>,
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
        let mut found_line_index: Option<(usize, usize, usize)> = None;
        if !self.diff.files.is_empty() {
            if let Some(blame_line) = blame_line {
                for (f, file) in self.diff.files.iter().enumerate() {
                    if file.path == blame_line.file_path {
                        file.view.expand(true);
                        for (h, hunk) in file.hunks.iter().enumerate() {
                            let mut found = false;
                            if found_line_index.is_none() {
                                for (l, line) in hunk.lines.iter().enumerate() {
                                    if let Some(found_line_no) = line.new_line_no {
                                        if found_line_no >= blame_line.hunk_start
                                            && line.content(hunk) == blame_line.content
                                        {
                                            line.view.make_current(true);
                                            found = true;
                                            found_line_index.replace((f, h, l));
                                            break;
                                        }
                                    }
                                }
                            }
                            if !found {
                                hunk.view.expand(false);
                            }
                        }
                        if found_line_index.is_some() {
                            break;
                        }
                    }
                }
            } else {
                self.diff.files[0].view.make_current(true);
            }
        }

        self.diff.render(&buffer, &mut iter, ctx);
        if let Some((f, h, l)) = found_line_index {
            let line_no = self.diff.files[f].hunks[h].lines[l].view.line_no.get();
            let buffer = txt.buffer();
            iter = buffer.iter_at_line(line_no).unwrap();
            buffer.place_cursor(&iter);
            txt.scroll_to_iter(&mut iter, 0.0, false, 0.0, 0.0);
        } else if !self.diff.files.is_empty() {
            let buffer = txt.buffer();
            iter = buffer
                .iter_at_line(self.diff.files[0].view.line_no.get())
                .unwrap();
            buffer.place_cursor(&iter);
        }
        self.diff.cursor(&txt.buffer(), iter.line(), ctx);
        txt.bind_highlights(ctx);
    }
}

pub fn show_commit_window(
    repo_path: PathBuf,
    oid: Oid,
    stash_num: Option<StashNum>,
    blame_line: Option<BlameLine>,
    app_window: CurrentWindow,
    main_sender: Sender<Event>, // i need that to trigger revert and cherry-pick.
) -> Window {
    let (sender, receiver) = async_channel::unbounded();

    let mut diff: Option<commit::CommitDiff> = None;

    const MAX_WIDTH: i32 = 1280;

    let mut builder = Window::builder()
        .default_width(MAX_WIDTH)
        .default_height(960);
    match app_window.clone() {
        CurrentWindow::Window(w) => {
            builder = builder.transient_for(&w);
        }
        CurrentWindow::ApplicationWindow(w) => {
            builder = builder.transient_for(&w);
        }
    }
    let window = builder.build();
    let scroll = ScrolledWindow::new();

    let hb = headerbar_factory(main_sender.clone(), oid, stash_num);

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
        let path = path.clone();
        async move {
            let diff = gio::spawn_blocking(move || commit::get_commit_diff(path.clone(), oid))
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
                            blame_line.clone(),
                        );
                        cursor_position = CursorPosition::from_context(&ctx);
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
                                d.diff.cursor(buffer, iter.line(), &mut ctx);
                                txt.bind_highlights(&ctx);
                                cursor_position = CursorPosition::from_context(&ctx);
                            }
                        }
                    }
                    Event::Cursor(_offset, line_no) => {
                        if let Some(d) = &mut diff {
                            let buffer = &txt.buffer();
                            d.diff.cursor(buffer, line_no, &mut ctx);
                            cursor_position = CursorPosition::from_context(&ctx);
                        }
                        // it should be called after cursor in ViewContainer !!!!!!!!
                        txt.bind_highlights(&ctx);
                    }
                    Event::TextViewResize(w) => {
                        info!("TextViewResize {} {:?}", w, ctx);
                    }
                    Event::Debug => {
                        let buffer = txt.buffer();
                        let pos = buffer.cursor_position();
                        let iter = buffer.iter_at_offset(pos);
                        for tag in iter.tags() {
                            debug!("Tag: {}", tag.name().unwrap());
                        }
                    }
                    Event::Stage(op) if op != StageOp::Kill => {
                        info!(
                            "Stage/Unstage or r pressed {:?} cursor position {:?}",
                            op, cursor_position
                        );
                        if let Some(diff) = &diff {
                            let (file_path, hunk_header) = match cursor_position {
                                CursorPosition::CursorDiff(_) => (None, None),
                                CursorPosition::CursorFile(_, file_idx) => {
                                    let file = &diff.diff.files[file_idx];
                                    (Some(file.path.clone()), None)
                                }
                                CursorPosition::CursorHunk(_, file_idx, hunk_idx)
                                | CursorPosition::CursorLine(_, file_idx, hunk_idx, _) => {
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
                    Event::Blame => {
                        let mut line_no: Option<HunkLineNo> = None;
                        let mut ofile_path: Option<PathBuf> = None;
                        let mut oline_content: Option<String> = None;
                        if let CursorPosition::CursorLine(_, file_idx, hunk_idx, line_idx) =
                            cursor_position
                        {
                            if let Some(diff) = &diff {
                                let file = &diff.diff.files[file_idx];
                                let hunk = &file.hunks[hunk_idx];
                                ofile_path.replace(file.path.clone());
                                let line = &hunk.lines[line_idx];
                                oline_content.replace(line.content(hunk).to_string());
                                // IMPORTANT - here we use new_line_no
                                line_no = line.new_line_no;
                            }
                        }
                        if let Some(line_no) = line_no {
                            glib::spawn_future_local({
                                let path = path.clone();
                                let sender = main_sender.clone();
                                let file_path = ofile_path.clone().unwrap();
                                let window = window.clone();
                                async move {
                                    let ooid = gio::spawn_blocking({
                                        let file_path = file_path.clone();
                                        move || blame(path, file_path.clone(), line_no, Some(oid))
                                    })
                                    .await
                                    .unwrap();
                                    match ooid {
                                        Ok((blame_oid, hunk_line_start)) => {
                                            if blame_oid == oid {
                                                alert(format!("This is the same commit {:?}", oid))
                                                    .present(Some(&window));
                                                return;
                                            }
                                            sender
                                                .send_blocking(crate::Event::ShowOid(
                                                    blame_oid,
                                                    None,
                                                    Some(BlameLine {
                                                        file_path,
                                                        hunk_start: hunk_line_start,
                                                        content: oline_content.unwrap(),
                                                    }),
                                                ))
                                                .expect("Could not send through channel");
                                        }
                                        Err(e) => alert(e).present(Some(&window))
                                    }
                                }
                            });
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
