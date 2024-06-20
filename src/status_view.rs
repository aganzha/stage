// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

pub mod commit;
pub mod container;
pub mod context;
pub mod headerbar;
pub mod tags;
pub mod stage_view;
use crate::dialogs::{alert, DangerDialog, YES};
use crate::git::{merge, remote, stash, abort_rebase, continue_rebase};
use crate::utils::StrPath;

use container::ViewContainer;
use core::time::Duration;
use git2::RepositoryState;
use stage_view::{cursor_to_line_offset, StageView};

pub mod reconciliation;
pub mod render;
pub mod tests;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::status_view::render::View;
use crate::{
    checkout_oid, get_current_repo_status, get_directories, git_debug,
    stage_untracked, stage_via_apply, track_changes, StageOp, Diff,
    Event, File as GitFile, Head, State, StatusRenderContext, Untracked,
};
use async_channel::Sender;

use gio::{
    Cancellable, File, FileMonitor, FileMonitorEvent, FileMonitorFlags,
};

use glib::clone;
use glib::signal::SignalHandlerId;
use gtk4::prelude::*;
use gtk4::{
    gio, glib, Box, Label as GtkLabel, ListBox, Orientation, SelectionMode,
    TextBuffer, TextIter, TextView, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, Banner, EntryRow, PasswordEntryRow, SwitchRow,
}; // _Window,
use log::{debug, info, trace};

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
    Cursor(i32),
    Expand(i32),
}

#[derive(Debug, Clone)]
pub struct Status {
    pub path: Option<PathBuf>,
    pub sender: Sender<Event>,
    pub head: Option<Head>,
    pub upstream: Option<Head>,
    pub state: Option<State>,

    pub untracked_spacer: Label,
    pub untracked_label: Label,
    pub untracked: Option<Untracked>,

    pub staged_spacer: Label,
    pub staged_label: Label,
    pub staged: Option<Diff>,

    pub unstaged_spacer: Label,
    pub unstaged_label: Label,
    pub unstaged: Option<Diff>,

    pub conflicted_spacer: Label,
    pub conflicted_label: Label,
    pub conflicted: Option<Diff>,

    pub rendered: bool, // what it is for ????
    pub stashes: Option<stash::Stashes>,
    pub monitor_global_lock: Rc<RefCell<bool>>,
    pub monitor_lock: Rc<RefCell<HashSet<PathBuf>>>,
    pub settings: gio::Settings,
}

impl Status {
    pub fn new(
        path: Option<PathBuf>,
        settings: gio::Settings,
        sender: Sender<Event>,
    ) -> Self {
        Self {
            path,
            sender,
            head: None,
            upstream: None,
            state: None,
            untracked_spacer: Label::from_string(""),
            untracked_label: Label::from_string(
                "<span weight=\"bold\" color=\"#8b6508\">Untracked files</span>",
            ),
            untracked: None,
            staged_spacer: Label::from_string(""),
            staged_label: Label::from_string(
                "<span weight=\"bold\" color=\"#8b6508\">Staged changes</span>",
            ),
            staged: None,
            unstaged_spacer: Label::from_string(""),
            unstaged_label: Label::from_string(
                "<span weight=\"bold\" color=\"#8b6508\">Unstaged changes</span>",
            ),
            unstaged: None,
            conflicted_spacer: Label::from_string(""),
            conflicted_label: Label::from_string(
                "<span weight=\"bold\" color=\"#ff0000\">Conflicts</span>",
            ),
            conflicted: None,
            rendered: false,
            stashes: None,
            monitor_global_lock: Rc::new(RefCell::new(true)),
            monitor_lock: Rc::new(RefCell::new(HashSet::new())),
            settings
        }
    }

