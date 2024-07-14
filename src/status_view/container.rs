// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::status_view::render::{View, ViewState};
use crate::status_view::stage_view::cursor_to_line_offset;
use crate::status_view::tags;
use crate::status_view::Label;
use crate::{
    Diff, DiffKind, File, Head, Hunk, Line, LineKind, State,
    StatusRenderContext, UnderCursor, Untracked, UntrackedFile,
};
use git2::{DiffLineType, RepositoryState};
use gtk4::prelude::*;
use gtk4::{TextBuffer, TextIter};
use log::trace;
use std::path::PathBuf;

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

    fn get_children(&self) -> Vec<&dyn ViewContainer>;

    fn get_view(&self) -> &View;

    // TODO - return bool and stop iteration when false
    // visitor takes child as first arg and parent as second arg
    fn walk_down(&self, visitor: &dyn Fn(&dyn ViewContainer)) {
        for child in self.get_children() {
            visitor(child);
            child.walk_down(visitor);
        }
    }

    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer);

    fn tags(&self) -> Vec<tags::TxtTag> {
        Vec::new()
    }

    fn fill_context(&self, ctx: &mut StatusRenderContext) {
        let view = self.get_view();
        if view.is_current() {
            ctx.highlight_cursor = view.line_no.get();
        }
    }

    fn force_forward(&self, buffer: &TextBuffer, iter: &mut TextIter) {
        let current_line = iter.line();
        let moved = iter.forward_lines(1);
        if !moved {
            // happens sometimes when buffer is over
            buffer.insert(iter, "\n");
            if iter.line() - 2 == current_line {
                iter.forward_lines(-1);
            }
            trace!(
                "buffer is over. force 1 line forward. iter now is it line {:?}",
                iter.line()
            );
        }
        assert!(current_line + 1 == iter.line());
    }

    fn start_end_iters(&self, buffer: &TextBuffer, line_no: i32) -> (TextIter, TextIter) {
        let mut start_iter = buffer.iter_at_line(line_no).unwrap();
        start_iter.set_line_offset(0);
        let mut end_iter = buffer.iter_at_line(line_no).unwrap();
        end_iter.forward_to_line_end();
        (start_iter, end_iter)
    }

    fn remove_tag(&self, buffer: &TextBuffer, tag: &tags::TxtTag) {
        let view = self.get_view();
        if view.tag_is_added(tag) {
            let (start_iter, end_iter) = self.start_end_iters(buffer, view.line_no.get());
            buffer.remove_tag_by_name(tag.name(), &start_iter, &end_iter);
            view.tag_removed(tag);
        }
    }

    fn add_tag(&self, buffer: &TextBuffer, tag: &tags::TxtTag) {
        let view = self.get_view();
        if !view.tag_is_added(tag) {
            let (start_iter, end_iter) = self.start_end_iters(buffer, view.line_no.get());
            buffer.apply_tag_by_name(tag.name(), &start_iter, &end_iter);
            view.tag_added(tag);
        }
    }

    fn apply_tags(
        &self,
        buffer: &TextBuffer,
        content_tags: &Vec<tags::TxtTag>,
    ) {
        for t in content_tags {
            self.add_tag(buffer, t);
        }
    }

    // ViewContainer
    fn render(
        &self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext,
    ) {
        let tags = self.tags();
        let is_markup = self.is_markup();

        
        // let view = self.get_view().render_in_textview(
        //     buffer, iter, self, is_markup, tags, context,
        // );

        // render_in_textview +++++++++++++++++++++++++++++++++++++++++++
        let line_no = iter.line();
        let view = self.get_view();
        match view.get_state_for(line_no) {
            ViewState::RenderedInPlace => {
                trace!("..render MATCH rendered_in_line {:?}", line_no);
                iter.forward_lines(1);
            }
            ViewState::Deleted => {
                trace!("..render MATCH !rendered squashed {:?}", line_no);
            }
            ViewState::NotYetRendered => {
                trace!("..render MATCH insert {:?}", line_no);
                self.write_content(iter, &buffer);
                buffer.insert(iter, "\n");
                // if is_markup {
                //     buffer.insert_markup(iter, &format!("{}\n", content));
                // } else {
                //     buffer.insert(iter, content);
                //     buffer.insert(iter, "\n");
                // }                
                view.line_no.replace(line_no);
                view.render(true);
                // if !content.is_empty() {
                self.apply_tags(buffer, &tags);
                //}
            }
            ViewState::TagsModified => {
                trace!("..render MATCH TagsModified {:?}", line_no);
                self.apply_tags(buffer, &tags);
                if !iter.forward_lines(1) {
                    assert!(iter.offset() == buffer.end_iter().offset());
                }
                view.render(true);
            }
            ViewState::MarkedForDeletion => {
                trace!("..render MATCH squashed {:?}", line_no);
                let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
                nel_iter.forward_lines(1);
                buffer.delete(iter, &mut nel_iter);
                view.render(false);
                view.cleanup_tags();

                if let Some(ec) = context.erase_counter {
                    context.erase_counter.replace(ec + 1);
                } else {
                    context.erase_counter.replace(1);
                }
                trace!(
                    ">>>>>>>>>>>>>>>>>>>> just erased line. context {:?}",
                    context
                );
            }
            ViewState::UpdatedFromGit(l) => {
                trace!(".. render MATCH UpdatedFromGit {:?}", l);
                view.line_no.replace(line_no);
       
                let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
                eol_iter.forward_to_line_end();

                // if content is empty - eol iter will drop onto next line!
                // no need to delete in this case!
                if iter.line() == eol_iter.line() {
                    buffer.remove_all_tags(iter, &eol_iter);
                    buffer.delete(iter, &mut eol_iter);
                }
                view.cleanup_tags();
                self.write_content(iter, buffer);
                // if is_markup {
                //     buffer.insert_markup(iter, content);
                // } else {
                //     buffer.insert(iter, content);
                // }            

                self.apply_tags(buffer, &tags);
                // if !content.is_empty() {
                //     self.apply_tags(buffer, &content_tags);
                // }
                self.force_forward(buffer, iter);
            }
            ViewState::RenderedNotInPlace(l) => {
                // TODO: somehow it is related to transfered!
                trace!(".. render match not in place {:?}", l);
                view.line_no.replace(line_no);
                self.force_forward(buffer, iter);
            }
        }

        view.dirty(false);
        view.squash(false);
        view.transfer(false);
        // render_in_textview +++++++++++++++++++++++++++++++++++++++++++

        


        // post render @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
        if view.is_expanded() || view.is_child_dirty() {
            for child in self.get_children() {
                child.render(buffer, iter, context);
            }
        }
        self.get_view().child_dirty(false);
        // during the render the structure is changed
        // and current highlighted line could be
        // shifted. e.g. view is still current
        // bit the line is changed!
        if self.get_view().is_current() {
            context.highlight_cursor = self.get_view().line_no.get();
        }
        self.fill_context(context);
        // post render @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
    }    
    
    // ViewContainer
    /// returns if view is changed during cursor move
    fn cursor(
        &self,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext,
    ) -> bool {
        let mut result = false;
        let view = self.get_view();

        let current_before = view.is_current();
        let active_before = view.is_active();

        let current = view.is_rendered_in(line_no);
        if current {
            self.fill_under_cursor(context)
        }
        let active_by_parent =
            self.is_active_by_parent(parent_active, context);
        let mut active_by_child = false;

        if view.is_expanded() {
            // this is only 1 level.
            // when line is active, its hunk is active and file is not.
            // when file is active, all hunks below are active_by_parent
            // and all lines below are active_by_parent
            for child in self.get_children() {
                active_by_child = child.get_view().is_rendered_in(line_no);
                if active_by_child {
                    // under cursor changed here BEFORE calling
                    // child cursor
                    child.fill_under_cursor(context);
                    break;
                }
            }
        }
        active_by_child = self.is_active_by_child(active_by_child, context);

        let self_active = active_by_parent || current || active_by_child;
        if self_active {
            if self.get_kind() == ViewKind::Line {
                trace!("active LINE in cursor. line {} active_by_parent? {} parent_active ? {}",
                       view.line_no.get(),
                       active_by_parent,
                       parent_active);
            }
            if self.get_kind() == ViewKind::Hunk {
                trace!("active HUNK in cursor. line {} active_by_parent? {} parent_active ? {}, active_by_child? {}",
                       view.line_no.get(),
                       active_by_parent,
                       parent_active,
                       active_by_child);
            }
            if self.get_kind() == ViewKind::File {
                trace!("active FILE in cursor. line {} active_by_parent? {} parent_active ? {}, active_by_child? {}",
                       view.line_no.get(),
                       active_by_parent,
                       parent_active,
                       active_by_child);
            }
        }
        view.activate(self_active);
        view.make_current(current);

        if view.is_rendered() {
            // repaint if highlight is changed
            // trace!("its me marking dirty, cursor! {} at {}", ((view.is_active() != active_before) || (view.is_current() != current_before)), view.line_no.get());

            result = view.is_active() != active_before
                || view.is_current() != current_before;
            // newhighlight
            // view.dirty(
            //     view.is_active() != active_before
            //         || view.is_current() != current_before,
            // );
            // result = view.is_dirty();
        }
        for child in self.get_children() {
            result = child.cursor(line_no, self_active, context) || result;
        }
        // result here just means view is changed
        // it does not actually means that view is under cursor
        self.fill_context(context);
        result
    }

    fn fill_under_cursor(&self, _context: &mut StatusRenderContext) {}

    // base
    fn is_active_by_child(
        &self,
        _child_active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        false
    }

    // base
    fn is_active_by_parent(
        &self,
        _parent_active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        false
    }

    // ViewContainer
    fn expand(
        &self,
        line_no: i32,
        context: &mut StatusRenderContext,
    ) -> Option<i32> {
        let mut found_line: Option<i32> = None;
        let v = self.get_view();
        if v.is_rendered_in(line_no) {
            let view = self.get_view();
            found_line = Some(line_no);
            view.expand(!view.is_expanded());
            view.child_dirty(true);
            let expanded = view.is_expanded();
            self.walk_down(&|vc: &dyn ViewContainer| {
                let view = vc.get_view();
                if expanded {
                    view.squash(false);
                    view.render(false);
                } else {
                    view.squash(true);
                }
            });
        } else if v.is_expanded() && v.is_rendered() {
            // go deeper for self.children
            for child in self.get_children() {
                found_line = child.expand(line_no, context);
                if found_line.is_some() {
                    break;
                }
            }
            if found_line.is_some() && self.is_expandable_by_child() {
                let line_no = self.get_view().line_no.get();
                return self.expand(line_no, context);
            }
        }
        found_line
    }

    fn is_expandable_by_child(&self) -> bool {
        false
    }

    // ViewContainer
    fn erase(&self, buffer: &TextBuffer, context: &mut StatusRenderContext) {
        // return;
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

        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let initial_line_offset = iter.line_offset();

        let view = self.get_view();
        trace!(
            "erasing {:?} at line {}",
            self.get_kind(),
            view.line_no.get()
        );
        let mut line_no = view.line_no.get();
        // trace!("original line_no {:?}", line_no);
        // let original_line_no = view.line_no.get();

        if let Some(ec) = context.erase_counter {
            trace!("erase counter {:?}", ec);
            line_no -= ec;
        }

        view.squash(true);
        view.child_dirty(true);
        self.walk_down(&|vc: &dyn ViewContainer| {
            let view = vc.get_view();
            view.squash(true);
            view.child_dirty(true);
        });
        // GOT BUG HERE DURING STAGING SAME FILES!
        // trace!("line finally {:?}", line_no);
        if let Some(mut iter) = buffer.iter_at_line(line_no) {
            self.render(buffer, &mut iter, context);
        } else {
            // todo - get all the buffer and write it to file completelly
            panic!("no line at the end of erase!!!!!!!!! {}", line_no);
        }
        cursor_to_line_offset(buffer, initial_line_offset);
    }
}

