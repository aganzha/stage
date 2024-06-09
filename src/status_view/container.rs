use crate::status_view::render::View;
use crate::status_view::tags;
use crate::status_view::{Label};

use crate::{
    Diff, DiffKind, File, Head, Hunk, Line, LineKind, State,
    StatusRenderContext, UnderCursor, Untracked, UntrackedFile,
};
use git2::{DiffLineType, RepositoryState};
use gtk4::prelude::*;
use gtk4::{TextBuffer, TextIter};
use log::{debug, trace};

pub fn make_tag(name: &str) -> tags::TxtTag {
    tags::TxtTag::from_str(name)
}

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

    fn is_markup(&self) -> bool {
        false
    }
    
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

    fn tags(&self) -> Vec<tags::TxtTag> {
        Vec::new()
    }

    fn fill_context(&self, _: &mut StatusRenderContext) {}

    // ViewContainer
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext,
    ) {
        self.fill_context(context);
        let content = self.get_content();
        let tags = self.tags();
        let is_markup = self.is_markup();
        let view =
            self.get_view().render(buffer, iter, content, is_markup, tags, context);
        if view.expanded || view.child_dirty {
            for child in self.get_children() {
                child.render(buffer, iter, context);
            }
        }
        self.get_view().child_dirty = false;
    }

    // ViewContainer
    fn cursor(
        &mut self,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext,
    ) -> bool {
        // returns if view is changed during cursor move
        let mut result = false;
        let view = self.get_view();

        let current_before = view.current;
        let active_before = view.active;

        let view_expanded = view.expanded;
        let current = view.is_rendered_in(line_no);
        if current {
            self.fill_under_cursor(context)
        }
        let active_by_parent =
            self.is_active_by_parent(parent_active, context);
        let mut active_by_child = false;

        if view_expanded {
            for child in self.get_children() {
                active_by_child = child.get_view().is_rendered_in(line_no);
                if active_by_child {
                    child.fill_under_cursor(context);
                    break;
                }
            }
        }
        active_by_child = self.is_active_by_child(active_by_child, context);

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
            result = child.cursor(line_no, self_active, context) || result;
            // if child.get_view().current {
            //     // self.fill_under_cursor(child, context);
            // }
        }
        // result here just means view is changed
        // it does not actually means that view is under cursor
        result
    }

    fn fill_under_cursor(&self, _context: &mut StatusRenderContext) {}

    fn is_active_by_child(
        &self,
        _child_active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        false
    }

    fn is_active_by_parent(
        &self,
        _parent_active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
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
            trace!("expand. ____________ go deeper");
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
        buffer: &TextBuffer,
        context: &mut StatusRenderContext,
    ) {
        // CAUTION. ATTENTION. IMPORTANT
        // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        // after this operation all prev iters bevome INVALID!
        // it need to reobtain them!

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
        trace!("original line_no {:?}", line_no);
        let original_line_no = view.line_no;

        if let Some(ec) = context.erase_counter {
            debug!("erase counter {:?}", ec);
            line_no -= ec;
        }

        view.squashed = true;
        view.child_dirty = true;
        self.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let view = vc.get_view();
            view.squashed = true;
            view.child_dirty = true;
        });
        // GOT BUG HERE DURING STAGING SAME FILES!
        trace!("line finally {:?}", line_no);
        let mut iter = buffer
            .iter_at_line(line_no)
            .expect("can't get iter at line");
        trace!("!! erase one signgle view at buffer line = {:?}. orig view line {:?}", line_no, original_line_no);

        self.render(buffer, &mut iter, context);
    }

    fn resize(
        &mut self,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext,
    ) {
        // this is just RE render with build_up
        let view = self.get_view();
        let line_no = view.line_no;
        if view.rendered {
            view.dirty = true;
            // TODO! why i need child dirty here?
            view.child_dirty = true;
        }
        self.walk_down(&mut |vc: &mut dyn ViewContainer| {
            let view = vc.get_view();
            view.dirty = true;
            // child dirty triggers expand?
            // view.child_dirty = true;
        });
        let mut iter = buffer
            .iter_at_line(line_no)
            .expect("can't get iter at line");
        self.render(buffer, &mut iter, context);
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
    fn cursor(
        &mut self,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext,
    ) -> bool {
        context.under_cursor_diff(&self.kind);
        let mut result = false;
        for file in &mut self.files {
            result = file.cursor(line_no, parent_active, context) || result;
        }
        result
    }

    // Diff
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext,
    ) {
        // why do i need it at all?
        self.view.line_no = iter.line();
        context.update_screen_line_width(self.max_line_len);

        for file in &mut self.files {
            file.render(buffer, iter, context);
        }
        let start_iter = buffer.iter_at_line(self.view.line_no).unwrap();
        let end_iter = buffer.iter_at_line(iter.line()).unwrap();
        for tag in self.tags() {
            buffer.apply_tag_by_name(tag.str(), &start_iter, &end_iter);
        }
    }
    // Diff
    fn expand(&mut self, _line_no: i32) -> Option<i32> {
        todo!("no one calls expand on diff");
    }

    fn tags(&self) -> Vec<tags::TxtTag> {
        match self.kind {
            DiffKind::Staged => vec![make_tag(tags::STAGED)],
            // TODO! create separate tag for conflicted!
            DiffKind::Unstaged | DiffKind::Conflicted => {
                vec![make_tag(tags::UNSTAGED)]
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
        format!(
            "{}{}",
            if self.status == git2::Delta::Deleted {
                "- "
            } else {
                ""
            },
            self.path.to_str().unwrap()
        )
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        self.hunks
            .iter_mut()
            .map(|vh| vh as &mut dyn ViewContainer)
            .collect()
    }
    fn tags(&self) -> Vec<tags::TxtTag> {
        let mut tags = vec![
            make_tag(tags::BOLD),
            make_tag(tags::POINTER),
        ];
        if self.status == git2::Delta::Deleted {
            tags.push(make_tag(tags::REMOVED));            
        }
        tags
    }

    fn fill_context(&self, context: &mut StatusRenderContext) {
        if let Some(len) = context.max_len {
            if len < self.max_line_len {
                context.max_len.replace(self.max_line_len);
            }
        } else {
            context.max_len.replace(self.max_line_len);
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
        let parts: Vec<&str> = self.header.split("@@").collect();
        let line_no = match self.kind {
            DiffKind::Unstaged | DiffKind::Conflicted => self.old_start,
            DiffKind::Staged => self.new_start,
        };
        let scope = parts.last().unwrap();
        if !scope.is_empty() {
            format!("Line {:} in{:}", line_no, scope)
        } else {
            format!("Line {:?}", line_no)
        }
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
    fn is_active_by_parent(
        &self,
        active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        // if file is active (cursor on it)
        // whole hunk is active
        active
    }

    // Hunk
    fn is_active_by_child(
        &self,
        active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        // if line is active (cursor on it)
        // whole hunk is active
        active
    }
    fn tags(&self) -> Vec<tags::TxtTag> {
        vec![
            make_tag(tags::HUNK),
            make_tag(tags::POINTER),
        ]
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
        self.view.get_mut()
    }

    fn get_content(&self) -> String {
        self.content.to_string()
    }

    fn get_children(&mut self) -> Vec<&mut dyn ViewContainer> {
        Vec::new()
    }

    // Line
    fn expand(&mut self, line_no: i32) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no == line_no {
            return Some(line_no);
        }
        None
    }

    // Line
    fn fill_under_cursor(&self, context: &mut StatusRenderContext) {
        context.under_cursor_line(&self.kind);
    }

    // Line
    fn is_active_by_parent(
        &self,
        active: bool,
        context: &mut StatusRenderContext,
    ) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        // Except conflicted lines

        match context.under_cursor {
            UnderCursor::Some {
                diff_kind: DiffKind::Conflicted,
                line_kind: LineKind::Ours(i),
            } => {
                return active && self.kind == LineKind::Ours(i);
            }
            UnderCursor::Some {
                diff_kind: DiffKind::Conflicted,
                line_kind: LineKind::Theirs(i),
            } => {
                return active && self.kind == LineKind::Theirs(i);
            }
            UnderCursor::Some {
                diff_kind: DiffKind::Conflicted,
                line_kind: _,
            } => {
                return false;
            }
            _ => {}
        }

        active
    }

    // Line
    fn tags(&self) -> Vec<tags::TxtTag> {
        match self.kind {// 
            LineKind::ConflictMarker(_) => return vec![make_tag(tags::CONFLICT_MARKER)],
            LineKind::Ours(_) => return vec![make_tag(tags::CONFLICT_MARKER)],
            LineKind::Theirs(_) => {
                // return Vec::new();
                return vec![make_tag(tags::THEIRS)];
            }
            _ => {}
        }
        match self.origin {
            DiffLineType::Addition => {
                vec![make_tag(tags::ADDED)]
            }
            DiffLineType::Deletion => {
                vec![make_tag(tags::REMOVED)]
            }
            _ => Vec::new(),
        }
    }
}

impl ViewContainer for Label {

    fn is_markup(&self) -> bool {
        true
    }
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

    fn is_markup(&self) -> bool {
        true
    }
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
            self.log_message
        )
    }
}

impl ViewContainer for State {

    fn is_markup(&self) -> bool {
        true
    }
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

    fn is_active_by_parent(
        &self,
        active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    fn tags(&self) -> Vec<tags::TxtTag> {
        Vec::new()
    }

    // Untracked
    fn render(
        &mut self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext,
    ) {
        self.view.line_no = iter.line();
        for file in &mut self.files {
            file.render(buffer, iter, context);
        }
    }

    // Untracked
    fn cursor(
        &mut self,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext,
    ) -> bool {
        let mut result = false;
        for file in &mut self.files {
            result = file.cursor(line_no, parent_active, context) || result;
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
        self.path.to_str().unwrap().to_string()
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

    fn is_active_by_parent(
        &self,
        active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    fn tags(&self) -> Vec<tags::TxtTag> {
        Vec::new()
    }
}
