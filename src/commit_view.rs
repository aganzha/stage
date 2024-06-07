use crate::context::{StatusRenderContext, TextViewWidth};
use crate::dialogs::{alert, ConfirmDialog, YES};
use crate::git::{apply_stash, commit};
use crate::status_view::{
    container::{ViewContainer, ViewKind},
    render::View,
    textview::CharView,
    Label as TextViewLabel,
};
use crate::Event;
use async_channel::Sender;
use git2::Oid;
use std::cell::RefCell;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    gdk, gio, glib, Button, EventControllerKey, Label, ScrolledWindow,
    TextView, TextWindowType, Widget, Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{HeaderBar, ToolbarView, Window};
use log::{debug, info, trace};

use std::path::PathBuf;
use std::rc::Rc;

async fn git_oid_op<F>(
    oid: git2::Oid,
    window: impl IsA<Widget>,
    msg: &str,
    op: F,
) where
    F: FnOnce() -> Result<(), git2::Error> + Send + 'static,
{
    let response = alert(ConfirmDialog(msg.to_string(), format!("{}", oid)))
        .choose_future(&window)
        .await;
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
                    git_oid_op(oid, window, "Apply stash?", move || {
                        apply_stash(path, num, sender)
                    })
                });
            } else {
                glib::spawn_future_local({
                    git_oid_op(oid, window, "Cherry pick commit?", move || {
                        commit::cherry_pick(path, oid, sender)
                    })
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
                    git_oid_op(oid, window, "Revert commit?", move || {
                        commit::revert(path, oid, sender)
                    })
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
        // split first by new lines. each new line in commit must go
        // on its own, separate label. BUT!
        // also split long lines to different labels also!
        let user_split = content.split("\n");

        for line in user_split {
            let mut split = line.split(" ");
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
                                    &acc.replace("\n", ""),
                                ));
                                acc = String::from(word);
                            } else {
                                acc.push_str(word);
                                acc.push_str(" ");
                            }
                        } else {
                            trace!("words are over! push last label!");
                            self.labels.push(TextViewLabel::from_string(
                                &acc.replace("\n", ""),
                            ));
                            break 'words;
                        }
                    }
                    trace!("reach line end. push label!");
                    self.labels.push(TextViewLabel::from_string(
                        &acc.replace("\n", ""),
                    ));
                    acc = String::from("");
                }
            }
        }
        // space for following diff
        self.labels.push(TextViewLabel::from_string(""));
        self.view.expanded = true;
    }
}

impl ViewContainer for MultiLineLabel {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }
    fn child_count(&self) -> usize {
        self.labels.len()
    }
    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.labels
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }

    fn get_content(&self) -> String {
        "".to_string()
    }
}

