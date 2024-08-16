// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

pub mod commit;
pub mod context;
pub mod headerbar;
pub mod monitor;
pub mod render;
pub mod stage_view;
pub mod tags;
use crate::dialogs::{alert, DangerDialog, YES};
use crate::git::{abort_rebase, continue_rebase, merge, remote, stash};
use crate::utils::StrPath;

use core::time::Duration;
use git2::RepositoryState;
use render::ViewContainer; // MayBeViewContainer o
use stage_view::{cursor_to_line_offset, StageView};

pub mod reconciliation;
pub mod tests;
pub mod view;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::status_view::view::View;
use crate::{
    get_current_repo_status,
    stage_untracked,
    stage_via_apply,
    track_changes,
    Diff,
    DiffKind,
    Event,
    File as GitFile,
    Head,
    StageOp,
    State,
    StatusRenderContext, //, Untracked,
};
use async_channel::Sender;

use gio::FileMonitor;

use chrono::{offset::Utc, DateTime};
use glib::clone;
use glib::signal::SignalHandlerId;
use gtk4::prelude::*;
use gtk4::{gio, glib, ListBox, SelectionMode, TextBuffer, TextIter, Widget};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, Banner, EntryRow, PasswordEntryRow, SwitchRow,
};
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
    Expand(i32),
}

pub const DUMP_DIR: &str = "stage_dump";

#[derive(Debug, Clone)]
pub struct Status {
    pub path: Option<PathBuf>,
    pub sender: Sender<Event>,
    pub head: Option<Head>,
    pub upstream: Option<Head>,
    pub state: Option<State>,

    // TODO! remove labels from Untracked as in diff!
    // pub untracked_spacer: Label,
    // pub untracked_label: Label,
    pub untracked: Option<Diff>,

    pub staged: Option<Diff>,
    pub unstaged: Option<Diff>,

