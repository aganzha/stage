// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod commit;
pub mod context;
pub mod headerbar;
pub mod monitor;
pub mod render;
pub mod stage;
pub mod stage_view;
pub mod tags;

use crate::dialogs::{alert, DangerDialog, YES};
use crate::git::{
    abort_rebase, branch::BranchData, continue_rebase, get_head, merge,
    remote, stash, HunkLineNo,
};

use core::time::Duration;
use git2::RepositoryState;
use render::ViewContainer; // MayBeViewContainer o
use stage_view::{cursor_to_line_offset, StageView};

pub mod reconciliation;
pub mod tests;
pub mod view;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::status_view::view::View;
use crate::{
    get_current_repo_status, track_changes, Diff, DiffKind, Event,
    File as GitFile, Head, StageOp, State, StatusRenderContext, DARK_CLASS,
    LIGHT_CLASS,
};
use async_channel::Sender;

use gio::FileMonitor;

use crate::status_view::context::CursorPosition as ContextCursorPosition;
use glib::clone;
use glib::signal::SignalHandlerId;
use gtk4::prelude::*;
use gtk4::{
    gio, glib, Align, Button, FileDialog, ListBox, SelectionMode, TextBuffer,
    TextIter, Widget, Window as GTKWindow,
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, Banner, ButtonContent, EntryRow, PasswordEntryRow,
    StatusPage, StyleManager, SwitchRow,
};
use log::{debug, trace};

