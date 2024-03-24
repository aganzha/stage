pub mod container;
pub mod factory;
use container::{ViewContainer, ViewKind};
pub mod render;
use render::Tag;
pub mod reconciliation;
pub mod tests;

use crate::{
    commit, get_current_repo_status, push, stage_via_apply, ApplyFilter,
    ApplySubject, Diff, DiffKind, Head, State, View,
};

use async_channel::Sender;


use gtk4::prelude::*;
use gtk4::{
    gio, glib,
    ListBox, SelectionMode, TextBuffer, TextView, Window as Gtk4Window,
};


use libadwaita::prelude::*;
use libadwaita::{EntryRow, SwitchRow};
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
    Erase,
}

#[derive(Debug, Clone)]
pub struct StatusRenderContext {
    pub erase_counter: Option<i32>,
    pub diff_kind: Option<DiffKind>,
    pub max_hunk_len: Option<i32>,
}

impl Default for StatusRenderContext {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusRenderContext {
    pub fn new() -> Self {
        {
            Self {
                erase_counter: None,
                diff_kind: None,
                max_hunk_len: None,
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub head: Option<Head>,
    pub upstream: Option<Head>,
    pub state: Option<State>,
    pub staged_spacer: Label,
    pub staged_label: Label,
    pub staged: Option<Diff>,
    pub unstaged_spacer: Label,
    pub unstaged_label: Label,
    pub unstaged: Option<Diff>,
    pub rendered: bool, // what it is for ????
    pub context: Option<StatusRenderContext>,
}

impl Status {
    pub fn new() -> Self {
        Self {
            head: None,
            upstream: None,
            state: None,
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
            context: None::<StatusRenderContext>
        }
    }

    pub fn get_status(
        &self,
        path: Option<OsString>,
        sender: Sender<crate::Event>,
    ) {
        gio::spawn_blocking({
            move || {
                get_current_repo_status(path, sender);
            }
        });
    }

    pub fn push(
        &mut self,
        path: &OsString,
        window: &impl IsA<Gtk4Window>,
        sender: Sender<crate::Event>,
    ) {
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
            .css_classes(vec!["input_field"])
            .text(self.choose_remote())
            .build();
        lb.append(&input);
        lb.append(&upstream);

        crate::make_confirm_dialog(
            window,
            Some(&lb),
            "Push to remote/origin", // TODO here is harcode
            "Push",
        )
        .choose(None::<&gio::Cancellable>, {
            let path = path.clone();
            let sender = sender.clone();
            move |result| {
                if result == "confirm" {
                    trace!(
                        "confirm push dialog {:?} {:?}",
                        input.text(),
                        upstream.is_active()
                    );
                    let remote_branch_name = format!("{}", input.text());
                    let track_remote = upstream.is_active();
                    gio::spawn_blocking({
                        let path = path.clone();
                        move || {
                            push(
                                path,
                                remote_branch_name,
                                track_remote,
                                sender,
                            );
                        }
                    });
                }
            }
        });
    }

    pub fn choose_remote(&self) -> String {
        if let Some(upstream) = &self.upstream {
            debug!(
                "-------------------> upstream branch {:?}",
                upstream.branch.clone()
            );
            return upstream.branch.clone();
        }
        if let Some(head) = &self.head {
            debug!("-------------------> head branch");
            return head.branch.clone();
        }
        debug!("-------------------> Default");
        String::from("origin/master")
    }

    pub fn commit(
        &mut self,
        path: &OsString,
        _txt: &TextView,
        window: &impl IsA<Gtk4Window>,
        sender: Sender<crate::Event>,
    ) {
        if self.staged.is_some() {
            let lb = ListBox::builder()
                .selection_mode(SelectionMode::None)
                .css_classes(vec![String::from("boxed-list")])
                .build();
            let input = EntryRow::builder()
                .title("Commit message:")
                .css_classes(vec!["input_field"])
                .build();
            lb.append(&input);
            // let me = Rc::new(RefCell::new(self));
            crate::make_confirm_dialog(window, Some(&lb), "Commit", "Commit")
                .choose(None::<&gio::Cancellable>, {
                    let path = path.clone();
                    let sender = sender.clone();
                    // let me = Rc::clone(&me);
                    move |result| {
                        if result == "confirm" {
                            trace!("confirm commit dialog {:?}", input.text());
                            let message = format!("{}", input.text());
                            gio::spawn_blocking({
                                let path = path.clone();
                                move || {
                                    commit(path, message, sender);
                                }
                            });
                        }
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
        // refactor.enrich
        match (&self.upstream, upstream.as_mut()) {
            (Some(current), Some(new)) => new.enrich_view(current),
            _ => {}
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

    pub fn update_staged(&mut self, mut diff: Diff, txt: &TextView) {
        if let Some(s) = &mut self.staged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(s, txt, &mut self.context);
        }
        self.staged.replace(diff);
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git);
        }
    }

    pub fn update_unstaged(&mut self, mut diff: Diff, txt: &TextView) {
        if let Some(u) = &mut self.unstaged {
            // DiffDirection is required here to choose which lines to
            // compare - new_ or old_
            // perhaps need to move to git.rs during sending event
            // to main (during update)
            diff.enrich_view(u, txt, &mut self.context);
        }
        self.unstaged.replace(diff);
        if self.staged.is_some() && self.unstaged.is_some() {
            self.render(txt, RenderSource::Git);
        }
    }
    // status
    pub fn cursor(&mut self, txt: &TextView, line_no: i32, offset: i32) {
        let mut changed = false;
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
                // avoid loops on cursor renders
                self.choose_cursor_position(txt, &buffer, Some(line_no));
            }
            RenderSource::Git => {
                // avoid loops on cursor renders
                self.choose_cursor_position(txt, &buffer, None);
            }
            _src => {}
        };
    }

    pub fn stage(
        &mut self,
        _txt: &TextView,
        _line_no: i32,
        path: &OsString,
        subject: ApplySubject,
        sender: Sender<crate::Event>,
    ) {
        // hm. this is very weird code
        match subject {
            ApplySubject::Stage => {
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
                ApplySubject::Stage => self.unstaged.as_mut().unwrap(),
                ApplySubject::Unstage => self.staged.as_mut().unwrap(),
            }
        };
        let mut filter = ApplyFilter::new(subject);
        let mut file_path_so_stage = String::new();
        let mut hunks_staged = 0;
        // there could be either file with all hunks
        // or just 1 hunk
        diff.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let content = vc.get_content();
            let kind = vc.get_kind();
            let view = vc.get_view();
            match kind {
                ViewKind::File => {
                    // just store current file_path
                    // in this loop. temporary variable
                    file_path_so_stage = content;
                }
                ViewKind::Hunk => {
                    if !view.active {
                        return;
                    }
                    // store active hunk in filter
                    // if the cursor is on file, all
                    // hunks under it will be active
                    filter.file_path = file_path_so_stage.clone();
                    filter.hunk_header.replace(content);
                    hunks_staged += 1;
                }
                _ => (),
            }
        });
        debug!("stage. apply filter {:?}", filter);
        if !filter.file_path.is_empty() {
            if hunks_staged > 1 {
                // stage all hunks in file
                filter.hunk_header = None;
            }
            debug!("stage via apply {:?}", filter);
            gio::spawn_blocking({
                let path = path.clone();
                move || {
                    stage_via_apply(path, filter, sender);
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
        trace!("choose_cursor_position. optional line {:?}", line_no);
        let offset = buffer.cursor_position();
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
        let iter = buffer.iter_at_offset(offset);
        // after git op view could be shifted.
        // cursor is on place and it is visually current,
        // but view under it is not current, cause line_no differs
        trace!("choose cursor when NOT on eof {:?}", iter.line());
        self.cursor(txt, iter.line(), offset);
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
