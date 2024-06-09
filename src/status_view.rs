pub mod tags;
pub mod commit;
pub mod container;
pub mod headerbar;
pub mod textview;
use crate::dialogs::{alert, DangerDialog, YES};
use crate::git::{merge, remote};
use container::{ViewContainer, ViewKind};
use core::time::Duration;
use git2::RepositoryState;

pub mod render;
pub mod reconciliation;
pub mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::status_view::render::View;
use crate::{
    checkout_oid, get_current_repo_status, get_directories, git_debug,
    stage_untracked, stage_via_apply, stash_changes, track_changes,
    ApplyFilter, ApplySubject, Diff, Event, Head, Stashes, State,
    StatusRenderContext, Untracked,
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
    TextBuffer, TextView, Widget,
};
use libadwaita::prelude::*;
use libadwaita::{
    ApplicationWindow, Banner, EntryRow, PasswordEntryRow, SwitchRow,
}; // _Window,
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
    Cursor(i32),
    Expand(i32),
    Resize,
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
    pub stashes: Option<Stashes>,
    pub monitor_lock: Rc<RefCell<bool>>,
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
            monitor_lock: Rc::new(RefCell::<bool>::new(false)),
            settings
        }
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
        self.monitor_lock.replace(lock);
    }

    pub fn setup_monitors(
        &mut self,
        monitors: Rc<RefCell<Vec<FileMonitor>>>,
        path: PathBuf,
    ) {
        glib::spawn_future_local({
            let sender = self.sender.clone();
            let lock = self.monitor_lock.clone();
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
                        move |_monitor, file, _other_file, event| {
                            // TODO get from SELF.settings
                            let patterns_to_exclude: Vec<&str> =
                                vec!["/.#", "/mout", "flycheck_", "/sed"];
                            match event {
                                FileMonitorEvent::Changed => {
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
                                    if *lock.borrow() {
                                        trace!("monitor locked");
                                        return;
                                    }
                                    lock.replace(true);
                                    trace!("set monitor lock");
                                    glib::source::timeout_add_local(
                                        Duration::from_millis(300),
                                        {
                                            let lock = lock.clone();
                                            let path = path.clone();
                                            let sender = sender.clone();
                                            let file_path = file_path.clone();
                                            move || {
                                                gio::spawn_blocking({
                                                    let path = path.clone();
                                                    let sender =
                                                        sender.clone();
                                                    let file_path =
                                                        file_path.clone();
                                                    lock.replace(false);
                                                    trace!(
                                                        "release monitor lock"
                                                    );
                                                    move || {
                                                        // TODO! throttle!
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
                debug!("my monitors a set {:?}", monitors.borrow().len());
            }
        });
    }

    pub fn update_stashes(&mut self, stashes: Stashes) {
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
        &mut self,
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
        &mut self,
        window: &ApplicationWindow, // &impl IsA<Gtk4Window>,
    ) {
        let mut amend_message: Option<String> = None;
        if let Some(head) = &self.head {
            if let Some(upstream) = &self.upstream {
                if head.oid != upstream.oid {
                    amend_message.replace(head.commit_body.clone());
                }
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
        txt: &TextView,
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
        txt: &TextView,
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
        txt: &TextView,
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
        txt: &TextView,
        context: &mut StatusRenderContext,
    ) {
        let mut settings =
            self.settings.get::<HashMap<String, Vec<String>>>("ignored");
        let repo_path = self
            .path
            .clone()
            .expect("no path")
            .into_os_string()
            .into_string()
            .expect("wrong path");
        if let Some(ignored) = settings.get_mut(&repo_path) {
            untracked.files.retain(|f| {
                let str_path = f
                    .path
                    .clone()
                    .into_os_string()
                    .into_string()
                    .expect("wrong string");
                !ignored.contains(&str_path)
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
        txt: &TextView,
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
                self.conflicted_label.view.dirty = true;
            }
            diff.enrich_view(s, &txt.buffer(), context);
        }
        if let Some(state) = &self.state {
            if diff.is_empty() {
                if banner.is_revealed() {
                    banner.set_revealed(false);
                }

                if state.need_final_commit() {
                    banner.set_title(&state.title_for_proceed_banner());
                    banner.set_css_classes(&["success"]);
                    banner.set_button_label(Some("Commit"));
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
                                            if state == RepositoryState::Merge
                                            {
                                                merge::final_merge_commit(
                                                    path.clone()
                                                        .expect("no path"),
                                                    sender,
                                                )
                                            } else {
                                                merge::final_commit(
                                                    path.clone()
                                                        .expect("no path"),
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
                    move |_| {
                        gio::spawn_blocking({
                            let sender = sender.clone();
                            let path = path.clone();
                            move || {
                                merge::abort(path.expect("no path"), sender);
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
        self.conflicted_label.view.dirty = true;
    }

    pub fn update_staged(
        &mut self,
        mut diff: Diff,
        txt: &TextView,
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
        // why check both??? perhaps just for very first render
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git, context);
        }
    }

    pub fn update_unstaged(
        &mut self,
        mut diff: Diff,
        txt: &TextView,
        context: &mut StatusRenderContext,
    ) {
        if let Some(u) = &mut self.unstaged {
            // hide untracked for now
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(u, &txt.buffer(), context);
        }
        self.unstaged.replace(diff);
        // why check both??? perhaps just for very first render
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git, context);
        }
    }
    // status
    pub fn cursor(
        &mut self,
        txt: &TextView,
        line_no: i32,
        offset: i32,
        context: &mut StatusRenderContext,
    ) {
        context.update_cursor_pos(line_no, offset);
        let mut changed = false;
        if let Some(untracked) = &mut self.untracked {
            changed = untracked.cursor(line_no, false, context) || changed;
        }
        if let Some(conflicted) = &mut self.conflicted {
            changed = conflicted.cursor(line_no, false, context) || changed;
        }
        if let Some(unstaged) = &mut self.unstaged {
            changed = unstaged.cursor(line_no, false, context) || changed;
        }
        if let Some(staged) = &mut self.staged {
            changed = staged.cursor(line_no, false, context) || changed;
        }
        if changed {
            self.render(txt, RenderSource::Cursor(line_no), context);
            // let buffer = txt.buffer();
            // let iter = &buffer.iter_at_offset(offset);
            // buffer.place_cursor(iter);
        }
    }

    // Status
    pub fn expand(
        &mut self,
        txt: &TextView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext,
    ) {
        // let mut changed = false;

        if let Some(conflicted) = &mut self.conflicted {
            for file in &mut conflicted.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(
                        txt,
                        RenderSource::Expand(expanded_line),
                        context,
                    );
                    return;
                }
            }
        }

        if let Some(unstaged) = &mut self.unstaged {
            for file in &mut unstaged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(
                        txt,
                        RenderSource::Expand(expanded_line),
                        context,
                    );
                    return;
                }
            }
        }
        if let Some(staged) = &mut self.staged {
            for file in &mut staged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(
                        txt,
                        RenderSource::Expand(expanded_line),
                        context,
                    );
                    return;
                }
            }
        }
    }

    // Status
    pub fn render(
        &mut self,
        txt: &TextView,
        source: RenderSource,
        context: &mut StatusRenderContext,
    ) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);

        if let Some(head) = &mut self.head {
            head.render(&buffer, &mut iter, context);
        }

        if let Some(upstream) = &mut self.upstream {
            upstream.render(&buffer, &mut iter, context);
        }

        if let Some(state) = &mut self.state {
            state.render(&buffer, &mut iter, context);
        }

        if let Some(untracked) = &mut self.untracked {
            if untracked.files.is_empty() {
                // hack :( TODO - get rid of it
                self.untracked_spacer.view.squash(true);
                self.untracked_label.view.squash(true);
            }
            self.untracked_spacer.render(&buffer, &mut iter, context);
            self.untracked_label.render(&buffer, &mut iter, context);
            untracked.render(&buffer, &mut iter, context);
        }

        if let Some(conflicted) = &mut self.conflicted {
            if conflicted.files.is_empty() {
                self.conflicted_spacer.view.squash(true);
                self.conflicted_label.view.squash(true);
            }
            self.conflicted_spacer.render(&buffer, &mut iter, context);
            self.conflicted_label.render(&buffer, &mut iter, context);
            conflicted.render(&buffer, &mut iter, context);
        }

        if let Some(unstaged) = &mut self.unstaged {
            if unstaged.files.is_empty() {
                // hack :(
                self.unstaged_spacer.view.squash(true);
                self.unstaged_label.view.squash(true);
            }
            self.unstaged_spacer.render(&buffer, &mut iter, context);
            self.unstaged_label.render(&buffer, &mut iter, context);
            unstaged.render(&buffer, &mut iter, context);
        }

        if let Some(staged) = &mut self.staged {
            if staged.files.is_empty() {
                // hack :(
                self.staged_spacer.view.squash(true);
                self.staged_label.view.squash(true);
            }
            self.staged_spacer.render(&buffer, &mut iter, context);
            self.staged_label.render(&buffer, &mut iter, context);
            staged.render(&buffer, &mut iter, context);
        }
        trace!("render source {:?}", source);
        match source {
            RenderSource::Cursor(_) => {
                // avoid loops on cursor renders
                trace!("avoid cursor position on cursor");
            }
            RenderSource::Expand(line_no) => {
                self.choose_cursor_position(
                    txt,
                    &buffer,
                    Some(line_no),
                    context,
                );
            }
            RenderSource::Git => {
                self.choose_cursor_position(txt, &buffer, None, context);
            }
            RenderSource::Resize => {}
        };
    }

    pub fn resize(
        &mut self,
        txt: &TextView,
        context: &mut StatusRenderContext,
    ) {
        // it need to rerender all highlights and
        // background to match new window size
        if let Some(diff) = &mut self.staged {
            diff.resize(&txt.buffer(), context)
        }
        if let Some(diff) = &mut self.unstaged {
            diff.resize(&txt.buffer(), context)
        }
        self.render(txt, RenderSource::Resize, context);
    }

    pub fn ignore(
        &mut self,
        txt: &TextView,
        line_no: i32,
        _offset: i32,
        context: &mut StatusRenderContext,
    ) {
        if let Some(untracked) = &mut self.untracked {
            for file in &mut untracked.files {
                // TODO!
                // refactor to some generic method
                // why other elements do not using this?
                let view = file.get_view();
                if view.current && view.line_no == line_no {
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
                        if line.view.current {
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

    pub fn stage(
        &mut self,
        _txt: &TextView,
        _line_no: i32,
        subject: ApplySubject,
        window: &ApplicationWindow,
    ) {
        if let Some(untracked) = &mut self.untracked {
            for file in &mut untracked.files {
                if file.get_view().current {
                    gio::spawn_blocking({
                        let path = self.path.clone();
                        let sender = self.sender.clone();
                        let file = file.clone();
                        move || {
                            stage_untracked(
                                path.expect("no path"),
                                file,
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
        match subject {
            ApplySubject::Stage | ApplySubject::Kill => {
                if self.unstaged.is_none() {
                    return;
                }
            }
            ApplySubject::Unstage => {
                if self.staged.is_none() {
                    return;
                }
            }
        }

        let diff = {
            match subject {
                ApplySubject::Stage | ApplySubject::Kill => {
                    self.unstaged.as_mut().unwrap()
                }
                ApplySubject::Unstage => self.staged.as_mut().unwrap(),
            }
        };
        let mut filter = ApplyFilter::new(subject);
        let mut file_path_so_stage = String::new();
        let mut hunks_staged = 0;
        // there could be either file with all hunks
        // or just 1 hunk
        diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let id = vc.get_id();
            let kind = vc.get_kind();
            let view = vc.get_view();
            trace!("walks down on apply {:} {:?}", id, kind);
            match kind {
                ViewKind::File => {
                    // just store current file_path
                    // in this loop. temporary variable
                    file_path_so_stage = id;
                }
                ViewKind::Hunk => {
                    if !view.active {
                        return;
                    }
                    // store active hunk in filter
                    // if the cursor is on file, all
                    // hunks under it will be active
                    filter.file_id = file_path_so_stage.clone();
                    filter.hunk_id.replace(id);
                    hunks_staged += 1;
                }
                _ => (),
            }
        });
        debug!("apply filter ----------------------> {:?}", filter);
        if !filter.file_id.is_empty() {
            if hunks_staged > 1 {
                // stage all hunks in file
                filter.hunk_id = None;
            }
            trace!("stage via apply {:?}", filter);
            glib::spawn_future_local({
                let window = window.clone();
                let path = self.path.clone();
                let sender = self.sender.clone();
                async move {
                    gio::spawn_blocking({
                        move || {
                            stage_via_apply(
                                path.expect("no path"),
                                filter,
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
    }

    pub fn choose_cursor_position(
        &mut self,
        txt: &TextView,
        buffer: &TextBuffer,
        line_no: Option<i32>,
        context: &mut StatusRenderContext,
    ) {
        let offset = buffer.cursor_position();
        trace!("choose_cursor_position. optional line {:?}. offset {:?}, line at offset {:?}",
               line_no,
               offset,
               buffer.iter_at_offset(offset).line()
        );
        if offset == buffer.end_iter().offset() {
            // first render. buffer at eof
            if let Some(unstaged) = &self.unstaged {
                if !unstaged.files.is_empty() {
                    let line_no = unstaged.files[0].view.line_no;
                    let iter = buffer.iter_at_line(line_no).unwrap();
                    debug!(
                        "ccccccccccccccccccccccccccccccccccchoose cursor at first unstaged file {:?}",
                        line_no
                    );
                    buffer.place_cursor(&iter);
                    self.cursor(txt, line_no, iter.offset(), context);
                    return;
                }
            }
        }
        let mut iter = buffer.iter_at_offset(offset);
        iter.backward_line();
        iter.forward_lines(1);
        // after git op view could be shifted.
        // cursor is on place and it is visually current,
        // but view under it is not current, cause line_no differs
        trace!("choose cursor when NOT on eof {:?}", iter.line());
        buffer.place_cursor(&iter);
        self.cursor(txt, iter.line(), iter.offset(), context);
    }

    pub fn has_staged(&self) -> bool {
        if let Some(staged) = &self.staged {
            return !staged.files.is_empty();
        }
        false
    }
    pub fn debug(&mut self, txt: &TextView) {
        let buffer = txt.buffer();
        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let current_line = iter.line();

        debug!("debug at line {:?}", current_line);
        if let Some(diff) = &mut self.staged {
            diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let content = vc.get_content();
                let view = vc.get_view();
                if view.line_no == current_line && view.rendered {
                    debug!("view under line {:?} {:?}", view.line_no, content);
                    debug!(
                        "is rendered in {:?} {:?}",
                        view.is_rendered_in(current_line),
                        current_line
                    );
                    dbg!(view);
                }
            });
        }
        if let Some(diff) = &mut self.unstaged {
            diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let content = vc.get_content();
                let view = vc.get_view();
                if view.line_no == current_line {
                    println!("found view {:?}", content);
                    dbg!(view);
                }
            });
        }
        gio::spawn_blocking({
            let path = self.path.clone().expect("no path");
            move || {
                git_debug(path);
            }
        });
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
                            stash_changes(
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
