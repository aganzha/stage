pub mod container;
pub mod factory;
use container::{ViewContainer, ViewKind};

pub mod render;
use render::Tag;
pub mod reconciliation;
pub mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    commit, get_current_repo_status, get_directories, pull, push, reset_hard,
    stage_untracked, stage_via_apply, track_changes, ApplyFilter,
    ApplySubject, Diff, DiffKind, Event, Head, Stashes, State, Untracked,
    View, StatusRenderContext
};

use async_channel::Sender;

use gio::{
    Cancellable, File, FileMonitor, FileMonitorEvent, FileMonitorFlags,
};
use glib::clone;
use gtk4::prelude::*;
use gtk4::{
    gio, glib, ListBox, SelectionMode, TextBuffer, TextView,
    Window as Gtk4Window,
};
use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, EntryRow, SwitchRow, PasswordEntryRow}; // _Window,
use log::{debug, trace};

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

    pub rendered: bool, // what it is for ????
    pub context: Option<StatusRenderContext>,
    pub stashes: Option<Stashes>,
    // pub monitors: Vec<FileMonitor>,
}

impl Status {
    pub fn new(path: Option<OsString>, sender: Sender<Event>) -> Self {
        Self {
            path: path,
            sender: sender,
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
            rendered: false,
            context: None::<StatusRenderContext>,
            stashes: None,
            // monitors: Vec::new()
        }
    }

    pub fn update_path(
        &mut self,
        path: OsString,
        monitors: Rc<RefCell<Vec<FileMonitor>>>,
    ) {
        if self.path.is_some() {
            panic!("got one more path ? {:?} {:?}", self.path, path);
        }
        self.path.replace(path);
        self.setup_monitor(monitors);
    }