impl ViewContainer for Diff {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Diff
    }

    fn child_count(&self) -> usize {
        self.files.len()
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn write_content(&self, _iter: &mut TextIter, _buffer: &TextBuffer) {
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        self.files
            .iter()
            .map(|vh| vh as &dyn ViewContainer)
            .collect()
    }

    // diff
    fn cursor(
        &self,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext,
    ) -> bool {
        if self.kind == DiffKind::Conflicted && !self.has_conflicts() {
            // when all conflicts are resolved, Conflicted
            // highlights must behave just line Unstaged
            // (highlight normally instead of ours/theirs
            context.under_cursor_diff(&DiffKind::Unstaged);
        } else {
            context.under_cursor_diff(&self.kind);
        }
        let mut result = false;
        for file in &self.files {
            result = file.cursor(line_no, parent_active, context) || result;
        }
        result
    }

    // Diff
    fn render(
        &self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext,
    ) {
        // why do i need it at all?
        self.view.line_no.replace(iter.line());
        context.update_screen_line_width(self.max_line_len);

        for file in &self.files {
            file.render(buffer, iter, context);
        }
        let start_iter = buffer.iter_at_line(self.view.line_no.get()).unwrap();
        let end_iter = buffer.iter_at_line(iter.line()).unwrap();
        for tag in self.tags() {
            buffer.apply_tag_by_name(tag.str(), &start_iter, &end_iter);
        }
    }
    // Diff
    fn expand(
        &self,
        line_no: i32,
        context: &mut StatusRenderContext,
    ) -> Option<i32> {
        let mut result: Option<i32> = None;
        for file in &self.files {
            if let Some(line) = file.expand(line_no, context) {
                result.replace(line);
            }
        }
        result
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

    fn get_view(&self) -> &View {
        &self.view
    }

    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
        if self.status == git2::Delta::Deleted {
            buffer.insert(iter, "- ");
        }
        buffer.insert(iter, self.path.to_str().unwrap());
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        self.hunks
            .iter()
            .map(|vh| vh as &dyn ViewContainer)
            .collect()
    }
    fn tags(&self) -> Vec<tags::TxtTag> {
        let mut tags = vec![make_tag(tags::BOLD), make_tag(tags::POINTER)];
        if self.status == git2::Delta::Deleted {
            tags.push(make_tag(tags::REMOVED));
        }
        tags
    }

    // file
    fn fill_context(&self, context: &mut StatusRenderContext) {
        if self.view.is_current() {
            context.highlight_cursor = self.view.line_no.get();
        }
        // does not used
        if let Some(len) = context.max_len {
            if len < self.max_line_len {
                context.max_len.replace(self.max_line_len);
            }
        } else {
            context.max_len.replace(self.max_line_len);
        }
    }

    /// if something in file is active
    /// the file IS NOT active
    /// (because when file is active everything
    /// in this file become active)
    fn is_active_by_child(
        &self,
        _active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        false
    }
}