impl commit::CommitDiff {
    fn render(
        &mut self,
        txt: &TextView,
        ctx: &mut StatusRenderContext,
        labels: &mut [TextViewLabel],
        obody_label: &mut Option<MultiLineLabel>,
    ) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);
        let labels_len = labels.len();
        for l in labels {
            l.render(&buffer, &mut iter, ctx)
        }
        let offset_before_erase = iter.offset();
        let mut body_label = {
            if obody_label.is_none() {
                MultiLineLabel::new(&self.message, ctx)
            } else {
                obody_label.take().unwrap()
            }
        };
        for l in &mut body_label.labels {
            l.erase(&buffer, ctx);
        }
        iter = buffer.iter_at_offset(offset_before_erase);
        // body_label.update_content(&self.message, ctx);
        body_label.render(&buffer, &mut iter, ctx);

        self.diff.render(&buffer, &mut iter, ctx);

        if !self.diff.files.is_empty() {
            let buffer = txt.buffer();
            let iter = buffer
                .iter_at_line(
                    (labels_len + body_label.labels.len() + 1) as i32,
                )
                .unwrap();
            buffer.place_cursor(&iter);
        }
        obody_label.replace(body_label);
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
    let txt = crate::textview_factory(
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
        let main_sender = main_sender.clone();
        let path = repo_path.clone();
        move |_, key, _, modifier| {
            match (key, modifier) {
                (gdk::Key::w, gdk::ModifierType::CONTROL_MASK) => {
                    window.close();
                }
                (gdk::Key::Escape, _) => {
                    window.close();
                }
                (gdk::Key::a, _) => {
                    let sender = main_sender.clone();
                    let path = path.clone();
                    let window = window.clone();
                    if let Some(num) = stash_num {
                        glib::spawn_future_local({
                            git_oid_op(
                                oid,
                                window,
                                "Apply stash?",
                                move || apply_stash(path, num, sender),
                            )
                        });
                    } else {
                        glib::spawn_future_local({
                            git_oid_op(
                                oid,
                                window,
                                "Cherry pick commit?",
                                move || commit::cherry_pick(path, oid, sender),
                            )
                        });
                    }
                }
                (gdk::Key::r, _) => {
                    if stash_num.is_none() {
                        let sender = main_sender.clone();
                        let path = path.clone();
                        let window = window.clone();
                        glib::spawn_future_local({
                            git_oid_op(
                                oid,
                                window,
                                "Revert commit?",
                                move || commit::revert(path, oid, sender),
                            )
                        });
                    }
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(event_controller);

    window.present();

    let mut main_diff: Option<commit::CommitDiff> = None;
    let mut body_label: Option<MultiLineLabel> = None;

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
            sender
                .send_blocking(Event::CommitDiff(commit_diff))
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

                    if !commit_diff.diff.files.is_empty() {
                        commit_diff.diff.files[0].view.current = true;
                    }
                    // let mut body_label = MultiLineLabel::new("", &mut ctx);
                    commit_diff.render(
                        &txt,
                        &mut ctx,
                        &mut labels,
                        &mut body_label,
                    );
                    main_diff.replace(commit_diff);
                }
                Event::Expand(_offset, line_no) => {
                    info!("Expand {}", line_no);
                    if let Some(d) = &mut main_diff {
                        let mut need_render = false;
                        for file in &mut d.diff.files {
                            need_render =
                                need_render || file.expand(line_no).is_some();
                            if need_render {
                                break;
                            }
                        }
                        let buffer = &txt.buffer();
                        let mut iter = buffer
                            .iter_at_line(d.diff.files[0].view.line_no)
                            .unwrap();
                        if need_render {
                            d.diff.render(buffer, &mut iter, &mut ctx);
                        }
                    }
                }
                Event::Cursor(_offset, line_no) => {
                    debug!("Cursor {}", line_no);
                    if let Some(d) = &mut main_diff {
                        if d.diff.cursor(line_no, false, &mut ctx) {
                            let buffer = &txt.buffer();
                            let mut iter = buffer
                                .iter_at_line(d.diff.files[0].view.line_no)
                                .unwrap();
                            // will render diff whithout rendering
                            // preceeding elements!
                            // is it ok? perhaps yes, cause they are on top of it
                            d.diff.render(buffer, &mut iter, &mut ctx);
                        }
                    }
                }
                Event::TextViewResize(w) => {
                    info!("TextViewResize {}", w);
                    ctx.screen_width.replace(text_view_width.clone());
                    if let Some(d) = &mut main_diff {
                        let buffer = &txt.buffer();
                        // during resize some views are build up
                        // and cursor could move
                        let cursor_before = buffer.cursor_position();
                        // calling diff resize will render it whithout rendering
                        // preceeding elements!
                        // is it ok? perhaps yes, cause they are on top of it
                        d.diff.resize(buffer, &mut ctx);
                        // restore it. DO IT NEEDED????
                        // TODO! perhaps move it to common render method???
                        buffer.place_cursor(
                            &buffer.iter_at_offset(cursor_before),
                        );
                    }
                }
                Event::TextCharVisibleWidth(w) => {
                    info!("TextCharVisibleWidth {}", w);
                    ctx.screen_width.replace(text_view_width.clone());
                    if let Some(d) = &mut main_diff {
                        d.render(&txt, &mut ctx, &mut labels, &mut body_label);
                    }
                }
                _ => {
                    debug!("unhandled event in commit_view {:?}", event);
                }
            }
        }
    });
}
