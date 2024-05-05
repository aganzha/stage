use crate::status_view::{Label, Tag};
use crate::{
    Diff, File, Head, Hunk, Line, State, StatusRenderContext, Untracked,
    UntrackedFile, View, DiffKind
};
use git2::{DiffLineType, RepositoryState};
use gtk4::prelude::*;
use gtk4::{TextBuffer, TextIter, TextView};
use log::{trace, debug};

#[derive(Debug, Clone, PartialEq)]
pub enum ViewKind {
    Diff,
    File,
    Hunk,
    Line,
    Label,
    Untracked,
    UntrackedFile,
}

pub trait ViewContainer {
    fn get_kind(&self) -> ViewKind;

    fn child_count(&self) -> usize;

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer>;

    fn get_view(&mut self) -> &mut View;

    // TODO - return bool and stop iteration when false
    // visitor takes child as first arg and parent as second arg
    fn walk_down(&mut self, visitor: &mut dyn FnMut(&mut dyn ViewContainer)) {
        for child in self.get_children() {
            visitor(child);
            child.walk_down(visitor);
        }
    }

    fn get_content(&self) -> String;

    fn tags(&self) -> Vec<Tag> {
        Vec::new()
    }

    fn fill_context(&self, _: &mut Option<StatusRenderContext>) {}

    // ViewContainer
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut Option<StatusRenderContext>,
    ) {
        self.fill_context(context);
        let content = self.get_content();
        let tags = self.tags();
        let view =
            self.get_view().render(buffer, iter, content, tags, context);
        if view.expanded || view.child_dirty {
            for child in self.get_children() {
                child.render(buffer, iter, context);
            }
        }
        self.get_view().child_dirty = false;
    }

    // ViewContainer
    fn cursor(&mut self, line_no: i32, parent_active: bool) -> bool {
        // returns if view is changed during cursor move
        let mut result = false;
        let view = self.get_view();

        let current_before = view.current;
        let active_before = view.active;

        let view_expanded = view.expanded;
        let current = view.is_rendered_in(line_no);
        let active_by_parent = self.is_active_by_parent(parent_active);
        let mut active_by_child = false;

        if view_expanded {
            for child in self.get_children() {
                active_by_child = child.get_view().is_rendered_in(line_no);
                if active_by_child {
                    break;
                }
            }
        }
        active_by_child = self.is_active_by_child(active_by_child);

        let self_active = active_by_parent || current || active_by_child;

        let view = self.get_view();
        view.active = self_active;
        view.current = current;

        if view.rendered {
            // repaint if highlight is changed
            view.dirty =
                view.active != active_before || view.current != current_before;
            result = view.dirty;
        }
        for child in self.get_children() {
            result = child.cursor(line_no, self_active) || result;
        }
        // result here just means view is changed
        // it does not actually means that view is under cursor
        result
    }

    fn is_active_by_child(&self, _child_active: bool) -> bool {
        false
    }

    fn is_active_by_parent(&self, _parent_active: bool) -> bool {
        false
    }

    // ViewContainer
    fn expand(&mut self, line_no: i32) -> Option<i32> {
        let mut found_line: Option<i32> = None;
        let v = self.get_view();
        if v.is_rendered_in(line_no) {
            let view = self.get_view();
            found_line = Some(line_no);
            view.expanded = !view.expanded;
            view.child_dirty = true;
            let expanded = view.expanded;
            self.walk_down(&mut |vc: &mut dyn ViewContainer| {
                let view = vc.get_view();
                if expanded {
                    view.squashed = false;
                    view.rendered = false;
                } else {
                    view.squashed = true;
                }
            });
        } else if v.expanded && v.rendered {
            // go deeper for self.children
            for child in self.get_children() {
                found_line = child.expand(line_no);
                if found_line.is_some() {
                    break;
                }
            }
            if found_line.is_some() && self.is_expandable_by_child() {
                let my_line = self.get_view().line_no;
                return self.expand(my_line);
            }
        }
        found_line
    }

    fn is_expandable_by_child(&self) -> bool {
        false
    }

    // ViewContainer
    fn erase(
        &mut self,
        txt: &TextView,
        context: &mut Option<StatusRenderContext>,
    ) {
        // CAUTION. ATTENTION. IMPORTANT
        // this ONLY rendering. the data remains
        // unchaged. means it used to be called just
        // before replacing data in status struct.
        // CAUTION. ATTENTION. IMPORTANT
        // if 1 view is rendered - it is ok.
        // next render on Status struct will shift all views.
        // But when erease multiple view in loop, all rest views
        // in loop must be shifted manually!
        // @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
        // during render the cursor moves down or become
        // unchanged in case of erasing (RenderedAndMarkedAsSquashed)
        // here we have only erase loop. cursor will be always
        // on same place, means it need to decrement view.line_no
        // by amount of deleted lines. thats why i need erase_counter.
        // but how to count, if erasing is recursive? it need to pass
        // it to render itself! means each render must receives context.
        // hm. how to avoid it? lets not avoid it. lets try to pass it,
        // and also put there prev_line length!
        let view = self.get_view();
        let mut line_no = view.line_no;
        if let Some(ctx) = context {
            if let Some(ec) = ctx.erase_counter {
                line_no -= ec;
            }
        }
        view.squashed = true;
        view.child_dirty = true;
        trace!("erasing ......{:?}", &view);
        self.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let view = vc.get_view();
            view.squashed = true;
            view.child_dirty = true;
        });
        let buffer = txt.buffer();
        // GOT BUG HERE DURING STAGING SAME FILES!
        let mut iter = buffer
            .iter_at_line(line_no)
            .expect("can't get iter at line");
        trace!("erase one signgle view at line > {:?}", line_no);
        self.render(&buffer, &mut iter, context);
    }

    fn resize(
        &mut self,
        txt: &TextView,
        context: &mut Option<StatusRenderContext>,
    ) {
        trace!("+++++++++++++++++++++ resize {:?}", context);
        let view = self.get_view();
        let line_no = view.line_no;
        if view.rendered {
            view.dirty = true;
            view.child_dirty = true;
        }
        self.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let view = vc.get_view();
            view.squashed = true;
            view.dirty = true;
            view.child_dirty = true;
        });
        let buffer = txt.buffer();
        let mut iter = buffer
            .iter_at_line(line_no)
            .expect("can't get iter at line");
        trace!("render after reisze at line {:?}", iter.line());
        self.render(&buffer, &mut iter, context);
    }

    fn get_id(&self) -> String {
        // unique id used in staging filter.
        // it is used in comparing files and hunks
        self.get_content()
    }
}