    pub conflicted: Option<Diff>,

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
            // untracked_spacer: Label::from_string(""),
            // untracked_label: Label::from_string(
            //     "<span weight=\"bold\" color=\"#8b6508\">Untracked files</span>",
            // ),
            untracked: None,
            // staged_spacer: Label::from_string(""),
            // staged_label: Label::from_string(
            //     "<span weight=\"bold\" color=\"#8b6508\">Staged changes</span>",
            // ),
            staged: None,
            // unstaged_spacer: Label::from_string(""),
            // unstaged_label: Label::from_string(
            //     "<span weight=\"bold\" color=\"#8b6508\">Unstaged changes</span>",
            // ),
            unstaged: None,
            // conflicted_spacer: Label::from_string(""),
            // conflicted_label: Label::from_string(
            //     "<span weight=\"bold\" color=\"#ff0000\">Conflicts</span>",
            // ),
            conflicted: None,
            // rendered: false,
            stashes: None,
            monitor_global_lock: Rc::new(RefCell::new(false)),
            monitor_lock: Rc::new(RefCell::new(HashSet::new())),
            settings,
        }
    }

    pub fn file_at_cursor(&self) -> Option<&GitFile> {
        for diff in [&self.staged, &self.unstaged, &self.conflicted] {
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
            // cleanup everything here. all diffs will be updated in get_status
            // IT DOES NOT WORK. garbage remains in stage
            // self.head.take();
            // self.upstream.take();
            // self.state.take();
            // self.staged.take();
            // self.unstaged.take();
            // self.conflicted.take();
            // self.stashes.take();

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
            debug!("???????????????? {:?}", upstream.branch);
            return upstream.branch.clone();
        }
        if let Some(head) = &self.head {
            return format!("{}", &head.branch);
        }
        String::from("master")
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

    pub fn update_head<'a>(
        &'a mut self,
        head: Head,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        if let Some(current_head) = &self.head {
            head.enrich_view(current_head, &txt.buffer(), context);
        }
        self.head.replace(head);
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_upstream<'a>(
        &'a mut self,
        upstream: Option<Head>,
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
        self.upstream = upstream;
        self.render(txt, RenderSource::Git, context);
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
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_untracked<'a>(
        &'a mut self,
        mut untracked: Option<Diff>,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        let mut settings =
            self.settings.get::<HashMap<String, Vec<String>>>("ignored");

        let repo_path = self.path.clone().unwrap();
        let str_path = repo_path.as_str();
        let mut has_files = true;
        if let Some(ignored) = settings.get_mut(str_path) {
            if let Some(new) = &mut untracked {
                new.files.retain(|f| {
                    let str_path = f.path.as_str();
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
                debug!("eeeeeeeerrrrrrrrrase untracked!");
                rendered.erase(buffer, context);
            }
        }
        self.untracked = untracked;
        if self.untracked.is_some() || render_required {
            self.render(txt, RenderSource::GitDiff, context);
        }
    }

    pub fn track_changes(&self, file_path: PathBuf, sender: Sender<Event>) {
        gio::spawn_blocking({
            let path = self.path.clone().unwrap();
            let sender = sender.clone();
            debug!(
                "track changes.................... {:?} {:?}",
                path, file_path
            );
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
            debug!(
                "************call track changes in status_view has_conflicted? {:?} interhunk {:?}",
                has_conflicted, interhunk
            );
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
        // TODO restore it!
        // if !diff.is_empty()
        //     && !diff.has_conflicts()
        //     && !self.conflicted_label.content.contains("resolved")
        // {
        //     self.conflicted_label.content = String::from("<span weight=\"bold\"\
        //                                                   color=\"#1c71d8\">Conflicts resolved</span> \
        //                                                   stage changes to complete merge");
        //     // both dirty and transfer is required.
        //     // only dirty means TagsModified state in render

        //     self.conflicted_label.view.dirty(true);
        //     self.conflicted_label.view.transfer(true);
        // }
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
                    // TODO restore it!
                    // restore original label for future conflicts
                    // self.conflicted_label.content = String::from(
                    //     "<span weight=\"bold\" color=\"#ff0000\">Conflicts</span>",
                    // );
                    // self.conflicted_label.view.dirty(true);
                }
                if state.need_final_commit() || state.need_rebase_continue() {
                    banner.set_title(&state.title_for_proceed_banner());
                    banner.set_css_classes(&["success"]);
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
        self.render(txt, RenderSource::Git, context);
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
            self.render(txt, RenderSource::GitDiff, context);
        }
    }

    pub fn update_unstaged<'a>(
        &'a mut self,
        diff: Option<Diff>,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        let _buffer = &txt.buffer();
        // works. looks ugly
        // if let Some(rendered) = &mut self.unstaged {
        //     rendered.adopt_other(
        //         diff.as_ref().map(|x| x as &dyn ViewContainer),
        //         buffer,
        //         context
        //     );
        // }

        // works. looks ugly
        // diff.as_ref().map(|d| d as &dyn ViewContainer).enrich_view(
        //         self.unstaged.as_ref().map(|d| d as &dyn ViewContainer),
        //         buffer,
        //         context
        //     );

        // original
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
        if self.unstaged.is_some() || render_required {
            self.render(txt, RenderSource::GitDiff, context);
        }
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
            // debug!(
            //     "--------------- updated file {:?} ----------",
            //     updated_file
            // );
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
                        debug!("enriiiiiiiiiiiiiiiiiiiiiiiich rendered file");
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
            debug!(
                "----------thats rendered files after enriching{:} {:?}",
                &rendered.files.len(),
                insert_ind
            );
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
        self.render(txt, RenderSource::GitDiff, context);
    }

    pub fn resize_highlights<'a>(
        &'a self,
        txt: &StageView,
        ctx: &mut StatusRenderContext<'a>,
    ) {
        let buffer = txt.buffer();
        let iter = buffer.iter_at_offset(buffer.cursor_position());
        self.cursor(txt, iter.line(), iter.offset(), ctx);
        glib::source::timeout_add_local(Duration::from_millis(10), {
            let txt = txt.clone();
            let mut context = StatusRenderContext::new();
            context.cursor = ctx.cursor;
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
        context.cursor = line_no;

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
        // NO NEED TO RENDER!
        txt.bind_highlights(context);
        changed
    }

    // Status
    pub fn expand<'a>(
        &'a self,
        txt: &StageView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext<'a>,
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
            }
        }
    }

    pub fn render<'a>(
        &'a self,
        txt: &StageView,
        source: RenderSource,
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
            // if untracked.files.is_empty() {
            //     // hack :( TODO - get rid of it
            //     self.untracked_spacer.view.squash(true);
            //     self.untracked_label.view.squash(true);
            // }
            // self.untracked_spacer.render(&buffer, &mut iter, context);
            // self.untracked_label.render(&buffer, &mut iter, context);
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
            self.cursor(txt, iter.line(), iter.offset(), context);
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
        }
        iter
    }

    pub fn ignore<'a>(
        &'a mut self,
        txt: &StageView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext<'a>,
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
                        self.untracked.clone(),
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
        info!("Stage in conflict");
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
                                let interhunk = conflicted.interhunk;
                                async move {
                                    if hunk.conflict_markers_count > 0
                                        && line.is_side_of_conflict()
                                    {
                                        info!("choose_conflict_side_of_hunk");
                                        gio::spawn_blocking({
                                            move || {
                                                merge::choose_conflict_side_of_hunk(
                                                    path, file_path, hunk, line,
                                                    interhunk, sender,
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
                                        info!(
                                            "cleanup_last_conflict_for_file"
                                        );
                                        gio::spawn_blocking({
                                            move || {
                                                merge::cleanup_last_conflict_for_file(
                                                    path, file_path, interhunk, sender,
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
            self.sender
                .send_blocking(Event::Toast(String::from("No file to stage")))
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
            let hunk = hunk.map(|h| h.header.clone());
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
    pub fn dump<'a>(
        &'a mut self,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        let mut path = self.path.clone().unwrap();
        path.push(DUMP_DIR);
        let create_result = std::fs::create_dir(&path);
        match create_result {
            Ok(_) => {}
            Err(err) => if err.kind() == ErrorKind::AlreadyExists {},
            Err(err) => {
                panic!("Error {}", err);
            }
        }
        let datetime: DateTime<Utc> = std::time::SystemTime::now().into();
        let fname = format!("dump_{}.txt", datetime.format("%d_%m_%Y_%T"));
        path.push(fname);
        let mut file = std::fs::File::create(path).unwrap();

        let buffer = txt.buffer();

        let pos = buffer.cursor_position();
        let iter = buffer.iter_at_offset(pos);
        self.cursor(txt, iter.line(), iter.offset(), context);
        self.render(txt, RenderSource::Git, context);

        let iter = buffer.iter_at_offset(0);
        let end_iter = buffer.end_iter();
        let content = buffer.text(&iter, &end_iter, true);
        file.write_all(content.as_bytes()).unwrap();
        file.write_all("\n ================================= \n".as_bytes())
            .unwrap();
        file.write_all(format!("context: {:?}", context).as_bytes())
            .unwrap();
        if let Some(conflicted) = &self.conflicted {
            file.write_all(
                "\n ==============Coflicted================= \n".as_bytes(),
            )
            .unwrap();
            file.write_all(conflicted.dump().as_bytes()).unwrap();
        }
        if let Some(unstaged) = &self.unstaged {
            file.write_all(
                "\n ==============UnStaged================= \n".as_bytes(),
            )
            .unwrap();
            file.write_all(unstaged.dump().as_bytes()).unwrap();
        }
        if let Some(staged) = &self.staged {
            file.write_all(
                "\n ==============Staged================= \n".as_bytes(),
            )
            .unwrap();
            file.write_all(staged.dump().as_bytes()).unwrap();
        }
        self.sender
            .send_blocking(Event::Toast(String::from("dumped")))
            .expect("cant send through sender");
    }
    pub fn head_oid(&self) -> crate::Oid {
        self.head.as_ref().unwrap().oid
    }

    pub fn debug<'a>(
        &'a mut self,
        txt: &StageView,
        context: &mut StatusRenderContext<'a>,
    ) {
        self.render(txt, RenderSource::GitDiff, context);
        // let buffer = txt.buffer();
        // let pos = buffer.cursor_position();
        // let iter = buffer.iter_at_offset(pos);
        // let (y, height) = txt.line_yrange(&iter);
        // debug!("+++++++++++++++++++++++++ y {:?} height {:?}", y, height);

        // self.render(txt, RenderSource::Git, context);
        // let (line_from, line_to) = context.highlight_lines.unwrap();
        // let mut iter = buffer.iter_at_line(line_from).unwrap();
        // let y_from = txt.line_yrange(&iter).0;
        // iter.set_line(line_to);
        // let (y, height) = txt.line_yrange(&iter);
        // debug!("+++++++++++++++++++++++++ line_from {:?} line_to {:?} y_from {:?} y {:?} height {:?}",
        //        line_from, line_to, y_from, y, height
        // );
        // let me = buffer.cursor_position();
        // let iter = buffer.iter_at_offset(me);
        // let (y, height) = txt.line_yrange(&iter);
        // debug!(
        //     "and thats me on line {:?} y {:?} height {:?}",
        //     iter.line(),
        //     y,
        //     height
        // );
    }
}