    pub fn file_at_cursor(&self) -> Option<&GitFile> {
        for diff in [&self.staged, &self.unstaged] {
            if let Some(diff) = diff {
                let maybe_file = diff.files.iter().find(|f| {
                    f.view.is_current()
                        || f.hunks.iter().any(|h| h.view.is_active())
                });
                if maybe_file.is_some() {
                    return maybe_file;
                }
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
            let mut line_no = hunk.new_start;
            let mut col_no = 0;
            if !hunk.view.is_current() {
                let line =
                    hunk.lines.iter().find(|l| l.view.is_current()).unwrap();
                line_no = line.new_line_no.or(line.old_line_no).unwrap_or(0);
                let pos = txt.buffer().cursor_position();
                let iter = txt.buffer().iter_at_offset(pos);
                col_no = iter.line_offset();
            }
            let mut base = self.path.clone().unwrap();
            base.pop();
            base.push(&file.path);
            return Some((base, line_no as i32, col_no));
        }
        None
    }

    pub fn to_abs_path(&self, path: &Path) -> PathBuf {
        let mut base = self.path.clone().unwrap();
        base.pop();
        base.push(path);
        base
    }

    pub fn branch_name(&self) -> String {
        if let Some(head) = &self.head {
            return head.branch.to_string();
        }
        "".to_string()
    }

    pub fn update_path(
        &mut self,
        path: PathBuf,
        monitors: Rc<RefCell<Vec<FileMonitor>>>,
        user_action: bool,
    ) {
        // here could come path selected by the user
        // this is 'dirty' one. The right path will
        // came from git with /.git/ suffix
        // but the 'dirty' path will be used first
        // for querying repo status and investigate real one
        if user_action {
            monitors.borrow_mut().retain(|fm: &FileMonitor| {
                fm.cancel();
                false
            });
        } else {
            // investigated path
            assert!(path.ends_with(".git/"));
            if self.path.is_none() || path != self.path.clone().unwrap() {
                let mut paths = self.settings.get::<Vec<String>>("paths");
                let str_path =
                    String::from(path.to_str().unwrap()).replace(".git/", "");
                self.settings
                    .set("lastpath", str_path.clone())
                    .expect("cant set lastpath");
                if !paths.contains(&str_path) {
                    paths.push(str_path.clone());
                    self.settings
                        .set("paths", paths)
                        .expect("cant set settings");
                }
                self.setup_monitors(monitors, PathBuf::from(str_path));
            }
        }
        self.path.replace(path.clone());
    }

    pub fn lock_monitors(&mut self, lock: bool) {
        self.monitor_global_lock.replace(lock);
    }

    pub fn setup_monitors(
        &mut self,
        monitors: Rc<RefCell<Vec<FileMonitor>>>,
        path: PathBuf,
    ) {
        glib::spawn_future_local({
            let sender = self.sender.clone();
            let lock = self.monitor_lock.clone();
            let global_lock = self.monitor_global_lock.clone();
            async move {
                let mut directories = gio::spawn_blocking({
                    let path = path.clone();
                    move || get_directories(path)
                })
                .await
                .expect("cant get direcories");
                let root = path
                    .to_str()
                    .expect("cant get string from path")
                    .replace(".git/", "");
                directories.insert(root.clone());
                for dir in directories {
                    trace!("dirname {:?}", dir);
                    let dir_name = match dir {
                        name if name == root => name,
                        name => {
                            format!("{}{}", root, name)
                        }
                    };
                    trace!("setup monitor {:?}", dir_name);
                    let file = File::for_path(dir_name);
                    let flags = FileMonitorFlags::empty();

                    let monitor = file
                        .monitor_directory(
                            flags,
                            Cancellable::current().as_ref(),
                        )
                        .expect("cant get monitor");
                    monitor.connect_changed({
                        let path = path.clone();
                        let sender = sender.clone();
                        let lock = lock.clone();
                        let global_lock = global_lock.clone();
                        move |_monitor, file, _other_file, event| {
                            // TODO get from SELF.settings
                            if *global_lock.borrow() {
                                return;
                            }
                            let patterns_to_exclude: Vec<&str> =
                                vec!["/.#", "/mout", "flycheck_", "/sed"];
                            match event {
                                FileMonitorEvent::Changed | FileMonitorEvent::ChangesDoneHint => {
                                    // ChangesDoneHint is not fired for small changes :(
                                    let file_path =
                                        file.path().expect("no file path");
                                    let str_file_path = file_path
                                        .clone()
                                        .into_os_string()
                                        .into_string()
                                        .expect("no file path");
                                    for pat in patterns_to_exclude {
                                        if str_file_path.contains(pat) {
                                            return;
                                        }
                                    }
                                    if lock.borrow().contains(&file_path) {
                                        trace!("NO WAY: monitor locked");
                                        return;
                                    }
                                    lock.borrow_mut().insert(file_path.clone());
                                    trace!("set monitor lock");
                                    glib::source::timeout_add_local(
                                        Duration::from_millis(300),
                                        {
                                            let lock = lock.clone();
                                            let path = path.clone();
                                            let sender = sender.clone();
                                            let file_path = file_path.clone();
                                            move || {
                                                trace!(".......... THROTTLED {:?}", file_path);
                                                gio::spawn_blocking({
                                                    let path = path.clone();
                                                    let sender =
                                                        sender.clone();
                                                    let file_path =
                                                        file_path.clone();
                                                    lock.borrow_mut().remove(&file_path);
                                                    trace!(
                                                        "release monitor lock"
                                                    );
                                                    move || {
                                                        track_changes(
                                                            path, file_path,
                                                            sender,
                                                        )
                                                    }
                                                });
                                                glib::ControlFlow::Break
                                            }
                                        },
                                    );
                                }
                                _ => {
                                    trace!(
                                        "file event in monitor {:?} {:?}",
                                        event,
                                        file.path()
                                    );
                                }
                            }
                        }
                    });
                    monitors.borrow_mut().push(monitor);
                }
                trace!("my monitors a set {:?}", monitors.borrow().len());
            }
        });
    }

    pub fn update_stashes(&mut self, stashes: stash::Stashes) {
        self.stashes.replace(stashes);
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
                get_current_repo_status(path, sender);
            }
        });
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
                    move || {
                        remote::pull(path, sender, user_pass);
                    }
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
            return upstream.branch.clone();
        }
        if let Some(head) = &self.head {
            return format!("origin/{}", &head.branch);
        }
        String::from("origin/master")
    }

    pub fn commit(
        &self,
        window: &ApplicationWindow, // &impl IsA<Gtk4Window>,
    ) {
        let mut amend_message: Option<String> = None;
        if let Some(head) = &self.head {
            if let Some(upstream) = &self.upstream {
                if head.oid != upstream.oid {
                    amend_message.replace(head.commit_body.clone());
                }
            } else {
                amend_message.replace(head.commit_body.clone());
            }
        }
        commit::commit(
            self.path.clone(),
            amend_message,
            window,
            self.sender.clone(),
        );
    }

    pub fn update_head(
        &mut self,
        mut head: Head,
        txt: &StageView,
        context: &mut StatusRenderContext,
    ) {
        // refactor.enrich
        if let Some(current_head) = &self.head {
            head.enrich_view(current_head);
        }
        self.head.replace(head);
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_upstream(
        &mut self,
        mut upstream: Option<Head>,
        txt: &StageView,
        context: &mut StatusRenderContext,
    ) {
        if let Some(rendered) = &mut self.upstream {
            if let Some(new) = upstream.as_mut() {
                new.enrich_view(rendered);
            } else {
                rendered.erase(&txt.buffer(), context);
            }
        }
        self.upstream = upstream;
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_state(
        &mut self,
        mut state: State,
        txt: &StageView,
        context: &mut StatusRenderContext,
    ) {
        if let Some(current_state) = &self.state {
            state.enrich_view(current_state)
        }
        self.state.replace(state);
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_untracked(
        &mut self,
        mut untracked: Untracked,
        txt: &StageView,
        context: &mut StatusRenderContext,
    ) {
        let mut settings =
            self.settings.get::<HashMap<String, Vec<String>>>("ignored");

        let repo_path = self.path.clone().unwrap();
        let str_path = repo_path.as_str();
        if let Some(ignored) = settings.get_mut(str_path) {
            untracked.files.retain(|f| {
                let str_path = f
                    .path.as_str();
                !ignored.contains(&str_path.to_string())
            });
        }
        if let Some(u) = &mut self.untracked {
            untracked.enrich_view(u, &txt.buffer(), context);
        }
        self.untracked.replace(untracked);
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_conflicted(
        &mut self,
        mut diff: Diff,
        txt: &StageView,
        window: &ApplicationWindow,
        sender: Sender<Event>,
        banner: &Banner,
        banner_button: &Widget,
        banner_button_clicked: Rc<RefCell<Option<SignalHandlerId>>>,
        context: &mut StatusRenderContext,
    ) {
        if let Some(s) = &mut self.conflicted {
            if s.has_conflicts() && !diff.has_conflicts() {
                self.conflicted_label.content = String::from("<span weight=\"bold\"\
                                                              color=\"#1c71d8\">Conflicts resolved</span>\
                                                              stage changes to complete merge");
                self.conflicted_label.view.dirty(true);
            }
            diff.enrich_view(s, &txt.buffer(), context);
        }
        if let Some(state) = &self.state {
            if diff.is_empty() {
                if banner.is_revealed() {
                    banner.set_revealed(false);
                }

                if state.need_final_commit() || state.need_rebase_continue() {
                    banner.set_title(&state.title_for_proceed_banner());
                    banner.set_css_classes(&["success"]);
                    banner.set_button_label(
                        if state.need_final_commit() {
                            Some("Commit")
                        } else {
                            Some("Continue")
                        }
                    );
                    banner_button.set_css_classes(&["suggested-action"]);
                    banner.set_revealed(true);
                    if let Some(handler_id) = banner_button_clicked.take() {
                        banner.disconnect(handler_id);
                    }
                    let new_handler_id = banner.connect_button_clicked({
                        let sender = sender.clone();
                        let path = self.path.clone();
                        let window = window.clone();
                        let state = state.state;
                        move |_| {
                            let sender = sender.clone();
                            let path = path.clone();
                            let window = window.clone();
                            glib::spawn_future_local({
                                async move {
                                    gio::spawn_blocking({
                                        move || {
                                            match state {
                                                RepositoryState::Merge =>                                                    
                                                    merge::final_merge_commit(
                                                        path.clone().unwrap(),
                                                        sender,
                                                    ),
                                                RepositoryState::RebaseMerge => 
                                                    continue_rebase(
                                                        path.clone().unwrap(),
                                                        sender,
                                                    ),
                                                _ => merge::final_commit(
                                                    path.clone().unwrap(),
                                                    sender,
                                                )
                                                    
                                            }
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
                banner.set_css_classes(&["error"]);
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
                    move |_| {
                        gio::spawn_blocking({
                            let sender = sender.clone();
                            let path = path.clone();
                            move || {
                                match state {
                                    RepositoryState::RebaseMerge => abort_rebase(path.expect("no path"), sender),
                                    _ => merge::abort(path.expect("no path"), sender)
                                }
                            }
                        });
                    }
                });
                banner_button_clicked.replace(Some(new_handler_id));
            }
        }
        self.conflicted.replace(diff);
        self.render(txt, RenderSource::Git, context);
        // restore original label for future conflicts
        self.conflicted_label.content = String::from(
            "<span weight=\"bold\" color=\"#ff0000\">Conflicts</span>",
        );
        self.conflicted_label.view.dirty(true);
    }

    pub fn update_staged(
        &mut self,
        mut diff: Diff,
        txt: &StageView,
        context: &mut StatusRenderContext,
    ) {
        if let Some(s) = &mut self.staged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(s, &txt.buffer(), context);
        }
        self.staged.replace(diff);

        self.render(txt, RenderSource::GitDiff, context);
    }

    pub fn update_unstaged(
        &mut self,
        mut diff: Diff,
        txt: &StageView,
        context: &mut StatusRenderContext,
    ) {
        let buffer = &txt.buffer();

        if let Some(u) = &mut self.unstaged {
            diff.enrich_view(u, buffer, context);
        }

        self.unstaged.replace(diff);

        self.render(txt, RenderSource::GitDiff, context);
    }

    pub fn resize_highlights(&self, txt: &StageView, ctx: &mut StatusRenderContext) {
        let buffer = txt.buffer();
        let iter = buffer.iter_at_offset(buffer.cursor_position());
        self.cursor(txt, iter.line(), iter.offset(), ctx);
        glib::source::timeout_add_local(
            Duration::from_millis(10),
            {
                let txt = txt.clone();
                let ctx = ctx.clone();
                move || {
                    txt.bind_highlights(&ctx);
                    glib::ControlFlow::Break
                }
            },
        );
    }
    
    /// cursor does not change structure, but changes highlights
    /// it will collect highlights in context. no need further render
    pub fn cursor(
        &self,
        txt: &StageView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext,
    ) -> bool {
        // this is actually needed for views which are not implemented
        // ViewContainer, and does not affect context!
        context.highlight_cursor = line_no;

        // context.update_cursor_pos(line_no, offset);
        let mut changed = false;
        if let Some(untracked) = &self.untracked {
            changed = untracked.cursor(line_no, false, context) || changed;
        }
        if let Some(conflicted) = &self.conflicted {
            changed = conflicted.cursor(line_no, false, context) || changed;
        }
        if let Some(unstaged) = &self.unstaged {
            changed = unstaged.cursor(line_no, false, context) || changed;
        }
        if let Some(staged) = &self.staged {
            changed = staged.cursor(line_no, false, context) || changed;
        }
        // NO NEED TO RENDER!
        txt.bind_highlights(context);
        changed
    }

    // Status
    pub fn expand(
        &self,
        txt: &StageView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext,
    ) {
        if let Some(conflicted) = &self.conflicted {
            if let Some(expanded_line) = conflicted.expand(line_no, context) {
                self.render(txt, RenderSource::Expand(expanded_line), context);
                return;
            }
        }

        if let Some(unstaged) = &self.unstaged {
            if let Some(expanded_line) = unstaged.expand(line_no, context) {
                self.render(txt, RenderSource::Expand(expanded_line), context);
                return;
            }
        }
        if let Some(staged) = &self.staged {
            if let Some(expanded_line) = staged.expand(line_no, context) {
                self.render(txt, RenderSource::Expand(expanded_line), context);
                return;
            }
        }
    }

    pub fn render(
        &self,
        txt: &StageView,
        source: RenderSource,
        context: &mut StatusRenderContext,
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
            if untracked.files.is_empty() {
                // hack :( TODO - get rid of it
                self.untracked_spacer.view.squash(true);
                self.untracked_label.view.squash(true);
            }
            self.untracked_spacer.render(&buffer, &mut iter, context);
            self.untracked_label.render(&buffer, &mut iter, context);
            untracked.render(&buffer, &mut iter, context);
        }

        if let Some(conflicted) = &self.conflicted {
            if conflicted.files.is_empty() {
                self.conflicted_spacer.view.squash(true);
                self.conflicted_label.view.squash(true);
            }
            self.conflicted_spacer.render(&buffer, &mut iter, context);
            self.conflicted_label.render(&buffer, &mut iter, context);
            conflicted.render(&buffer, &mut iter, context);
        }

        if let Some(unstaged) = &self.unstaged {
            if unstaged.files.is_empty() {
                // hack :(
                self.unstaged_spacer.view.squash(true);
                self.unstaged_label.view.squash(true);
            }
            self.unstaged_spacer.render(&buffer, &mut iter, context);
            self.unstaged_label.render(&buffer, &mut iter, context);
            unstaged.render(&buffer, &mut iter, context);
        }

        if let Some(staged) = &self.staged {
            if staged.files.is_empty() {
                // hack :(
                self.staged_spacer.view.squash(true);
                self.staged_label.view.squash(true);
            }
            self.staged_spacer.render(&buffer, &mut iter, context);
            self.staged_label.render(&buffer, &mut iter, context);
            staged.render(&buffer, &mut iter, context);
        }

        cursor_to_line_offset(&txt.buffer(), initial_line_offset);

        if source == RenderSource::GitDiff || source == RenderSource::Git {
            // it need to put cursor in place here,
            // EVEN WITHOUT SMART CHOOSE
            // cause cursor could be, for example, in unstaged hunk
            // during staging. after staging, the content behind the cursor
            // is changed (hunk is erased and new hunk come to its place),
            // and it need to highlight new content on the same cursor
            // position
            let iter = self.smart_cursor_position(&buffer);
            buffer.place_cursor(&iter);
            self.cursor(&txt, iter.line(), iter.offset(), context);
        }

        txt.bind_highlights(context);

        // match source {
        //     RenderSource::Cursor(_) => {
        //         // avoid loops on cursor renders
        //         trace!("avoid cursor position on cursor");
        //     }
        //     RenderSource::Expand(line_no) => {
        //         self.choose_cursor_position(
        //             txt,
        //             &buffer,
        //             Some(line_no),
        //             context,
        //         );
        //     }
        //     RenderSource::Git => {
        //         self.choose_cursor_position(txt, &buffer, None, context);
        //     }
        // };
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
        // - smart_choose_pos AFTER cursor
        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let last_line = buffer.end_iter().line();
        if iter.line() == last_line {
            for diff in [&self.conflicted, &self.unstaged, &self.staged] {
                if let Some(diff) = diff {
                    if !diff.files.is_empty() {
                        return buffer
                            .iter_at_line(diff.files[0].view.line_no.get())
                            .unwrap();
                    }
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
            // if iter.line() == last_line {
            //     debug!("lets maybe state ? {:?}", self.state);
            //     if let Some(state) = &self.state {
            //         return buffer.iter_at_line(state.view.line_no.get()).unwrap();
            //     }
            // }
        }
        iter
    }

    pub fn ignore(
        &mut self,
        txt: &StageView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext,
    ) {
        if let Some(untracked) = &self.untracked {
            for file in &untracked.files {
                // TODO!
                // refactor to some generic method
                // why other elements do not using this?
                let view = file.get_view();
                if view.is_current() && view.line_no.get() == line_no {
                    let ignore_path = file
                        .path
                        .clone()
                        .into_os_string()
                        .into_string()
                        .expect("wrong string");
                    trace!("ignore path! {:?}", ignore_path);
                    let mut settings =
                        self.settings
                            .get::<HashMap<String, Vec<String>>>("ignored");
                    let repo_path = self
                        .path
                        .clone()
                        .expect("no path")
                        .into_os_string()
                        .into_string()
                        .expect("wrong path");
                    if let Some(stored) = settings.get_mut(&repo_path) {
                        stored.push(ignore_path);
                        trace!("added ignore {:?}", settings);
                    } else {
                        settings.insert(repo_path, vec![ignore_path]);
                        trace!("first ignored file {:?}", settings);
                    }
                    self.settings
                        .set("ignored", settings)
                        .expect("cant set settings");
                    self.update_untracked(
                        self.untracked.clone().unwrap(),
                        txt,
                        context,
                    );
                    break;
                }
            }
        }
    }

    pub fn stage_in_conflict(&self, window: &ApplicationWindow) -> bool {
        // it need to implement method for diff, which will return current Hunk, Line and File and use it in stage.
        // also it must return indicator what of this 3 is current.
        if let Some(conflicted) = &self.conflicted {
            // also someone can press stage on label!
            for f in &conflicted.files {
                // also someone can press stage on file!
                for hunk in &f.hunks {
                    // also someone can press stage on hunk!
                    for line in &hunk.lines {
                        if line.view.is_current() {
                            glib::spawn_future_local({
                                let path = self.path.clone().unwrap();
                                let sender = self.sender.clone();
                                let file_path = f.path.clone();
                                let hunk = hunk.clone();
                                let line = line.clone();
                                let window = window.clone();
                                async move {
                                    if hunk.conflicts_count > 0
                                        && line.is_side_of_conflict()
                                    {
                                        gio::spawn_blocking({
                                            move || {
                                                merge::choose_conflict_side_of_hunk(
                                                    path, file_path, hunk, line,
                                                    sender,
                                                )
                                            }
                                        }).await.unwrap_or_else(|e| {
                                            alert(format!("{:?}", e)).present(&window);
                                            Ok(())
                                        }).unwrap_or_else(|e| {
                                            alert(e).present(&window);
                                        });
                                    } else {
                                        // this should be never called
                                        // conflicts are resolved in branch above
                                        gio::spawn_blocking({
                                            move || {
                                                merge::cleanup_last_conflict_for_file(
                                                    path, None, file_path, sender,
                                                )
                                            }
                                        }).await.unwrap_or_else(|e| {
                                            alert(format!("{:?}", e)).present(&window);
                                            Ok(())
                                        }).unwrap_or_else(|e| {
                                            alert(e).present(&window);
                                        });
                                    }
                                }
                            });
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    // regular commit
    pub fn stage(
        &mut self,
        _txt: &StageView,
        _line_no: i32,
        op: StageOp,
        window: &ApplicationWindow,
    ) {
        if let Some(untracked) = &self.untracked {
            for file in &untracked.files {
                if file.get_view().is_current() {
                    gio::spawn_blocking({
                        let path = self.path.clone();
                        let sender = self.sender.clone();
                        let file_path = file.path.clone();
                        move || {
                            stage_untracked(
                                path.expect("no path"),
                                file_path,
                                sender,
                            );
                        }
                    });
                    return;
                }
            }
        }

        if self.stage_in_conflict(window) {
            return;
        }

        // just a check
        match op {
            StageOp::Stage | StageOp::Kill => {
                if self.unstaged.is_none() {
                    return;
                }
            }
            StageOp::Unstage => {
                if self.staged.is_none() {
                    return;
                }
            }
        }

        let diff = {
            match op {
                StageOp::Stage | StageOp::Kill => {
                    self.unstaged.as_mut().unwrap()
                }
                StageOp::Unstage => self.staged.as_mut().unwrap(),
            }
        };

        let (file, hunk) = diff.chosen_file_and_hunk();
        if file.is_none() {
            info!("no file to stage");
            self.sender.send_blocking(Event::Toast(String::from("No file to stage")))
                .expect("cant send through sender");
            return;
        }
        trace!(
            "stage via apply ----------------------> {:?} {:?} {:?}",
            op,
            file,
            hunk
        );

        glib::spawn_future_local({
            let window = window.clone();
            let path = self.path.clone();
            let sender = self.sender.clone();
            let file_path = file.unwrap().path.clone();
            let hunk = hunk.and_then(|h| Some(h.header.clone()));
            async move {
                gio::spawn_blocking({
                    move || {
                        stage_via_apply(
                            path.expect("no path"),
                            file_path,
                            hunk,
                            op,
                            sender,
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

    pub fn has_staged(&self) -> bool {
        if let Some(staged) = &self.staged {
            return !staged.files.is_empty();
        }
        false
    }
    pub fn debug(
        &mut self,
        txt: &StageView,
        context: &mut StatusRenderContext,
    ) {

    }

    pub fn checkout_error(
        &mut self,
        window: &ApplicationWindow,
        oid: crate::Oid,
        ref_log_msg: String,
        err_msg: String,
    ) {
        glib::spawn_future_local({
            let window = window.clone();
            let sender = self.sender.clone();
            let path = self.path.clone();
            async move {
                let bx = Box::builder()
                    .orientation(Orientation::Vertical)
                    .margin_top(2)
                    .margin_bottom(2)
                    .margin_start(2)
                    .margin_end(2)
                    .spacing(12)
                    .build();
                let label = GtkLabel::builder().label(&err_msg).build();

                let lb = ListBox::builder()
                    .selection_mode(SelectionMode::None)
                    .css_classes(vec![String::from("boxed-list")])
                    .build();
                let stash = SwitchRow::builder()
                    .title("Stash changes and checkout")
                    .css_classes(vec!["input_field"])
                    .active(true)
                    .build();
                let conflicts = SwitchRow::builder()
                    .title("Checkout with conflicts")
                    .css_classes(vec!["input_field"])
                    .active(false)
                    .build();
                lb.append(&stash);
                lb.append(&conflicts);
                let _bind = stash
                    .bind_property("active", &conflicts, "active")
                    .transform_to(move |_, value: bool| Some(!value))
                    //.bidirectional()
                    .build();
                let _bind = conflicts
                    .bind_property("active", &stash, "active")
                    .transform_to(move |_, value: bool| Some(!value))
                    //.bidirectional()
                    .build();
                bx.append(&label);
                bx.append(&lb);

                let dialog = crate::confirm_dialog_factory(
                    &window,
                    Some(&bx),
                    "Checkout error ",
                    "Proceed",
                );
                let response = dialog.choose_future().await;
                if "confirm" != response {
                    return;
                }
                let stash = stash.is_active();
                gio::spawn_blocking({
                    let path = path.clone().expect("no path");
                    let sender = sender.clone();
                    move || {
                        if stash {
                            stash::stash(
                                path.clone(),
                                ref_log_msg.clone(),
                                true,
                                sender.clone(),
                            );
                        }
                        // DOES NOT WORK! if local branch has commits diverged from upstream
                        // all commit become lost! because you simple checkout ortogonal tree
                        // and put head on it! IT NEED TO MERGE upstream branch of course!
                        // think about it! perhaps it need to call merge analysys
                        // during pull! if its fast formard - ok. if not - do merge, please.
                        // see what git suggests:
                        // Pulling without specifying how to reconcile divergent branches is
                        // discouraged. You can squelch this message by running one of the following
                        // commands sometime before your next pull:

                        //   git config pull.rebase false  # merge (the default strategy)
                        //   git config pull.rebase true   # rebase
                        //   git config pull.ff only       # fast-forward only

                        // You can replace "git config" with "git config --global" to set a default
                        // preference for all repositories. You can also pass --rebase, --no-rebase,
                        // or --ff-only on the command line to override the configured default per
                        // invocation.
                        checkout_oid(path, sender, oid, Some(ref_log_msg));
                    }
                });
            }
        });
    }
}
