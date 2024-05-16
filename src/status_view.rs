pub mod container;
pub mod headerbar;
pub mod textview;
use container::{ViewContainer, ViewKind};
use crate::git::{merge, LineKind};
use core::time::Duration;

pub mod render;
use textview::Tag;
pub mod reconciliation;
pub mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{
    checkout_oid, commit, get_current_repo_status, get_directories, pull,
    push, reset_hard, stage_untracked, stage_via_apply, stash_changes, merge_dialog_factory,
    track_changes, git_debug, UnderCursor,
    ApplyFilter, ApplySubject, Diff, Event, Head, Stashes,
    State, StatusRenderContext, Untracked, View, OURS, THEIRS, ABORT, PROCEED
};

use async_channel::Sender;

use gio::{
    Cancellable, File, FileMonitor, FileMonitorEvent, FileMonitorFlags,
};

use gtk4::prelude::*;
use gtk4::{
    gio, glib, Box, Label as GtkLabel, ListBox, Orientation, SelectionMode,
    TextBuffer, TextView, Widget
};
use glib::clone;
use glib::signal::SignalHandlerId;
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, EntryRow, PasswordEntryRow, SwitchRow, Banner}; // _Window,
use log::{debug, trace, info};

use std::ffi::OsString;

#[derive(Debug, Clone, Default)]
pub struct Label {
    content: String,
    view: View,
}
impl Label {
    pub fn from_string(content: &str) -> Self {
        Label {
            content: String::from(content),
            view: View::new_markup(),
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
    pub path: Option<OsString>,
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
    // pub context: Option<StatusRenderContext>,
    pub stashes: Option<Stashes>,
    pub monitor_lock: Rc<RefCell<bool>>,
    pub settings: gio::Settings,
}

impl Status {
    pub fn new(
        path: Option<OsString>,
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
            // context: None::<StatusRenderContext>,
            stashes: None,
            monitor_lock: Rc::new(RefCell::<bool>::new(false)),
            settings
        }
    }

    pub fn head_title(&self) -> String {
        if let Some(head) = &self.head {
            return format!("On {}: {}", &head.branch, &head.commit);
        }
        String::from("there are no head")
    }

    pub fn update_path(
        &mut self,
        path: OsString,
        monitors: Rc<RefCell<Vec<FileMonitor>>>,
        user_action: bool,
    ) {
        // here could come path selected by the user
        // this is 'dirty' one. The right path will
        // came from git with /.git/ suffix
        // but the 'dirty' path will be used first
        // for querying repo status and investigate real one
        let str_path = path.clone().into_string().unwrap();
        if user_action {
            monitors.borrow_mut().retain(|fm: &FileMonitor| {
                fm.cancel();
                false
            });
        } else {
            // investigated path
            assert!(str_path.contains("/.git/"));
            if self.path.is_none() || path != self.path.clone().unwrap() {
                let mut paths = self.settings.get::<Vec<String>>("paths");
                let str_path =
                    path.clone().into_string().unwrap().replace(".git/", "");
                self.settings
                    .set("lastpath", str_path.clone())
                    .expect("cant set lastpath");
                if !paths.contains(&str_path) {
                    paths.push(str_path);
                    self.settings
                        .set("paths", paths)
                        .expect("cant set settings");
                }
                self.setup_monitors(monitors, path.clone());
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
        path: OsString,
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
                                FileMonitorEvent::ChangesDoneHint => {
                                    let file_path = file
                                        .path()
                                        .expect("no file path")
                                        .into_os_string();
                                    let str_file_path = file_path
                                        .clone()
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
                                        "file event in monitor {:?}",
                                        event
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

    pub fn reset_hard(&self, _sender: Sender<Event>) {
        gio::spawn_blocking({
            let path = self.path.clone().expect("np path");
            let sender = self.sender.clone();
            move || {
                reset_hard(path, sender);
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
                        pull(path, sender, user_pass);
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
                gio::spawn_blocking({
                    move || {
                        push(
                            path.expect("no path"),
                            remote_branch_name,
                            track_remote,
                            sender,
                            user_pass,
                        );
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
        if self.staged.is_some() {
            glib::spawn_future_local({
                let window = window.clone();
                let sender = self.sender.clone();
                let path = self.path.clone();
                async move {
                    let lb = ListBox::builder()
                        .selection_mode(SelectionMode::None)
                        .css_classes(vec![String::from("boxed-list")])
                        .build();
                    let input = EntryRow::builder()
                        .title("Commit message:")
                        .show_apply_button(true)
                        .css_classes(vec!["input_field"])
                        .build();
                    lb.append(&input);
                    let dialog = crate::confirm_dialog_factory(
                        &window,
                        Some(&lb),
                        "Commit",
                        "Commit",
                    );
                    input.connect_apply(
                        clone!(@strong dialog as dialog => move |_entry| {
                            // someone pressed enter
                            dialog.response("confirm");
                            dialog.close();
                        }),
                    );
                    let response = dialog.choose_future().await;
                    if "confirm" != response {
                        return;
                    }
                    gio::spawn_blocking({
                        let message = format!("{}", input.text());
                        move || {
                            commit(path.expect("no path"), message, sender);
                        }
                    });
                }
            });
        }
    }

    pub fn update_head(&mut self, mut head: Head, txt: &TextView, context: &mut StatusRenderContext) {
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
        context: &mut StatusRenderContext
    ) {
        if let Some(rendered) = &mut self.upstream {
            if let Some(new) = upstream.as_mut() {
                new.enrich_view(rendered);
            } else {
                rendered.erase(txt, &mut Some(context));
            }
        }
        self.upstream = upstream;
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_state(&mut self, mut state: State, txt: &TextView, context: &mut StatusRenderContext) {
        if let Some(current_state) = &self.state {
            state.enrich_view(current_state)
        }
        self.state.replace(state);
        self.render(txt, RenderSource::Git, context);
    }

    fn path_as_string(&self) -> String {
        self.path
            .clone()
            .expect("no path")
            .into_string()
            .expect("wrong string")
    }

    pub fn update_untracked(
        &mut self,
        mut untracked: Untracked,
        txt: &TextView,
        context: &mut StatusRenderContext
    ) {
        let mut settings =
            self.settings.get::<HashMap<String, Vec<String>>>("ignored");
        let repo_path = self.path_as_string();
        if let Some(ignored) = settings.get_mut(&repo_path) {
            untracked.files.retain(|f| {
                let str_path =
                    f.path.clone().into_string().expect("wrong string");
                !ignored.contains(&str_path)
            });
        }
        context.update_screen_line_width(untracked.max_line_len);
        if let Some(u) = &mut self.untracked {
            untracked.enrich_view(u, txt, &mut Some(context));
        }
        self.untracked.replace(untracked);
        self.render(txt, RenderSource::Git, context);
    }

    pub fn update_conflicted(&mut self,
                             mut diff: Diff,
                             txt: &TextView,
                             window: &ApplicationWindow,
                             sender: Sender<Event>,
                             banner: &Banner,
                             banner_button: &Widget,
                             banner_button_clicked: Rc<RefCell<Option<SignalHandlerId>>>,
                             context: &mut StatusRenderContext
    ) {
        context.update_screen_line_width(diff.max_line_len);
        if let Some(s) = &mut self.conflicted {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(s, txt, &mut Some(context));
        }
        if diff.is_empty() {
            if banner.is_revealed() {
                banner.set_revealed(false);
            }
            if let Some(state) = &self.state {
                if state.is_merging() {
                    banner.set_title("All conflicts fixed but you are still merging. Commit to conclude merge");
                    banner.set_css_classes(&vec!["success"]);
                    banner.set_button_label(Some("Commit"));
                    banner_button.set_css_classes(&vec!["suggested-action"]);
                    banner.set_revealed(true);
                    if let Some(handler_id) = banner_button_clicked.take() {
                        banner.disconnect(handler_id);
                    }
                    let new_handler_id = banner.connect_button_clicked({
                        let sender = sender.clone();
                        let path = self.path.clone();
                        move |_| {
                            let sender = sender.clone();
                            let path = path.clone();
                            gio::spawn_blocking({
                                move || {
                                    merge::commit(path.clone().expect("no path"));
                                    get_current_repo_status(path, sender);
                                }
                            });
                        }
                    });
                    banner_button_clicked.replace(Some(new_handler_id));
                }
            }
        }
        else {
            if !banner.is_revealed() {
                banner.set_title("Got conflicts while merging branch master");
                banner.set_css_classes(&vec!["error"]);
                banner.set_button_label(Some("Abort or Resolve All"));
                banner_button.set_css_classes(&vec!["destructive-action"]);
                banner.set_revealed(true);
                if let Some(handler_id) = banner_button_clicked.take() {
                    banner.disconnect(handler_id);
                }
                let new_handler_id = banner.connect_button_clicked({
                    let sender = sender.clone();
                    let window = window.clone();
                    let path = self.path.clone();
                    move |_| {
                        glib::spawn_future_local({
                            let window = window.clone();
                            let sender = sender.clone();
                            let path = path.clone();
                            async move {
                                let dialog = merge_dialog_factory(&window, sender.clone());
                                let response = dialog.choose_future().await;
                                match response.as_str() {
                                    ABORT => {
                                        info!("merge. abort");
                                        gio::spawn_blocking({
                                            move || {
                                                merge::abort(path.expect("no path"), sender);
                                            }
                                        });
                                    }
                                    OURS => {
                                        info!("merge. choose ours");
                                        gio::spawn_blocking({
                                            move || {
                                                merge::choose_conflict_side(path.expect("no path"), true, sender);
                                            }
                                        });
                                    }
                                    THEIRS => {
                                        info!("merge. choose theirs");
                                        gio::spawn_blocking({
                                            move || {
                                                merge::choose_conflict_side(path.expect("no path"), false, sender);
                                            }
                                        });
                                    }
                                    _ => {
                                        debug!("=============> proceed");
                                    }
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
    }

    pub fn update_staged(&mut self, mut diff: Diff, txt: &TextView, context: &mut StatusRenderContext) {
        context.update_screen_line_width(diff.max_line_len);
        if let Some(s) = &mut self.staged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(s, txt, &mut Some(context));
        }
        self.staged.replace(diff);
        // why check both??? perhaps just for very first render
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git, context);
        }
    }

    pub fn update_unstaged(&mut self, mut diff: Diff, txt: &TextView, context: &mut StatusRenderContext) {
        context.update_screen_line_width(diff.max_line_len);
        if let Some(u) = &mut self.unstaged {
            // hide untracked for now
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(u, txt, &mut Some(context));
        }
        self.unstaged.replace(diff);
        // why check both??? perhaps just for very first render
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git, context);
        }
    }
    // status
    pub fn cursor(&mut self, txt: &TextView, line_no: i32, offset: i32, context: &mut StatusRenderContext) {
        let mut changed = false;
        if let Some(untracked) = &mut self.untracked {
            changed = untracked.cursor(line_no, false, &mut Some(context)) || changed;
        }
        if let Some(conflicted) = &mut self.conflicted {
            context.under_cursor_diff(&conflicted.kind);
            changed = conflicted.cursor(line_no, false, &mut Some(context)) || changed;
        }
        if let Some(unstaged) = &mut self.unstaged {
            context.under_cursor_diff(&unstaged.kind);
            changed = unstaged.cursor(line_no, false, &mut Some(context)) || changed;
            debug!("+++++++++++++++++++++++ > {:?}", context);
        }
        if let Some(staged) = &mut self.staged {
            context.under_cursor_diff(&staged.kind);
            changed = staged.cursor(line_no, false, &mut Some(context)) || changed;
        }
        if changed {
            self.render(txt, RenderSource::Cursor(line_no), context);
            let buffer = txt.buffer();
            trace!("put cursor on line {:?} in CURSOR", line_no);
            buffer.place_cursor(&buffer.iter_at_offset(offset));
        }
    }

    // Status
    pub fn expand(&mut self, txt: &TextView, line_no: i32, _offset: i32, context: &mut StatusRenderContext) {
        // let mut changed = false;

        if let Some(conflicted) = &mut self.conflicted {
            for file in &mut conflicted.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(txt, RenderSource::Expand(expanded_line), context);
                    return;
                }
            }
        }

        if let Some(unstaged) = &mut self.unstaged {
            for file in &mut unstaged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(txt, RenderSource::Expand(expanded_line), context);
                    return;
                }
            }
        }
        if let Some(staged) = &mut self.staged {
            for file in &mut staged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(txt, RenderSource::Expand(expanded_line), context);
                    return;
                }
            }
        }
    }

    // Status
    pub fn render(&mut self, txt: &TextView, source: RenderSource, context: &mut StatusRenderContext) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);

        if let Some(head) = &mut self.head {
            head.render(&buffer, &mut iter, &mut Some(context));
        }

        if let Some(upstream) = &mut self.upstream {
            upstream.render(&buffer, &mut iter, &mut Some(context));
        }

        if let Some(state) = &mut self.state {
            state.render(&buffer, &mut iter, &mut Some(context));
        }

        if let Some(untracked) = &mut self.untracked {
            if untracked.files.is_empty() {
                self.untracked_spacer.view.squashed = true;
                self.untracked_label.view.squashed = true;
            }
            self.untracked_spacer.render(
                &buffer,
                &mut iter,
                &mut Some(context),
            );
            self.untracked_label
                .render(&buffer, &mut iter, &mut Some(context));
            untracked.render(&buffer, &mut iter, &mut Some(context));
        }

        if let Some(conflicted) = &mut self.conflicted {
            if conflicted.files.is_empty() {
                self.conflicted_spacer.view.squashed = true;
                self.conflicted_label.view.squashed = true;
            }
            self.conflicted_spacer
                .render(&buffer, &mut iter, &mut Some(context));
            self.conflicted_label
                .render(&buffer, &mut iter, &mut Some(context));
            conflicted.render(&buffer, &mut iter, &mut Some(context));
        }

        if let Some(unstaged) = &mut self.unstaged {
            if unstaged.files.is_empty() {
                self.unstaged_spacer.view.squashed = true;
                self.unstaged_label.view.squashed = true;
            }
            self.unstaged_spacer
                .render(&buffer, &mut iter, &mut Some(context));
            self.unstaged_label
                .render(&buffer, &mut iter, &mut Some(context));
            unstaged.render(&buffer, &mut iter, &mut Some(context));
        }

        if let Some(staged) = &mut self.staged {
            if staged.files.is_empty() {
                self.staged_spacer.view.squashed = true;
                self.staged_label.view.squashed = true;
            }
            self.staged_spacer
                .render(&buffer, &mut iter, &mut Some(context));
            self.staged_label
                .render(&buffer, &mut iter, &mut Some(context));
            staged.render(&buffer, &mut iter, &mut Some(context));
        }
        trace!("render source {:?}", source);
        match source {
            RenderSource::Cursor(_) => {
                // avoid loops on cursor renders
                trace!("avoid cursor position on cursor");
            }
            RenderSource::Expand(line_no) => {
                self.choose_cursor_position(txt, &buffer, Some(line_no), context);
            }
            RenderSource::Git => {
                self.choose_cursor_position(txt, &buffer, None, context);
            }
            RenderSource::Resize => {}
        };
    }

    pub fn resize(&mut self, txt: &TextView, context: &mut StatusRenderContext) {
        // it need to rerender all highlights and
        // background to match new window size
        if let Some(diff) = &mut self.staged {
            diff.resize(txt, &mut Some(context))
        }
        if let Some(diff) = &mut self.unstaged {
            diff.resize(txt, &mut Some(context))
        }
        self.render(txt, RenderSource::Resize, context);
    }

    pub fn ignore(&mut self, txt: &TextView, line_no: i32, _offset: i32, context: &mut StatusRenderContext) {
        if let Some(untracked) = &mut self.untracked {
            for file in &mut untracked.files {
                // TODO!
                // refactor to some generic method
                // why other elements do not using this?
                let view = file.get_view();
                if view.current && view.line_no == line_no {
                    let ignore_path =
                        file.path.clone().into_string().expect("wrong string");
                    trace!("ignore path! {:?}", ignore_path);
                    let mut settings =
                        self.settings
                            .get::<HashMap<String, Vec<String>>>("ignored");
                    let repo_path = self.path_as_string();
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
                        context
                    );
                    break;
                }
            }
        }
    }

    pub fn stage(
        &mut self,
        _txt: &TextView,
        _line_no: i32,
        subject: ApplySubject,
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
        if let Some(conflicted) = &self.conflicted {
            for f in &conflicted.files {
                for hunk in &f.hunks {
                    for line in &hunk.lines {
                        if line.view.current && (line.kind == LineKind::Ours || line.kind == LineKind::Theirs){
                                    gio::spawn_blocking({
                                        let path = self.path.clone().unwrap();
                                        let sender = self.sender.clone();
                                        let file_path = f.path.clone();
                                        let hunk = hunk.clone();
                                        let line = line.clone();
                                        move || {
                                            merge::choose_conflict_side_of_hunk(path, file_path, hunk, line, sender);
                                            // merge::choose_conflict_side_once(path, file_path, hunk_header, origin, sender);
                                        }
                                    });
                                    return;
                            // }
                        }
                    }
                }
            }
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
        if !filter.file_id.is_empty() {
            if hunks_staged > 1 {
                // stage all hunks in file
                filter.hunk_id = None;
            }
            debug!("stage via apply {:?}", filter);
            gio::spawn_blocking({
                let path = self.path.clone();
                let sender = self.sender.clone();
                move || {
                    stage_via_apply(path.expect("no path"), filter, sender);
                }
            });
        }
    }

    pub fn choose_cursor_position(
        &mut self,
        txt: &TextView,
        buffer: &TextBuffer,
        line_no: Option<i32>,
        context: &mut StatusRenderContext
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
                    trace!(
                        "choose cursor at first unstaged file {:?}",
                        line_no
                    );
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
                    debug!("is rendered in {:?} {:?}", view.is_rendered_in(current_line), current_line);
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