impl ViewContainer for Diff {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Diff
    }

    fn child_count(&self) -> usize {
        self.files.len()
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        String::from("")
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.files
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }

    // diff
    fn cursor(&mut self, line_no: i32, parent_active: bool) -> bool {
        let mut result = false;
        for file in &mut self.files {
            result = file.cursor(line_no, parent_active) || result;
        }
        result
    }

    // Diff
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut Option<StatusRenderContext>,
    ) {
        // why do i need it at all?
        self.view.line_no = iter.line();
        for file in &mut self.files {
            file.render(buffer, iter, context);
        }
        let start_iter = buffer.iter_at_line(self.view.line_no).unwrap();
        let end_iter = buffer.iter_at_line(iter.line()).unwrap();
        for tag in self.tags() {
            buffer.apply_tag_by_name(tag.name(), &start_iter, &end_iter);
        }
    }
    // Diff
    fn expand(&mut self, _line_no: i32) -> Option<i32> {
        todo!("no one calls expand on diff");
    }

    fn tags(&self) -> Vec<Tag> {
        match self.kind {
            DiffKind::Staged => {
                return vec![Tag::Staged]
            },
            // TODO! create separate tag for conflicted!
            DiffKind::Unstaged | DiffKind::Conflicted => {
                return vec![Tag::Unstaged]
            }
        }
    }
}

impl ViewContainer for File {
    fn get_kind(&self) -> ViewKind {
        ViewKind::File
    }

    fn child_count(&self) -> usize {
        self.hunks.len()
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.title()
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.hunks
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }
    fn tags(&self) -> Vec<Tag> {
        vec![Tag::Bold, Tag::Pointer]
    }

    fn fill_context(&self, context: &mut Option<StatusRenderContext>) {
        if let Some(ctx) = context {
            if let Some(len) = ctx.max_len {
                if len < self.max_line_len {
                    ctx.max_len.replace(self.max_line_len);
                }
            } else {
                ctx.max_len.replace(self.max_line_len);
            }
        }
    }

    fn get_id(&self) -> String {
        // unique id used in staging filter.
        // it is used in comparing files and hunks
        self.path.to_str().unwrap().to_string()
    }
}

impl ViewContainer for Hunk {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Hunk
    }

    fn child_count(&self) -> usize {
        self.lines.len()
    }

    fn get_content(&self) -> String {
        self.title()
    }

    fn get_view(&mut self) -> &mut View {
        if self.view.line_no == 0 && !self.view.expanded {
            // hunks are expanded by default
            self.view.expanded = true
        }
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.lines
            .iter_mut()
            .filter(|l| {
                !matches!(
                    l.origin,
                    DiffLineType::FileHeader | DiffLineType::HunkHeader
                )
            })
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }
    // Hunk
    fn is_active_by_parent(&self, active: bool) -> bool {
        // if file is active (cursor on it)
        // whole hunk is active
        active
    }

    // Hunk
    fn is_active_by_child(&self, active: bool) -> bool {
        // if line is active (cursor on it)
        // whole hunk is active
        active
    }
    fn tags(&self) -> Vec<Tag> {
        vec![Tag::Hunk, Tag::Pointer]
    }

    fn is_expandable_by_child(&self) -> bool {
        true
    }

    fn get_id(&self) -> String {
        // unique id used in staging filter.
        // it is used in comparing files and hunks
        self.header.clone()
    }
}