impl State {
    pub fn title_for_proceed_banner(&self) -> String {
        match self.state {
            RepositoryState::Merge => format!("All conflicts fixed but you are\
                                               still merging. Commit to conclude merge branch {}", self.subject),
            RepositoryState::CherryPick => format!("Commit to finish cherry-pick {}", self.subject),
            RepositoryState::Revert => format!("Commit to finish revert {}", self.subject),
            _ => "".to_string()
        }
    }
    pub fn title_for_conflict_banner(&self) -> String {
        let start = "Got conflicts while";
        match self.state {
            RepositoryState::Merge => {
                format!("{} merging branch {}", start, self.subject)
            }
            RepositoryState::CherryPick => {
                format!("{} cherry picking {}", start, self.subject)
            }
            _ => "".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Label {
    pub content: String,
    view: View,
}
impl Label {
    pub fn from_string(content: &str) -> Self {
        Label {
            content: String::from(content),
            view: View::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RenderSource {
    Git,
    GitDiff,
    Expand(i32),
}

pub const DUMP_DIR: &str = "stage_dump";

#[derive(Debug, Clone)]
pub struct LastOp {
    op: StageOp,
    file_path: Option<PathBuf>,
    hunk_header: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CursorPosition {
    CursorDiff(DiffKind),
    CursorFile(DiffKind, usize),
    CursorHunk(DiffKind, usize, usize),
    CursorLine(DiffKind, usize, usize, usize),
    None,
}

impl CursorPosition {
    pub fn from_context(context: &StatusRenderContext) -> Self {
        match context.cursor_position {
            ContextCursorPosition::CursorDiff(diff) => {
                return CursorPosition::CursorDiff(diff.kind);
            }
            ContextCursorPosition::CursorFile(f) => {
                let diff = context.selected_diff.unwrap();
                let file = context.selected_file.unwrap();
                assert!(std::ptr::eq(file, f));
                return CursorPosition::CursorFile(
                    diff.kind,
                    diff.files
                        .iter()
                        .position(|f| std::ptr::eq(file, f))
                        .unwrap(),
                );
            }
            ContextCursorPosition::CursorHunk(h) => {
                let diff = context.selected_diff.unwrap();
                let file = context.selected_file.unwrap();
                let hunk = context.selected_hunk.unwrap();
                assert!(std::ptr::eq(hunk, h));
                return CursorPosition::CursorHunk(
                    diff.kind,
                    diff.files
                        .iter()
                        .position(|f| std::ptr::eq(file, f))
                        .unwrap(),
                    file.hunks
                        .iter()
                        .position(|h| std::ptr::eq(hunk, h))
                        .unwrap(),
                );
            }
            ContextCursorPosition::CursorLine(line) => {
                let diff = context.selected_diff.unwrap();
                let file = context.selected_file.unwrap();
                let hunk = context.selected_hunk.unwrap();
                return CursorPosition::CursorLine(
                    diff.kind,
                    diff.files
                        .iter()
                        .position(|f| std::ptr::eq(file, f))
                        .unwrap(),
                    file.hunks
                        .iter()
                        .position(|h| std::ptr::eq(hunk, h))
                        .unwrap(),
                    hunk.lines
                        .iter()
                        .position(|l| std::ptr::eq(line, l))
                        .unwrap(),
                );
            }
            _ => {}
        }
        CursorPosition::None
    }
}

#[derive(Debug, Clone)]
pub struct Status {
    pub path: Option<PathBuf>,
    pub sender: Sender<Event>,
    pub head: Option<Head>,
    pub upstream: Option<Head>,
    pub state: Option<State>,

    pub untracked: Option<Diff>,
    pub staged: Option<Diff>,
    pub unstaged: Option<Diff>,
    pub conflicted: Option<Diff>,

    pub stashes: Option<stash::Stashes>,
    pub branches: Option<Vec<BranchData>>,

    pub monitor_global_lock: Rc<RefCell<bool>>,
    pub monitor_lock: Rc<RefCell<HashSet<PathBuf>>>,
    //pub settings: gio::Settings,
    pub last_op: Option<LastOp>,
    pub cursor_position: Cell<CursorPosition>,
}

impl Status {
    pub fn new(
        path: Option<PathBuf>,
        // settings: gio::Settings,
        sender: Sender<Event>,
    ) -> Self {
        Self {
            path,
            sender,
            head: None,
            upstream: None,
            state: None,

            untracked: None,
            staged: None,
            unstaged: None,
            conflicted: None,

            stashes: None,
            branches: None,
            monitor_global_lock: Rc::new(RefCell::new(false)),
            monitor_lock: Rc::new(RefCell::new(HashSet::new())),
            //settings,
            last_op: None,
            cursor_position: Cell::new(CursorPosition::None),
        }
    }

    pub fn file_at_cursor(&self) -> Option<&GitFile> {
        for diff in [&self.staged, &self.unstaged, &self.conflicted]
            .into_iter()
            .flatten()
        {
            let maybe_file = diff.files.iter().find(|f| {
                f.view.is_current()
                    || f.hunks.iter().any(|h| h.view.is_active())
            });
            if maybe_file.is_some() {
                return maybe_file;
            }
        }
        None
    }

    pub fn editor_args_at_cursor(
        &self,
        txt: &StageView,
    ) -> Option<(PathBuf, i32, i32)> {
        if let Some(file) = self.file_at_cursor() {
            if file.view.is_current() {
                return Some((self.to_abs_path(&file.path), 0, 0));
            }
            let hunk = file.hunks.iter().find(|h| h.view.is_active()).unwrap();
            // TODO move Line old_line_no and new_line_no
            let mut line_no = hunk.new_start;
            let mut col_no = 0;
            if !hunk.view.is_current() {
                let line =
                    hunk.lines.iter().find(|l| l.view.is_current()).unwrap();
                line_no = line
                    .new_line_no
                    .or(line.old_line_no)
                    .unwrap_or(HunkLineNo::new(0));
                let pos = txt.buffer().cursor_position();
                let iter = txt.buffer().iter_at_offset(pos);
                col_no = iter.line_offset();
            }
            let mut base = self.path.clone().unwrap();
            base.pop();
            base.push(&file.path);
            return Some((base, line_no.as_i32(), col_no));
        }
        None
    }

    pub fn to_abs_path(&self, path: &Path) -> PathBuf {
        let mut base = self.path.clone().unwrap();
        base.pop();
        base.push(path);
        base
    }

    pub fn head_name(&self) -> String {
        if let Some(head) = &self.head {
            if let Some(branch_name) = &head.branch_name {
                return branch_name.to_string();
            }
        }
        "Detached head".to_string()
    }

    pub fn update_path(
        &mut self,
        path: PathBuf,
        monitors: Rc<RefCell<Vec<FileMonitor>>>,
        user_action: bool,
        settings: &gio::Settings,
    ) {
        // here could come path selected by the user
        // this is 'dirty' one. The right path will
        // came from git with /.git/ suffix
        // but the 'dirty' path will be used first
        // for querying repo status and investigate real one
        if user_action {
            self.stashes.take();
            self.branches.take();

            monitors.borrow_mut().retain(|fm: &FileMonitor| {
                fm.cancel();
                false
            });
        } else {
            // investigated path
            assert!(path.ends_with(".git/"));
            if self.path.is_none() || path != self.path.clone().unwrap() {
                let mut paths = settings.get::<Vec<String>>("paths");
                let str_path =
                    String::from(path.to_str().unwrap()).replace(".git/", "");
                settings
                    .set("lastpath", str_path.clone())
                    .expect("cant set lastpath");
                if !paths.contains(&str_path) {
                    paths.push(str_path.clone());
                    settings.set("paths", paths).expect("cant set settings");
                }
                self.setup_monitors(monitors, PathBuf::from(str_path));
            }
        }
        self.path.replace(path.clone());
    }

    pub fn update_stashes(&mut self, stashes: stash::Stashes) {
        self.stashes.replace(stashes);
    }
    pub fn update_branches(&mut self, branches: Vec<BranchData>) {
        self.branches.replace(branches);
    }

    pub fn reset_hard(
        &self,
        _ooid: Option<crate::Oid>,
        window: &impl IsA<Widget>,
    ) {
        glib::spawn_future_local({
            let sender = self.sender.clone();
            let path = self.path.clone().unwrap();
            let window = window.clone();
            async move {
                let response = alert(DangerDialog(
                    String::from("Reset"),
                    String::from("Hard reset to Head"),
                ))
                .choose_future(&window)
                .await;
                if response != YES {
                    return;
                }
                gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || crate::reset_hard(path, None, sender)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(false)
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&window);
                    false
                });
            }
        });
    }

    pub fn get_status(&self) {
        gio::spawn_blocking({
            let path = self.path.clone();
            let sender = self.sender.clone();
            move || {
                get_current_repo_status(path, sender)
                    .expect("cant get status");
            }
        });
    }

    pub fn get_empty_view(&self) -> impl IsA<Widget> {
        let button_content = ButtonContent::builder()
            .icon_name("document-open-symbolic")
            .label("Open")
            .use_underline(true)
            .valign(Align::Baseline)
            .build();
        let button = Button::builder()
            .child(&button_content)
            .halign(Align::Center)
            .has_frame(true)
            .css_classes(vec!["suggested-action", "pill"])
            .hexpand(false)
            .build();
        button.connect_clicked({
            let sender = self.sender.clone();
            move |_| {
                let dialog = FileDialog::new();
                dialog.select_folder(
                    None::<&GTKWindow>,
                    None::<&gio::Cancellable>,
                    {
                        let sender = sender.clone();
                        move |result| {
                            if let Ok(file) = result {
                                if let Some(path) = file.path() {
                                    sender
                                        .send_blocking(crate::Event::OpenRepo(
                                            path,
                                        ))
                                        .expect(
                                            "Could not send through channel",
                                        );
                                }
                            }
                        }
                    },
                );
            }
        });
        StatusPage::builder()
            .icon_name("com.github.aganzha.stage") //document-open-symbolic
            .title("Open repository")
            .child(&button)
            .build()
        // let bx = Box::builder()
        //     .hexpand(true)
        //     .vexpand(true)
        //     .vexpand_set(true)
        //     .hexpand_set(true)
        //     .valign(Align::Center)
        //     .orientation(Orientation::Vertical)
        //     .build();
        // let image = Image::builder().icon_name("document-open-symbolic").build();
        // bx.append(&image);
        // bx.append(&GTKLabel::new(Some("Open repository")));
        // bx
    }

    pub fn pull(&self, window: &ApplicationWindow, ask_pass: Option<bool>) {
        glib::spawn_future_local({
            let path = self.path.clone().expect("no path");
            let sender = self.sender.clone();
            let window = window.clone();
            async move {
                let mut user_pass: Option<(String, String)> = None;
                if let Some(ask) = ask_pass {
                    if ask {
                        let lb = ListBox::builder()
                            .selection_mode(SelectionMode::None)
                            .css_classes(vec![String::from("boxed-list")])
                            .build();

                        let user_name = EntryRow::builder()
                            .title("User name:")
                            .show_apply_button(true)
                            .css_classes(vec!["input_field"])
                            .build();
                        let password = PasswordEntryRow::builder()
                            .title("Password:")
                            .css_classes(vec!["input_field"])
                            .build();
                        let dialog = crate::confirm_dialog_factory(
                            &window,
                            Some(&lb),
                            "Pull from remote/origin", // TODO here is harcode
                            "Pull",
                        );
                        let response = dialog.choose_future().await;
                        if "confirm" != response {
                            return;
                        }
                        user_pass.replace((
                            format!("{}", user_name.text()),
                            format!("{}", password.text()),
                        ));
                    }
                }
                gio::spawn_blocking({
                    move || remote::pull(path, sender, user_pass)
                })
                .await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e)).present(&window);
                    Ok(())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&window);
                });
            }
        });
    }

    pub fn push(
        &self,
        window: &ApplicationWindow,
        remote_dialog: Option<(String, bool, bool)>,
    ) {
        let remote = self.choose_remote();
        glib::spawn_future_local({
            let window = window.clone();
            let path = self.path.clone();
            let sender = self.sender.clone();
            async move {
                let lb = ListBox::builder()
                    .selection_mode(SelectionMode::None)
                    .css_classes(vec![String::from("boxed-list")])
                    .build();
                let upstream = SwitchRow::builder()
                    .title("Set upstream")
                    .css_classes(vec!["input_field"])
                    .active(true)
                    .build();

                let input = EntryRow::builder()
                    .title("Remote branch name:")
                    .show_apply_button(true)
                    .css_classes(vec!["input_field"])
                    .text(remote)
                    .build();

                let user_name = EntryRow::builder()
                    .title("User name:")
                    .show_apply_button(true)
                    .css_classes(vec!["input_field"])
                    .build();
                let password = PasswordEntryRow::builder()
                    .title("Password:")
                    .css_classes(vec!["input_field"])
                    .build();
                let dialog = crate::confirm_dialog_factory(
                    &window,
                    Some(&lb),
                    "Push to remote/origin", // TODO here is harcode
                    "Push",
                );

                input.connect_apply(
                    clone!(@strong dialog as dialog => move |_| {
                        // someone pressed enter
                        dialog.response("confirm");
                        dialog.close();
                    }),
                );
                input.connect_entry_activated(
                    clone!(@strong dialog as dialog => move |_| {
                        // someone pressed enter
                        dialog.response("confirm");
                        dialog.close();
                    }),
                );
                let mut pass = false;
                match remote_dialog {
                    None => {
                        lb.append(&input);
                        lb.append(&upstream);
                    }
                    Some((remote_branch, track_remote, ask_password))
                        if ask_password =>
                    {
                        input.set_text(&remote_branch);
                        if track_remote {
                            upstream.set_active(true);
                        }
                        lb.append(&user_name);
                        lb.append(&password);
                        pass = true;
                    }
                    _ => {
                        panic!("unknown case");
                    }
                }

                let response = dialog.choose_future().await;
                if "confirm" != response {
                    return;
                }
                let remote_branch_name = format!("{}", input.text());
                let track_remote = upstream.is_active();
                let mut user_pass: Option<(String, String)> = None;
                if pass {
                    user_pass.replace((
                        format!("{}", user_name.text()),
                        format!("{}", password.text()),
                    ));
                }
                glib::spawn_future_local({
                    async move {
                        gio::spawn_blocking({
                            move || {
                                remote::push(
                                    path.expect("no path"),
                                    remote_branch_name,
                                    track_remote,
                                    sender,
                                    user_pass,
                                )
                            }
                        })
                        .await
                        .unwrap_or_else(|e| {
                            alert(format!("{:?}", e)).present(&window);
                            Ok(())
                        })
                        .unwrap_or_else(|e| {
                            alert(e).present(&window);
                        });
                    }
                });
            }
        });
    }

    pub fn choose_remote(&self) -> String {
        if let Some(upstream) = &self.upstream {
            if let Some(branch_name) = &upstream.branch_name {
                return branch_name.local_name();
            }
        }
        if let Some(head) = &self.head {
            if let Some(branch_name) = &head.branch_name {
                return branch_name.to_string();
            }
        }
        "".to_string()
    }

    pub fn commit(
        &self,
        window: &ApplicationWindow, // &impl IsA<Gtk4Window>,
    ) {
        let mut amend_message: Option<String> = None;
        if let Some(head) = &self.head {
            if let Some(upstream) = &self.upstream {
                if head.oid != upstream.oid {
                    amend_message.replace(head.raw_message.clone());
                }
            } else {
                amend_message.replace(head.raw_message.clone());
            }
        }
        commit::commit(
            self.path.clone(),
            amend_message,
            window,
            self.sender.clone(),
        );
    }

    pub fn update_head<'a>(
        &'a mut self,
        mut head: Head,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        if let Some(current_head) = &self.head {
            head.enrich_view(current_head, &txt.buffer(), context);
        }
        if let Some(branches) = &mut self.branches {
            if let Some(head_branch) = head.branch.take() {
                if let Some(ind) = branches.iter().position(|b| b.is_head) {
                    trace!(
                        "replace branch by index {:?} {:?}",
                        ind,
                        head_branch.name
                    );
                    branches[ind] = head_branch;
                }
            }
        }
        self.head.replace(head);
        self.render(txt, None, context);
    }

    pub fn update_upstream<'a>(
        &'a mut self,
        mut upstream: Option<Head>,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        if let Some(rendered) = &mut self.upstream {
            if let Some(new) = &upstream {
                new.enrich_view(rendered, &txt.buffer(), context);
            } else {
                rendered.erase(&txt.buffer(), context);
            }
        }
        if let Some(branches) = &mut self.branches {
            if let Some(upstream) = &mut upstream {
                if let Some(upstream_branch) = upstream.branch.take() {
                    if let Some(ind) = branches.iter().position(|b| {
                        b.name == upstream_branch.name
                            && b.branch_type == upstream_branch.branch_type
                    }) {
                        trace!(
                            "replace branch by index {:?} {:?}",
                            ind,
                            upstream_branch.name
                        );
                        branches[ind] = upstream_branch;
                    }
                }
            }
        }
        self.upstream = upstream;
        self.render(txt, None, context);
    }

    pub fn update_state<'a>(
        &'a mut self,
        state: State,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        if let Some(current_state) = &self.state {
            state.enrich_view(current_state, &txt.buffer(), context)
        }
        self.state.replace(state);
        self.render(txt, None, context);
    }

    pub fn update_untracked<'a>(
        &'a mut self,
        mut untracked: Option<Diff>,
        txt: &StageView,
        gio_settings: &gio::Settings,
        context: &mut StatusRenderContext<'a>,
    ) {
        let mut settings =
            gio_settings.get::<HashMap<String, Vec<String>>>("ignored");

        let repo_path = self.path.clone().unwrap();
        let str_path = repo_path.to_str().unwrap();
        let mut has_files = true;
        if let Some(ignored) = settings.get_mut(str_path) {
            if let Some(new) = &mut untracked {
                new.files.retain(|f| {
                    let str_path = f.path.to_str().unwrap();
                    !ignored.contains(&str_path.to_string())
                });
                has_files = !new.files.is_empty();
            }
        }
        if !has_files {
            untracked = None;
        }
        debug!("update untracked! {:?}", untracked.is_some());
        let mut render_required = false;
        if let Some(rendered) = &mut self.untracked {
            render_required = true;
            let buffer = &txt.buffer();
            if let Some(new) = &untracked {
                new.enrich_view(rendered, buffer, context);
            } else {
                rendered.erase(buffer, context);
            }
        }
        self.untracked = untracked;
        if self.untracked.is_some() || render_required {
            self.render(txt, None, context);
        }
    }

    pub fn track_changes(&self, file_path: PathBuf, sender: Sender<Event>) {
        gio::spawn_blocking({
            let path = self.path.clone().unwrap();
            let sender = sender.clone();
            let mut interhunk = None;
            let mut has_conflicted = false;
            if let Some(diff) = &self.conflicted {
                if let Some(stored_interhunk) = diff.interhunk {
                    interhunk.replace(stored_interhunk);
                }
                for file in &diff.files {
                    if file.path == file_path {
                        has_conflicted = true;
                    }
                }
            }
            move || {
                track_changes(
                    path,
                    file_path,
                    interhunk,
                    has_conflicted,
                    sender,
                )
            }
        });
    }

    pub fn update_conflicted<'a>(
        &'a mut self,
        diff: Option<Diff>,
        state: Option<State>,
        txt: &StageView,
        window: &ApplicationWindow,
        sender: Sender<Event>,
        banner: &Banner,
        banner_button: &Widget,
        banner_button_clicked: Rc<RefCell<Option<SignalHandlerId>>>,
        context: &mut StatusRenderContext<'a>,
    ) {
        if let Some(state) = state {
            if let Some(current_state) = &self.state {
                state.enrich_view(current_state, &txt.buffer(), context)
            }
            self.state.replace(state);
        }

        if let Some(rendered) = &mut self.conflicted {
            let buffer = &txt.buffer();
            if let Some(new) = &diff {
                new.enrich_view(rendered, buffer, context);
            } else {
                rendered.erase(buffer, context);
            }
        }
        // banner is separate thing. perhaps assign method below to banner?
        if let Some(state) = &self.state {
            if diff.is_none() {
                if banner.is_revealed() {
                    banner.set_revealed(false);
                }
                if state.need_final_commit() || state.need_rebase_continue() {
                    banner.set_title(&state.title_for_proceed_banner());
                    banner.set_css_classes(
                        if StyleManager::default().is_dark() {
                            &[DARK_CLASS, "success"]
                        } else {
                            &[LIGHT_CLASS, "success"]
                        },
                    );
                    banner.set_button_label(if state.need_final_commit() {
                        Some("Commit")
                    } else {
                        Some("Continue")
                    });
                    banner_button.set_css_classes(&["suggested-action"]);
                    banner.set_revealed(true);
                    if let Some(handler_id) = banner_button_clicked.take() {
                        banner.disconnect(handler_id);
                    }
                    let new_handler_id = banner.connect_button_clicked({
                        let sender = sender.clone();
                        let path = self.path.clone();
                        let window = window.clone();
                        let banner = banner.clone();
                        let state = state.state;
                        move |_| {
                            let sender = sender.clone();
                            let path = path.clone();
                            let window = window.clone();
                            banner.set_revealed(false);
                            glib::spawn_future_local({
                                async move {
                                    gio::spawn_blocking({
                                        move || match state {
                                            RepositoryState::Merge => {
                                                merge::final_merge_commit(
                                                    path.clone().unwrap(),
                                                    sender,
                                                )
                                            }
                                            RepositoryState::RebaseMerge => {
                                                continue_rebase(
                                                    path.clone().unwrap(),
                                                    sender,
                                                )
                                            }
                                            _ => merge::final_commit(
                                                path.clone().unwrap(),
                                                sender,
                                            ),
                                        }
                                    })
                                    .await
                                    .unwrap_or_else(|e| {
                                        alert(format!("{:?}", e))
                                            .present(&window);
                                        Ok(())
                                    })
                                    .unwrap_or_else(|e| {
                                        alert(e).present(&window);
                                    });
                                }
                            });
                        }
                    });
                    banner_button_clicked.replace(Some(new_handler_id));
                }
            } else if !banner.is_revealed() {
                banner.set_title(&state.title_for_conflict_banner());
                banner.set_css_classes(if StyleManager::default().is_dark() {
                    &[DARK_CLASS, "error"]
                } else {
                    &[LIGHT_CLASS, "error"]
                });
                banner.set_button_label(Some("Abort"));
                banner_button.set_css_classes(&["destructive-action"]);
                banner.set_revealed(true);
                if let Some(handler_id) = banner_button_clicked.take() {
                    banner.disconnect(handler_id);
                }
                let new_handler_id = banner.connect_button_clicked({
                    let sender = sender.clone();
                    let path = self.path.clone();
                    let state = self.state.clone().unwrap().state;
                    let banner = banner.clone();
                    move |_| {
                        banner.set_revealed(false);
                        gio::spawn_blocking({
                            let sender = sender.clone();
                            let path = path.clone();
                            move || match state {
                                RepositoryState::RebaseMerge => abort_rebase(
                                    path.expect("no path"),
                                    sender,
                                ),
                                _ => merge::abort(
                                    path.expect("no path"),
                                    sender,
                                ),
                            }
                        });
                    }
                });
                banner_button_clicked.replace(Some(new_handler_id));
            }
        }
        self.conflicted = diff;
        self.render(txt, None, context);
    }

    pub fn update_staged<'a>(
        &'a mut self,
        diff: Option<Diff>,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        let mut render_required = false;
        if let Some(rendered) = &mut self.staged {
            render_required = true;
            let buffer = &txt.buffer();
            if let Some(new) = &diff {
                new.enrich_view(rendered, buffer, context);
            } else {
                rendered.erase(buffer, context);
            }
        }
        self.staged = diff;
        if self.staged.is_some() || render_required {
            self.render(txt, None, context);
        }
    }

    pub fn update_unstaged<'a>(
        &'a mut self,
        diff: Option<Diff>,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        let _buffer = &txt.buffer();

        let mut render_required = false;
        if let Some(rendered) = &mut self.unstaged {
            render_required = true;
            let buffer = &txt.buffer();
            if let Some(new) = &diff {
                new.enrich_view(rendered, buffer, context);
            } else {
                rendered.erase(buffer, context);
            }
        }

        self.unstaged = diff;

        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~cleanup StageOp here!
        let mut op: Option<LastOp> = None;
        if self.unstaged.is_some() {
            if let Some(last_op) = &self.last_op {
                if let StageOp::Stage(_) = last_op.op {
                    op = self.last_op.take();
                    // op.replace(stage);
                }
            }
        }
        // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~cleanup StageOp here!

        if self.unstaged.is_some() || render_required {
            self.render(txt, op, context);
        }
        // if self.unstaged.is_some() {
        // }
    }

    pub fn update_tracked_file<'a>(
        &'a mut self,
        file_path: PathBuf,
        diff: Diff,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        // this method is called only if there is something to
        // update in unstaged/conflicted and they will remain after!
        // if tracked file is returning to original state
        // and it must be removed from unstaged/conflicted and this is
        // ONLY file in unstaged/conflicted, then another event will raise and diff
        // will be removed completelly
        let mine = if diff.kind == DiffKind::Conflicted {
            &mut self.conflicted
        } else {
            &mut self.unstaged
        };
        if let Some(rendered) = mine {
            // so. it need to find file in rendered.
            // if it is there - enrich new one by it and replace
            // if it is not there - insert
            // if it is there and new is empty - erase it

            let updated_file =
                diff.files.into_iter().find(|f| f.path == file_path);
            let buffer = &txt.buffer();
            let mut ind = 0;
            let mut insert_ind: Option<usize> = None;
            debug!(
                "track 1 file. rendered files are {:}",
                &rendered.files.len()
            );
            rendered.files.retain_mut(|f| {
                ind += 1;
                if f.path == file_path {
                    insert_ind = Some(ind);
                    if let Some(file) = &updated_file {
                        file.enrich_view(f, buffer, context);
                    } else {
                        debug!("ERASE rendered file!!!!!!!!!");
                        f.erase(buffer, context);
                    }
                    false
                } else {
                    true
                }
            });

            if let Some(file) = updated_file {
                if let Some(ind) = insert_ind {
                    rendered.files.insert(ind - 1, file);
                } else {
                    // insert alphabetically
                    let mut ind = 0;
                    for rendered_file in &rendered.files {
                        debug!("________compare files while insert alphabetically {:?} {:?} {:?}", file.path, rendered_file.path, file.path < rendered_file.path);
                        if file.path < rendered_file.path {
                            break;
                        }
                        ind += 1
                    }
                    rendered.files.insert(ind, file);
                }
                debug!("just inserted new file...........");
            }
        } else if diff.kind == DiffKind::Conflicted {
            self.conflicted = Some(diff);
        } else {
            self.unstaged = Some(diff);
        }
        self.render(txt, None, context);
    }

    pub fn resize_highlights<'a>(
        &'a mut self,
        txt: &StageView,
        ctx: &mut StatusRenderContext<'a>,
    ) {
        let buffer = txt.buffer();
        let iter = buffer.iter_at_offset(buffer.cursor_position());
        self.cursor(txt, iter.line(), iter.offset(), ctx);
        glib::source::timeout_add_local(Duration::from_millis(10), {
            let txt = txt.clone();
            let mut context = StatusRenderContext::new();
            context.highlight_lines = ctx.highlight_lines;
            context.highlight_hunks.clone_from(&ctx.highlight_hunks);
            move || {
                txt.bind_highlights(&context);
                glib::ControlFlow::Break
            }
        });
    }

    /// cursor does not change structure, but changes highlights
    /// it will collect highlights in context. no need further render
    pub fn cursor<'a>(
        &'a self,
        txt: &StageView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext<'a>,
    ) -> bool {
        // this is actually needed for views which are not implemented
        // ViewContainer, and does not affect context!
        // do i still have such views????

        // thought: so, diff also implements
        // view container, so yes. no need to do that
        // (will be done in view container, BUT!)
        // why it is even possible to do this here???
        // it must be hidden in such a place, where ONLY
        // ViewContainer is needed to setup cursor!
        // context must receive ViewContainer as
        // argument and use its line_no to store cursor!
        // it is used only once in resize_highlights for copy!
        // self.cursor_position.replace(Rc::new(context.cursor_position.clone()));
        let mut changed = false;
        let buffer = txt.buffer();
        if let Some(untracked) = &self.untracked {
            changed =
                untracked.cursor(&buffer, line_no, false, context) || changed;
        }
        if let Some(conflicted) = &self.conflicted {
            changed =
                conflicted.cursor(&buffer, line_no, false, context) || changed;
        }
        if let Some(unstaged) = &self.unstaged {
            changed =
                unstaged.cursor(&buffer, line_no, false, context) || changed;
        }
        if let Some(staged) = &self.staged {
            changed =
                staged.cursor(&buffer, line_no, false, context) || changed;
        }

        // this is called once in status_view and 3 times in commit view!!!
        txt.bind_highlights(context);
        self.cursor_position
            .replace(CursorPosition::from_context(context));
        changed
    }

    // Status
    pub fn expand<'a>(
        &'a mut self,
        txt: &StageView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext<'a>,
    ) {
        if let Some(conflicted) = &self.conflicted {
            if conflicted.expand(line_no, context).is_some() {
                self.render(txt, None, context);
                return;
            }
        }

        if let Some(unstaged) = &self.unstaged {
            if unstaged.expand(line_no, context).is_some() {
                self.render(txt, None, context);
                return;
            }
        }
        if let Some(staged) = &self.staged {
            if staged.expand(line_no, context).is_some() {
                self.render(txt, None, context);
            }
        }
    }

    pub fn render<'a>(
        &'a self,
        txt: &StageView,
        last_op: Option<LastOp>,
        context: &mut StatusRenderContext<'a>,
    ) {
        let buffer = txt.buffer();
        let initial_line_offset = buffer
            .iter_at_offset(buffer.cursor_position())
            .line_offset();

        let mut iter = buffer.iter_at_offset(0);

        if let Some(head) = &self.head {
            head.render(&buffer, &mut iter, context);
        }

        if let Some(upstream) = &self.upstream {
            upstream.render(&buffer, &mut iter, context);
        }

        if let Some(state) = &self.state {
            state.render(&buffer, &mut iter, context);
        }

        if let Some(untracked) = &self.untracked {
            untracked.render(&buffer, &mut iter, context);
        }

        if let Some(conflicted) = &self.conflicted {
            conflicted.render(&buffer, &mut iter, context);
        }

        if let Some(unstaged) = &self.unstaged {
            unstaged.render(&buffer, &mut iter, context);
        }

        if let Some(staged) = &self.staged {
            staged.render(&buffer, &mut iter, context);
        }

        // first place is here
        cursor_to_line_offset(&txt.buffer(), initial_line_offset);

        let iter = self.choose_cursor_position(&buffer, last_op);
        trace!("__________ chused position {:?}", iter.line());
        buffer.place_cursor(&iter);
        // hey. here is a cursor kust in the end of render.
        // do i need after_render at all?
        // only diff is using after render, cause it sets its own tags.
        // file is only using it for max_width (no longer used).

        // so, the pyramid:
        // expand->render->cursor. cursor is last thing called.
        self.cursor(txt, iter.line(), iter.offset(), context);
    }

    pub fn choose_cursor_position(
        //
        &self,
        buffer: &TextBuffer,
        last_op: Option<LastOp>, // context: &mut StatusRenderContext<'a>,
    ) -> TextIter {
        trace!(
            "...................choose cursor position {:?}",
            self.last_op
        );
        let this_pos = buffer.cursor_position();
        let mut iter = buffer.iter_at_offset(this_pos);
        if let Some(last_op) = &last_op {
            if let StageOp::Stage(_line_no) = last_op.op {
                // if i am still in staging diff - be here.
                // if let Some(diff) = self.// fuck, i need whole diff by
                // cursor_line!
                // what can i do? store it in the context...
                // for current line store its current diff!
                // how diff could be active! i;ve using it
                // for highlight! it need to set active diff
                // from file or hunk then!

                // FUUUUUUUUUUUUUUUUUUUUUUUUUCK
                // there is no active diff, cause cursor is
                // called AFTER this function!
                // and previous active diffs are cleanud up
                // cause context is new every time!
                // i must not relate to active links here!
                // those are only for stage_via_apply!
                // so. what can i get here.
                // i have self.views rendered!
                // i can use everything EXCEPT active_.. fields
                // FUUUUUUUUUUUUUUUUUUUUUUUUUCK
                if let Some(unstaged) = &self.unstaged {
                    if let Some(line_to_go) =
                        unstaged.nearest_line_to_go(iter.line())
                    {
                        debug!("i am missied in unstaged, but have line to go!!!!!! {line_to_go}");
                        debug!("here it need to cleanup op to stop smart choosing line!");
                        iter.set_line(line_to_go);
                    } else {
                        debug!("i am either in unstaged, or there are no place to go in unstaged!");
                        debug!("how do i know, that it need to clean the op?");
                        debug!("it need to clean the op in operation itself! after the render!!!!!");
                    }
                    // // this works! but lets just return last nearest line!
                    // if !unstaged.has_view_on(iter.line()) {
                    //     // i am still
                    //     debug!("i have no view in unstaged !!!!!!!!!!!");
                    //     debug!(" but unstaged is alive! it must be upper!");

                    //     // let (nearest_top, nearest_bottom) = unstaged.nearest(iter.line(), context);
                    //     // debug!("????????????????? nearest_top {:?} nearest_bottom {:?}", nearest_top, nearest_bottom);
                    //     // if let Some(nearest_bottom) = nearest_bottom {
                    //     //     iter.set_line(nearest_bottom);
                    //     // } else {
                    //     //     iter.set_line(nearest_top.unwrap());
                    //     // }
                    // } else {
                    //     debug!("hey! i am still here in unstaged!")
                    // }
                } else {
                    debug!("i have no unstaged. lets may be go to staged then? {:?}", self.staged.is_some());
                    debug!("how do i know, that it need to clean the op?");
                    debug!("it need to clean the op in operation itself! after the render!!!!!");
                }

                // if let Some(diff) = context.active_diff {
                //     if diff.kind == DiffKind::Unstaged {
                //         debug!(
                //             "~~~~~~~~~~~~~~~~~~~~~~~~~~~~i am still in unstaged after staging. all ok"
                //         );
                //         return iter;
                //     }
                // } else {
                //     debug!("ooooooooops. i am nowhere after staging");
                //     if let Some(diff) = &self.unstaged {
                //         debug!("but i still have unstaged. lets go to last hunk or line? then");
                //         debug!("it is better line, cause it could be visually closer!");
                //         debug!("u need last line!");
                //         // let last_line = diff.last_visible_line();
                //         // hey!
                //         debug!("if i have hunk below - go below, but first actual line");
                //         debug!("if i have something above - go above last line");
                //         let (nearest_top, nearest_bottom) = diff.nearest(iter.line(), context);
                //         debug!("????????????????? nearest_top {:?} nearest_bottom {:?} iter line {:?}", nearest_top, nearest_bottom, iter.line());
                //         if let Some(nearest_bottom) = nearest_bottom {
                //             iter.set_line(nearest_bottom);
                //         } else {
                //             iter.set_line(nearest_top.unwrap());
                //         }
                //     }
                // }
                // debug!(
                //     "...........................> {:?} {:?}",
                //     line_no,
                //     iter.line()
                // );
                // context is here, right in the full filled context!
            }
        }
        iter
        // so. lets see just 2 cases: staging-unstaging
        // user staged --------------------------

        // staged diff -> do nothing (stay same and staged will come)

        // staged file and has another below -> do nothing (will stay on another)
        // staged file and has no more files -> go to staged diff
        // staged file and has no more files below, but 1 above -> go to file above

        // staged hunk and has another below -> do nothing (will stay on another hunk)
        // staged hunk and has no more hunks -> go to unstaged diff
        // staged hunk and has no more hunks below, but 1 above -> go to hunk above

        // user unstaged --------------------------------
        // unstaged diff -> go to staged diff

        // ustaged file and has another below -> do nothing (will stay on another)
        // unstaged file and has no more files -> go to unstaged diff
        // ustaged file and has no more files below, but 1 above -> go file above

        // unstaged hunk and has another below -> do nothing (stay on hunk)
        // staged hunk and has no more hunks -> go to unstaged diff
        // staged hunk and has no more hunks below, but 1 above -> go to hunk above.

        // during staging-unstaging op it need to remember 'op'.
        // then after op completed, choose proper direction.
        // for remember 'DiffKind' could be used AND view.kind could be used.

        // i have a common method: stage which is used for staging/unstaging/killling
        // thats the enter point.
        // Lets introduce Op - current user operation.
        // DiffKind, direction and ViewKind. Hot to impress direction?
        // it does not needed. Any op always removes something from Diff.
        // but unstaged could be moved either to staged or killed!
        // never mind. even if kill staged, it need to go to unstaged, anyways.
        // so, just DiffKind and ViewKind!
        // ViewKind already has DiffKind in it, but only for diff!
        // lets put it in every view!
        // ViewKind is never used!!!!!
        // I have already StageOp!
    }

    pub fn smart_cursor_position(&self, buffer: &TextBuffer) -> TextIter {
        // its buggy. it need to now what happens right now!
        // it need to introduce what_it_was at the end of render
        // then check "what it was" by previous UnderCursor and
        // render source! then looks like it need to compare it
        // with current UnderCursor, but how? the cursor is not
        // set yet! looks like it need to do it AFTER cursor:
        // - render (rendersource::Git)
        // - smart_choose_pos before cursor
        // - cursor to highlight whats behind
        // - smart_choose_pos AFTER cursor ???? why after? why second chose????
        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let last_line = buffer.end_iter().line();
        if iter.line() == last_line {
            for diff in [&self.conflicted, &self.unstaged, &self.staged]
                .into_iter()
                .flatten()
            {
                if !diff.files.is_empty() {
                    return buffer
                        .iter_at_line(diff.files[0].view.line_no.get())
                        .unwrap();
                }
            }
            if iter.line() == last_line {
                if let Some(untracked) = &self.untracked {
                    if !untracked.files.is_empty() {
                        return buffer
                            .iter_at_line(
                                untracked.files[0].view.line_no.get(),
                            )
                            .unwrap();
                    }
                }
            }
        }
        iter
    }

    pub fn has_staged(&self) -> bool {
        if let Some(staged) = &self.staged {
            return !staged.files.is_empty();
        }
        false
    }

    // pub fn dump<'a>(
    //     &'a mut self,
    //     txt: &StageView,
    //     context: &mut StatusRenderContext<'a>,
    // ) {
    //     let mut path = self.path.clone().unwrap();
    //     path.push(DUMP_DIR);
    //     let create_result = std::fs::create_dir(&path);
    //     match create_result {
    //         Ok(_) => {}
    //         Err(err) => {
    //             if err.kind() != ErrorKind::AlreadyExists {
    //                 panic!("Error {}", err);
    //             }
    //         }
    //     }
    //     let datetime: DateTime<Utc> = std::time::SystemTime::now().into();
    //     let fname = format!("dump_{}.txt", datetime.format("%d_%m_%Y_%T"));
    //     path.push(fname);
    //     let mut file = std::fs::File::create(path).unwrap();

    //     let buffer = txt.buffer();

    //     let pos = buffer.cursor_position();
    //     let iter = buffer.iter_at_offset(pos);
    //     self.cursor(txt, iter.line(), iter.offset(), context);
    //     self.render(txt, None, context);

    //     let iter = buffer.iter_at_offset(0);
    //     let end_iter = buffer.end_iter();
    //     let content = buffer.text(&iter, &end_iter, true);
    //     file.write_all(content.as_bytes()).unwrap();
    //     file.write_all("\n ================================= \n".as_bytes())
    //         .unwrap();
    //     file.write_all(format!("context: {:?}", context).as_bytes())
    //         .unwrap();
    //     if let Some(conflicted) = &self.conflicted {
    //         file.write_all(
    //             "\n ==============Coflicted================= \n".as_bytes(),
    //         )
    //         .unwrap();
    //         file.write_all(conflicted.dump().as_bytes()).unwrap();
    //     }
    //     if let Some(unstaged) = &self.unstaged {
    //         file.write_all(
    //             "\n ==============UnStaged================= \n".as_bytes(),
    //         )
    //         .unwrap();
    //         file.write_all(unstaged.dump().as_bytes()).unwrap();
    //     }
    //     if let Some(staged) = &self.staged {
    //         file.write_all(
    //             "\n ==============Staged================= \n".as_bytes(),
    //         )
    //         .unwrap();
    //         file.write_all(staged.dump().as_bytes()).unwrap();
    //     }
    //     self.sender
    //         .send_blocking(Event::Toast(String::from("dumped")))
    //         .expect("cant send through sender");
    // }
    pub fn head_oid(&self) -> crate::Oid {
        self.head.as_ref().unwrap().oid
    }

    pub fn copy_to_clipboard<'a>(
        &'a self,
        txt: &StageView,
        start_offset: i32,
        end_offset: i32,
        context: &mut StatusRenderContext<'a>,
    ) {
        // in fact the content IS already copied to clipboard
        // so, here it need to clean it from status_view artefacts
        let buffer = txt.buffer();
        let start_iter = buffer.iter_at_offset(start_offset);
        let end_iter = buffer.iter_at_offset(end_offset);
        let line_from = start_iter.line();
        let line_from_offset = start_iter.line_offset();
        let line_to = end_iter.line();
        let line_to_offset = end_iter.line_offset();
        let mut clean_content: HashMap<i32, (String, i32)> = HashMap::new();
        for diff in [&self.conflicted, &self.unstaged, &self.staged]
            .into_iter()
            .flatten()
        {
            diff.collect_clean_content(
                line_from,
                line_to,
                &mut clean_content,
                context,
            );
        }
        if !clean_content.is_empty() {
            let clipboard = txt.clipboard();
            glib::spawn_future_local({
                async move {
                    let mut new_content = String::new();
                    let mut replace_content = false;
                    if let Ok(Some(content)) =
                        clipboard.read_text_future().await
                    {
                        for (i, line) in content.split("\n").enumerate() {
                            replace_content = true;
                            let ind = i as i32 + line_from;
                            if let Some((clean_line, clean_offset)) =
                                clean_content.get(&ind)
                            {
                                if ind == line_from
                                    && &line_from_offset >= clean_offset
                                {
                                    new_content.push_str(
                                        &clean_line[(line_from_offset
                                            - clean_offset)
                                            as usize..],
                                    );
                                } else if ind == line_to
                                    && &line_to_offset >= clean_offset
                                {
                                    new_content.push_str(
                                        &clean_line[..(line_to_offset
                                            - clean_offset)
                                            as usize],
                                    );
                                } else {
                                    new_content.push_str(clean_line);
                                }
                            } else {
                                new_content.push_str(line);
                            }
                            new_content.push('\n');
                        }
                    }
                    if replace_content {
                        clipboard.set_text(&new_content);
                    }
                }
            });
        };
    }

    pub fn debug<'a>(
        &'a mut self,
        _txt: &StageView,
        _context: &mut StatusRenderContext<'a>,
    ) {
        gio::spawn_blocking({
            let sender = self.sender.clone();
            let path = self.path.clone().unwrap();
            move || {
                get_head(path, sender).expect("cant get head");
            }
        });
    }
}