    pub fn setup_monitor(&mut self, monitors: Rc<RefCell<Vec<FileMonitor>>>) {
        if let Some(_) = &self.path {
            glib::spawn_future_local({
                let path = self.path.clone().expect("no path");
                let sender = self.sender.clone();
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
                        let dir_name = match dir {
                            name if name == root => name,
                            name => {
                                format!("{}/{}", root, name)
                            }
                        };
                        debug!("setup monitor {:?}", dir_name);
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
                            move |_monitor, file, _other_file, event| {
                                match event {
                                    FileMonitorEvent::ChangesDoneHint => {
                                        // TODO! throttle for checkout/pull!!!!
                                        gio::spawn_blocking({
                                            let path = path.clone();
                                            let sender = sender.clone();
                                            let file_path = file
                                                .path()
                                                .expect("no file path")
                                                .into_os_string();
                                            move || {
                                                track_changes(
                                                    path, file_path, sender,
                                                )
                                            }
                                        });
                                    }
                                    _ => {
                                        debug!("file event in monitor {:?}", event);
                                    }
                                }
                            }
                        });
                        monitors.borrow_mut().push(monitor);
                    }
                }
            });
        }
    }

    pub fn update_stashes(&mut self, stashes: Stashes) {
        self.stashes.replace(stashes);
    }

    pub fn reset_hard(&self, sender: Sender<Event>) {
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

    pub fn pull(&self) {
        gio::spawn_blocking({
            let path = self.path.clone().expect("no path");
            let sender = self.sender.clone();
            move || {
                pull(path, sender);
            }
        });
    }

    pub fn push(
        &mut self,
        window: &ApplicationWindow,
        remote_dialog: Option<(String, bool, bool)>
    ){
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
                let dialog = crate::make_confirm_dialog(
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
                    Some((remote_branch, track_remote, ask_password)) if ask_password => {
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
                    user_pass.replace(
                        (
                            format!("{}", user_name.text()),
                            format!("{}", password.text())
                        ));
                }
                gio::spawn_blocking({
                    move || {
                        push(
                            path.expect("no path"),
                            remote_branch_name,
                            track_remote,
                            sender,
                            user_pass
                        );
                    }
                });
            }
        });
    }

    pub fn make_context(&mut self, text_view_width: Rc<RefCell<(i32, i32)>>) {
        let mut ctx = StatusRenderContext::new();
        ctx.screen_width.replace(*text_view_width.borrow());
        self.context.replace(ctx);
        // lines in diffs could be wider then screen
        if let Some(diff) = &self.staged {
            self.update_screen_line_width(diff.max_line_len);
        }
        if let Some(diff) = &self.unstaged {
            self.update_screen_line_width(diff.max_line_len);
        }
    }

    pub fn update_screen_line_width(&mut self, max_line_len: i32) {
        if let Some(ctx) = &mut self.context {
            if let Some(sw) = ctx.screen_width {
                if sw.1 < max_line_len {
                    ctx.screen_width.replace((sw.0, max_line_len));
                }
            }
        }
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
        _txt: &TextView,
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
                    let dialog = crate::make_confirm_dialog(
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
                    debug!("got response from dialog! {:?}", response);
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

    pub fn update_head(&mut self, mut head: Head, txt: &TextView) {
        // refactor.enrich
        if let Some(current_head) = &self.head {
            head.enrich_view(current_head);
        }
        self.head.replace(head);
        self.render(txt, RenderSource::Git);
    }

    pub fn update_upstream(
        &mut self,
        mut upstream: Option<Head>,
        txt: &TextView,
    ) {
        if let Some(rendered) = &mut self.upstream {
            if let Some(new) = upstream.as_mut() {
                new.enrich_view(rendered);
            } else {
                rendered.erase(txt, &mut self.context);
            }
        }

        self.upstream = upstream;
        self.render(txt, RenderSource::Git);
    }

    pub fn update_state(&mut self, mut state: State, txt: &TextView) {
        if let Some(current_state) = &self.state {
            state.enrich_view(current_state)
        }
        self.state.replace(state);
        self.render(txt, RenderSource::Git);
    }

    pub fn update_untracked(
        &mut self,
        mut untracked: Untracked,
        txt: &TextView,
    ) {
        self.update_screen_line_width(untracked.max_line_len);
        if let Some(u) = &mut self.untracked {
            untracked.enrich_view(u, txt, &mut self.context);
        }
        self.untracked.replace(untracked);
        self.render(txt, RenderSource::Git);
    }

    pub fn update_staged(&mut self, mut diff: Diff, txt: &TextView) {
        self.update_screen_line_width(diff.max_line_len);
        if let Some(s) = &mut self.staged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(s, txt, &mut self.context);
        }
        self.staged.replace(diff);
        // why check both??? perhaps just for very first render
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git);
        }
    }

    pub fn update_unstaged(&mut self, mut diff: Diff, txt: &TextView) {
        self.update_screen_line_width(diff.max_line_len);
        if let Some(u) = &mut self.unstaged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(u, txt, &mut self.context);
        }
        self.unstaged.replace(diff);
        // why check both??? perhaps just for very first render
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git);
        }
    }
    // status
    pub fn cursor(&mut self, txt: &TextView, line_no: i32, offset: i32) {
        let mut changed = false;
        if let Some(untracked) = &mut self.untracked {
            changed = changed || untracked.cursor(line_no, false);
        }
        if let Some(unstaged) = &mut self.unstaged {
            changed = changed || unstaged.cursor(line_no, false);
        }
        if let Some(staged) = &mut self.staged {
            changed = changed || staged.cursor(line_no, false);
        }
        if changed {
            self.render(txt, RenderSource::Cursor(line_no));
            let buffer = txt.buffer();
            trace!("put cursor on line {:?} in CURSOR", line_no);
            buffer.place_cursor(&buffer.iter_at_offset(offset));
        }
    }

    // Status
    pub fn expand(&mut self, txt: &TextView, line_no: i32, _offset: i32) {
        // let mut changed = false;
        if let Some(unstaged) = &mut self.unstaged {
            for file in &mut unstaged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(txt, RenderSource::Expand(expanded_line));
                    return;
                }
            }
        }
        if let Some(staged) = &mut self.staged {
            for file in &mut staged.files {
                if let Some(expanded_line) = file.expand(line_no) {
                    self.render(txt, RenderSource::Expand(expanded_line));
                    return;
                }
            }
        }
    }

    // Status
    pub fn render(&mut self, txt: &TextView, source: RenderSource) {
        let buffer = txt.buffer();
        let mut iter = buffer.iter_at_offset(0);

        if let Some(head) = &mut self.head {
            head.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(upstream) = &mut self.upstream {
            upstream.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(state) = &mut self.state {
            state.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(untracked) = &mut self.untracked {
            if untracked.files.is_empty() {
                self.untracked_spacer.view.squashed = true;
                self.untracked_label.view.squashed = true;
            }
            self.untracked_spacer.render(
                &buffer,
                &mut iter,
                &mut self.context,
            );
            self.untracked_label
                .render(&buffer, &mut iter, &mut self.context);
            untracked.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(unstaged) = &mut self.unstaged {
            if unstaged.files.is_empty() {
                self.unstaged_spacer.view.squashed = true;
                self.unstaged_label.view.squashed = true;
            }
            self.unstaged_spacer
                .render(&buffer, &mut iter, &mut self.context);
            self.unstaged_label
                .render(&buffer, &mut iter, &mut self.context);
            unstaged.render(&buffer, &mut iter, &mut self.context);
        }

        if let Some(staged) = &mut self.staged {
            if staged.files.is_empty() {
                self.staged_spacer.view.squashed = true;
                self.staged_label.view.squashed = true;
            }
            self.staged_spacer
                .render(&buffer, &mut iter, &mut self.context);
            self.staged_label
                .render(&buffer, &mut iter, &mut self.context);
            staged.render(&buffer, &mut iter, &mut self.context);
        }
        trace!("render source {:?}", source);
        match source {
            RenderSource::Cursor(_) => {
                // avoid loops on cursor renders
                trace!("avoid cursor position on cursor");
            }
            RenderSource::Expand(line_no) => {
                self.choose_cursor_position(txt, &buffer, Some(line_no));
            }
            RenderSource::Git => {
                self.choose_cursor_position(txt, &buffer, None);
            }
            RenderSource::Resize => {}
        };
    }

    pub fn resize(&mut self, txt: &TextView) {
        // it need to rerender all highlights and
        // background to match new window size
        if let Some(diff) = &mut self.staged {
            diff.resize(txt, &mut self.context)
        }
        if let Some(diff) = &mut self.unstaged {
            diff.resize(txt, &mut self.context)
        }
        self.render(txt, RenderSource::Resize);
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
                    debug!("stage untracked {:?}", file.title());
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
        debug!("stage. apply filter {:?}", filter);
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
                    self.cursor(txt, line_no, iter.offset());
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
        self.cursor(txt, iter.line(), iter.offset());
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
        println!("debug at line {:?}", current_line);
        if let Some(diff) = &mut self.staged {
            for f in &diff.files {
                dbg!(&f);
            }
            diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let content = vc.get_content();
                let view = vc.get_view();
                if view.line_no == current_line {
                    println!("found view {:?}", content);
                    dbg!(view);
                }
            });
        }
        if let Some(diff) = &mut self.unstaged {
            for f in &diff.files {
                dbg!(&f);
            }
            diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let content = vc.get_content();
                let view = vc.get_view();
                if view.line_no == current_line {
                    println!("found view {:?}", content);
                    dbg!(view);
                }
            });
        }
    }
}