impl ViewContainer for Line {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Line
    }
    fn child_count(&self) -> usize {
        0
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.content.to_string()
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    // line
    fn expand(&mut self, line_no: i32) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no == line_no {
            return Some(line_no);
        }
        None
    }

    // LIne
    fn is_active_by_parent(&self, active: bool) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    fn tags(&self) -> Vec<Tag> {
        match self.origin {
            DiffLineType::Addition => {
                vec![Tag::Added]
            }
            DiffLineType::Deletion => {
                vec![Tag::Removed]
            }
            _ => Vec::new(),
        }
    }
}

impl ViewContainer for Label {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }
    fn child_count(&self) -> usize {
        0
    }
    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn get_content(&self) -> String {
        self.content.to_string()
    }
}

impl ViewContainer for Head {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }
    fn child_count(&self) -> usize {
        0
    }
    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn get_content(&self) -> String {
        format!(
            "{}<span color=\"#4a708b\">{}</span> {}",
            if !self.remote {
                "Head:     "
            } else {
                "Upstream: "
            },
            &self.branch,
            self.commit
        )
    }
}

impl ViewContainer for State {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }
    fn child_count(&self) -> usize {
        0
    }
    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn get_content(&self) -> String {
        let state = match self.state {
            RepositoryState::Clean => "Clean",
            RepositoryState::Merge => "<span color=\"#ff0000\">Merge</span>",
            RepositoryState::Revert => "<span color=\"#ff0000\">Revert</span>",
            RepositoryState::RevertSequence => {
                "<span color=\"#ff0000\">RevertSequence</span>"
            }
            RepositoryState::CherryPick => {
                "<span color=\"#ff0000\">CherryPick</span>"
            }
            RepositoryState::CherryPickSequence => {
                "<span color=\"#ff0000\">CherryPickSequence</span>"
            }
            RepositoryState::Bisect => "<span color=\"#ff0000\">Bisect</span>",
            RepositoryState::Rebase => "<span color=\"#ff0000\">Rebase</span>",
            RepositoryState::RebaseInteractive => {
                "<span color=\"#ff0000\">RebaseInteractive</span>"
            }
            RepositoryState::RebaseMerge => {
                "<span color=\"#ff0000\">RebaseMerge</span>"
            }
            RepositoryState::ApplyMailbox => {
                "<span color=\"#ff0000\">ApplyMailbox</span>"
            }
            RepositoryState::ApplyMailboxOrRebase => {
                "<span color=\"#ff0000\">ApplyMailboxOrRebase</span>"
            }
        };
        format!("State:    {}", state)
    }
}

impl ViewContainer for Untracked {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Untracked
    }
    fn child_count(&self) -> usize {
        self.files.len()
    }

    // untracked
    fn get_view(&mut self) -> &mut View {
        self.view.expanded = true;
        &mut self.view
    }

    // Untracked
    fn get_content(&self) -> String {
        String::from("")
    }

    // Untracked
    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.files
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }

    // Untracked
    fn expand(&mut self, line_no: i32) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no == line_no {
            return Some(line_no);
        }
        None
    }

    fn is_active_by_parent(&self, active: bool) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    fn tags(&self) -> Vec<Tag> {
        Vec::new()
    }

    // Untracked
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut Option<StatusRenderContext>,
    ) {
        self.view.line_no = iter.line();
        for file in &mut self.files {
            file.render(buffer, iter, context);
        }
    }

    // Untracked
    fn cursor(&mut self, line_no: i32, parent_active: bool) -> bool {
        let mut result = false;
        for file in &mut self.files {
            result = file.cursor(line_no, parent_active) || result;
        }
        result
    }
}

impl ViewContainer for UntrackedFile {
    fn get_kind(&self) -> ViewKind {
        ViewKind::UntrackedFile
    }
    fn child_count(&self) -> usize {
        0
    }

    fn get_view(&mut self) -> &mut View {
        &mut self.view
    }

    fn get_content(&self) -> String {
        self.title()
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    fn expand(&mut self, line_no: i32) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no == line_no {
            return Some(line_no);
        }
        None
    }

    fn is_active_by_parent(&self, active: bool) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    fn tags(&self) -> Vec<Tag> {
        Vec::new()
    }
}