impl ViewContainer for Hunk {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Hunk
    }

    fn child_count(&self) -> usize {
        self.lines.len()
    }


    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
        let parts: Vec<&str> = self.header.split("@@").collect();
        let line_no = match self.kind {
            DiffKind::Unstaged | DiffKind::Conflicted => self.old_start,
            DiffKind::Staged => self.new_start,
        };
        let scope = parts.last().unwrap();
        buffer.insert(iter, "Line ");
        buffer.insert(iter, &format!("{}", line_no));
        if !scope.is_empty() {
            buffer.insert(iter, &format!("in {}", scope));
        }
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        self.lines
            .iter()
            .filter(|l| {
                !matches!(
                    l.origin,
                    DiffLineType::FileHeader | DiffLineType::HunkHeader
                )
            })
            .map(|vh| vh as &dyn ViewContainer)
            .collect()
    }

    // Hunk
    fn fill_context(&self, ctx: &mut StatusRenderContext) {
        if self.view.is_current() {
            ctx.highlight_cursor = self.view.line_no.get();
        }
        if self.view.is_rendered() {
            ctx.collect_hunk_highlights(self.view.line_no.get());
        }
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
        Vec::new()
    }

    fn is_expandable_by_child(&self) -> bool {
        true
    }
}

impl ViewContainer for Line {
    fn get_kind(&self) -> ViewKind {
        ViewKind::Line
    }
    fn child_count(&self) -> usize {
        0
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
        buffer.insert(iter, &self.content);
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    // Line
    fn fill_context(&self, ctx: &mut StatusRenderContext) {
        if self.view.is_current() {
            ctx.highlight_cursor = self.view.line_no.get();
        }
        if self.view.is_rendered() && self.view.is_active() {
            ctx.collect_line_highlights(self.view.line_no.get());
        }
    }

    // Line
    fn expand(
        &self,
        line_no: i32,
        _context: &mut StatusRenderContext,
    ) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no.get() == line_no {
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
        match self.kind {
            //
            LineKind::ConflictMarker(_) => {
                return vec![make_tag(tags::CONFLICT_MARKER)]
            }
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
    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
        buffer.insert(iter, &self.content);
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
    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
        if !self.remote {
            buffer.insert(iter, "Head:     ");
        } else {
                buffer.insert(iter, "Upstream: ");
        }
        buffer.insert(iter, "<span color=\"#4a708b\">");
        buffer.insert(iter, &self.branch);
        buffer.insert(iter, "</span> ");
        buffer.insert(iter, &self.log_message);
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
    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
        buffer.insert(iter, "State:    ");
        match self.state {
            RepositoryState::Clean => {
                buffer.insert(iter, "Clean");
            },
            RepositoryState::Merge => {
                buffer.insert(iter, "<span color=\"#ff0000\">Merge</span>");
            },
            RepositoryState::Revert => {
                buffer.insert(iter, "<span color=\"#ff0000\">Revert</span>");
            },
            RepositoryState::RevertSequence => {
                buffer.insert(iter, "<span color=\"#ff0000\">RevertSequence</span>");
            }
            RepositoryState::CherryPick => {
                buffer.insert(iter, "<span color=\"#ff0000\">CherryPick</span>");
            }
            RepositoryState::CherryPickSequence => {
                buffer.insert(iter, "<span color=\"#ff0000\">CherryPickSequence</span>");
            }
            RepositoryState::Bisect => {
                buffer.insert(iter, "<span color=\"#ff0000\">Bisect</span>");
            },
            RepositoryState::Rebase => {
                buffer.insert(iter, "<span color=\"#ff0000\">Rebase</span>");
            },
            RepositoryState::RebaseInteractive => {
                buffer.insert(iter, "<span color=\"#ff0000\">RebaseInteractive</span>");
            }
            RepositoryState::RebaseMerge => {
                buffer.insert(iter, "<span color=\"#ff0000\">RebaseMerge</span>");
            }
            RepositoryState::ApplyMailbox => {
                buffer.insert(iter, "<span color=\"#ff0000\">ApplyMailbox</span>");
            }
            RepositoryState::ApplyMailboxOrRebase => {
                buffer.insert(iter, "<span color=\"#ff0000\">ApplyMailboxOrRebase</span>");
            }
        };
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
    fn get_view(&self) -> &View {
        self.view.expand(true);
        &self.view
    }

    // Untracked
    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
    }

    // Untracked
    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        self.files
            .iter()
            .map(|vh| vh as &dyn ViewContainer)
            .collect()
    }

    // Untracked (diff)
    fn expand(
        &self,
        line_no: i32,
        _context: &mut StatusRenderContext,
    ) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no.get() == line_no {
            return Some(line_no);
        }
        None
    }

