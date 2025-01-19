// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::dialogs::{alert, ConfirmDialog, YES};
use crate::git::{commit, stash};
use crate::status_view::context::StatusRenderContext;
use crate::status_view::{
    render::ViewContainer, stage_view::StageView, view::View, CursorPosition,
    Label as TextViewLabel,
};
use crate::{DiffKind, Event};
use async_channel::Sender;
use git2::Oid;

use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, pango, Box, Button, EventControllerKey, Label, Orientation, Overflow,
    PolicyType, ScrolledWindow, TextBuffer, TextIter, Widget, Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, StyleManager, ToolbarView, Window};
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
        let path = repo_path.clone();
        let window = window.clone();
        move |_| {
            let sender = sender.clone();
            let path = path.clone();
            let window = window.clone();
            if let Some(num) = stash_num {
                glib::spawn_future_local({
                    git_oid_op(
                        ConfirmDialog("Apply stash?".to_string(), format!("{}", num)),
                        window,
                        move || stash::apply(path, num, None, None, sender),
                    )
                });
            } else {
                sender
                    .send_blocking(crate::Event::CherryPick(oid, false, None, None))
                    .expect("cant send through channel");
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
            move |_| {
                sender
                    .send_blocking(crate::Event::CherryPick(oid, true, None, None))
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
        if let Some(map) = ctx.map {
            map.bind_highlights(&ctx);
        }
    }
}

pub fn show_commit_window(
    repo_path: PathBuf,
    oid: Oid,
    stash_num: Option<usize>,
    app_window: &impl IsA<Gtk4Window>,
    main_sender: Sender<Event>, // i need that to trigger revert and cherry-pick.
) -> Window {
    let (sender, receiver) = async_channel::unbounded();

    let mut diff: Option<commit::CommitDiff> = None;

    const MAX_WIDTH: i32 = 1280;
    let window = Window::builder()
        .transient_for(app_window)
        .default_width(MAX_WIDTH)
        .default_height(960)
        .build();

    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();

    let hb = headerbar_factory(
        repo_path.clone(),
        &window.clone(),
        main_sender.clone(),
        oid,
        stash_num,
    );

    let (txt, map) = crate::make_stage(sender.clone(), "commit_view", &scroll);

    scroll.set_child(Some(&txt));

    let map_box = Box::builder()
        .hexpand(true)
        .vexpand(true)
        .overflow(Overflow::Hidden)
        .orientation(Orientation::Horizontal)
        .build();
    
    map_box.append(&scroll);

    // let map_scroll = ScrolledWindow::builder()
    //     .hexpand(false)
    //     .vexpand(false)
    //     .hscrollbar_policy(PolicyType::Never)
    //     .vscrollbar_policy(PolicyType::External)
    //     .overflow(Overflow::Hidden)
    //     .build();
    // map_scroll.set_child(Some(&map));
    // map_box.append(&map_scroll);
    map_box.append(&map);
    
    let tb = ToolbarView::builder().content(&map_box).build();
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
    map.after_window_present();

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
    let manager = StyleManager::default();
    let color = if manager.is_dark() {
        "#34cae2"
    } else {
        "#4a708b"
    };
    let mut labels: [TextViewLabel; 3] = [
        TextViewLabel::from_string(&format!(
            "commit: <span color=\"{}\">{:?}</span>",
            color, oid
        )),
        TextViewLabel::from_string(""),
        TextViewLabel::from_string(""),
    ];

    glib::spawn_future_local({
        let window = window.clone();
        async move {
            while let Ok(event) = receiver.recv().await {
                let mut ctx = crate::StatusRenderContext::new();
                ctx.stage = Some(&txt);
                ctx.map = Some(&map);
                match event {
                    Event::CommitDiff(mut commit_diff) => {
                        info!("CommitDiff");
                        let mut line_count = 10; // double height of diff line
                        for file in &commit_diff.diff.files {
                            line_count += 1;
                            for hunk in &file.hunks {
                                line_count += 1 + hunk.lines.len();
                            }
                        }
                        // TODO! there are 3 lines on top of each commit
                        // + 2 spacer lines + 2 lines at bottom + COMMIT MESSAGE
                        // so its better to call all that after render
                        // to know lines amout in multilabel of COMMIT MESSAGE
                        debug!("line count-------------------------> {:?}", line_count);
                        map.adjust_height(line_count);
                        labels[1].content = format!(
                            "Author: <span color=\"{}\">{}</span>",
                            color, commit_diff.author
                        );
                        labels[2].content = format!(
                            "Date: <span color=\"{}\">{}</span>",
                            color, commit_diff.commit_dt
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
                            debug!(".................{:?}", d.diff.view.line_no);
                            if d.diff.expand(line_no, &mut ctx).is_some() {
                                let buffer = &txt.buffer();
                                let mut iter =
                                    buffer.iter_at_line(d.diff.view.line_no.get()).unwrap();
                                d.diff.render(buffer, &mut iter, &mut ctx);
                                let iter = buffer.iter_at_offset(buffer.cursor_position());
                                d.diff.cursor(buffer, iter.line(), true, &mut ctx);
                                txt.bind_highlights(&ctx);
                                if let Some(map) = ctx.map {
                                    map.bind_highlights(&ctx);
                                }
                                debug!(
                                    "CommitView............... end of render AFTER EXPAND {:?}",
                                    buffer.line_count()
                                );
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
                        if let Some(map) = ctx.map {
                            map.bind_highlights(&ctx);
                        }
                    }
                    Event::TextViewResize(w) => {
                        //info!("TextViewResize {} {:?}", w, ctx);
                    }
                    Event::TextCharVisibleWidth(w) => {
                        info!("TextCharVisibleWidth {}", w);
                        // is this still required?
                        if let Some(d) = &mut diff {
                            d.render(&txt, &mut ctx, &mut labels, body_label.as_mut().unwrap());
                        }
                    }
                    Event::Stage(_) | Event::RepoPopup => {
                        info!("Stage/Unstage or r pressed");
                        if let Some(diff) = &diff {
                            let title = if stash_num.is_some() {
                                "Apply stash"
                            } else {
                                match event {
                                    Event::Stage(_) => "Cherry pick",
                                    _ => "Revert",
                                }
                            };
                            let (body, file_path, hunk_header) = match cursor_position {
                                CursorPosition::CursorDiff(_) => (oid.to_string(), None, None),
                                CursorPosition::CursorFile(_, Some(file_idx)) => {
                                    let file = &diff.diff.files[file_idx];
                                    (
                                        format!("File: {}", file.path.to_str().unwrap()),
                                        Some(file.path.clone()),
                                        None,
                                    )
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
                                    (
                                            format!(
                                                "File: {}\nApplying single hunks is not yet implemented :(",
                                                file.path.to_str().unwrap()
                                            ),
                                            Some(file.path.clone()),
                                            Some(hunk.header.clone()),
                                        )
                                }
                                _ => ("".to_string(), None, None),
                            };
                            let mut cherry_pick_handled = false;
                            match event {
                                Event::Stage(_) => {
                                    if stash_num.is_none() {
                                        cherry_pick_handled = true;
                                        main_sender
                                            .send_blocking(crate::Event::CherryPick(
                                                oid,
                                                false,
                                                file_path.clone(),
                                                hunk_header.clone(),
                                            ))
                                            .expect("cant send through channel");
                                    }
                                }
                                _ => {
                                    cherry_pick_handled = true;
                                    main_sender
                                        .send_blocking(crate::Event::CherryPick(
                                            oid,
                                            true,
                                            file_path.clone(),
                                            hunk_header.clone(),
                                        ))
                                        .expect("cant send through channel");
                                }
                            }
                            // temporary untill revert is not going via main event loop
                            if !cherry_pick_handled {
                                let path = repo_path.clone();
                                let sender = main_sender.clone();
                                let window = window.clone();
                                glib::spawn_future_local({
                                    git_oid_op(
                                        ConfirmDialog(title.to_string(), body.to_string()),
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
                                                    Ok(())
                                                }
                                            }
                                            _ => Ok(()),
                                        },
                                    )
                                });
                            }
                        }
                    }
                    Event::Debug => {
                        info!("Debug");
                        debug!(
                            "map visible lines {:?} {:?}",
                            map.visible_start_line(),
                            map.visible_end_line()
                        );
                        let buffer = map.buffer();
                        let iter = buffer.iter_at_offset(0);
                        let pango_ctx = map.ltr_context();
                        let metrics = pango_ctx.metrics(None, None);
                        debug!(
                            "font_metrics!!!!!!!!! {:?} scaled {:.2} asc and desc {:?} {:?}",
                            metrics.height(),
                            metrics.height() as f32 / pango::SCALE as f32,
                            metrics.ascent(),
                            metrics.descent()
                        );
                        // if let Some(font_metrics) = metrics {
                        //     debug!("font_metrics!!!!!!!!! {:?}", font_metrics.height());
                        // }
                        debug!("line yrange {:?}", map.line_yrange(&iter).1);
                        if let Some(mut descr) = pango_ctx.font_description() {
                            debug!("oooooooooooooooooooo {:?}", descr.size());
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