    // Untracked
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
        &self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext,
    ) {
        self.view.line_no.replace(iter.line());
        for file in &self.files {
            file.render(buffer, iter, context);
        }
    }

    // Untracked
    fn cursor(
        &self,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext,
    ) -> bool {
        let mut result = false;
        for file in &self.files {
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

    fn get_view(&self) -> &View {
        &self.view
    }

    fn write_content(&self, iter: &mut TextIter, buffer: &TextBuffer) {
        buffer.insert(iter, self.path.to_str().unwrap());
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    // untracked file
    fn expand(
        &self,
        line_no: i32,
        _context: &mut StatusRenderContext,
    ) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no.get() == line_no {
            return Some(line_no);
        }
        None
    }

    // untracked
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

impl Diff {
    pub fn chosen_file_and_hunk_old(
        &self,
    ) -> (Option<PathBuf>, Option<String>) {
        let mut file_path: Option<PathBuf> = None;
        let mut hunk_header: Option<String> = None;
        for file in &self.files {
            if file.view.is_current() {
                file_path.replace(file.path.clone());
                break;
            }
            for hunk in &file.hunks {
                if hunk.view.is_active() {
                    // if more then 1 hunks are active that means
                    // that file is active and previous break
                    // must prevent to going here
                    assert!(hunk_header.is_none());
                    file_path.replace(file.path.clone());
                    hunk_header.replace(hunk.header.clone());
                    break;
                }
            }
        }
        (file_path, hunk_header)
    }
    pub fn chosen_file_and_hunk(&self) -> (Option<&File>, Option<&Hunk>) {
        let mut file_path: Option<&File> = None;
        let mut hunk_header: Option<&Hunk> = None;
        for file in &self.files {
            if file.view.is_current() {
                file_path.replace(file);
                break;
            }
            for hunk in &file.hunks {
                if hunk.view.is_active() {
                    // if more then 1 hunks are active that means
                    // that file is active and previous break
                    // must prevent to going here
                    assert!(hunk_header.is_none());
                    file_path.replace(file);
                    hunk_header.replace(hunk);
                    break;
                }
            }
        }
        (file_path, hunk_header)
    }

    pub fn dump(&self) -> String {
        String::from("dump")
        // let mut result = String::new();
        // for file in &self.files {
        //     result.push_str(&format!("FILE: {}", file.get_content()));
        //     result.push_str("\n\t");
        //     result.push_str(&file.view.repr());
        //     result.push('\n');
        //     for hunk in &file.hunks {
        //         result.push_str(&format!("HUNK: {}", hunk.get_content()));
        //         result.push_str("\n\t");
        //         result.push_str(&hunk.view.repr());
        //         result.push('\n');
        //         for line in &hunk.lines {
        //             result.push_str(&format!("LINE: {}", line.get_content()));
        //             result.push_str("\n\t");
        //             result.push_str(&line.view.repr());
        //             result.push('\n');
        //         }
        //     }
        // }
        // result
    }
}
