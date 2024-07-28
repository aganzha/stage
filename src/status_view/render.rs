// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use std::{thread, time};
use crate::status_view::stage_view::cursor_to_line_offset;
use crate::status_view::tags;
use crate::status_view::view::{View, ViewState};
use crate::status_view::Label;
use crate::{
    Diff, DiffKind, File, Head, Hunk, Line, LineKind, State,
    StatusRenderContext, UnderCursor, Untracked, UntrackedFile,
};
use git2::{DiffLineType, RepositoryState};
use gtk4::prelude::*;
use gtk4::{TextBuffer, TextIter};
use log::{debug, trace};
use std::path::PathBuf;

pub const LINE_NO_SPACE: i32 = 6;

pub fn make_tag(name: &str) -> tags::TxtTag {
    tags::TxtTag::from_str(name)
}


// This enum must implement all render methods!
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
    fn is_empty(&self, context: &mut StatusRenderContext<'_>) -> bool;

    fn get_kind(&self) -> ViewKind;

    fn get_children(&self) -> Vec<&dyn ViewContainer>;

    fn get_view(&self) -> &View;

    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'_>,
    );

    // method just for debugging
    fn get_content_for_debug(
        &self,
        _context: &mut StatusRenderContext<'_>,
    ) -> String {
        String::from("unknown")
    }

    // fn adopt_other<'a>(&self,
    //                    other: Option<&dyn ViewContainer>,
    //                    buffer: &TextBuffer,
    //                    context: &mut StatusRenderContext<'a>) where Self: Sized {
    //     if let Some(new) = other {
    //         new.enrich_view(self, buffer, context);
    //     } else {
    //         self.erase(buffer, context);
    //     }
    // }

    fn adopt_view(&self, other_rendered_view: &View) {
        let view = self.get_view();
        view.line_no.replace(other_rendered_view.line_no.get());
        view.flags.replace(other_rendered_view.flags.get());
        view.tag_indexes
            .replace(other_rendered_view.tag_indexes.get());
        view.transfer(true);
    }

    fn enrich_view(
        &self,
        rendered: &dyn ViewContainer,
        _buffer: &TextBuffer,
        _context: &mut crate::StatusRenderContext,
    ) {
        self.adopt_view(rendered.get_view());
    }

    // TODO - return bool and stop iteration when false
    // visitor takes child as first arg and parent as second arg
    fn walk_down(&self, visitor: &mut dyn FnMut(&dyn ViewContainer)) {
        for child in self.get_children() {
            visitor(child);
            child.walk_down(visitor);
        }
    }

    // ViewContainer
    fn tags<'a>(&'a self, _ctx: &mut StatusRenderContext<'a>) -> Vec<tags::TxtTag> {
        Vec::new()
    }

    fn prepare_context<'a>(&'a self, _ctx: &mut StatusRenderContext<'a>) {}

    fn fill_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) {
        let view = self.get_view();
        if view.is_current() {
            ctx.cursor = view.line_no.get();
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

    fn start_end_iters(
        &self,
        buffer: &TextBuffer,
        line_no: i32,
    ) -> (TextIter, TextIter) {
        let mut start_iter = buffer.iter_at_line(line_no).unwrap();
        start_iter.set_line_offset(0);
        let mut end_iter = buffer.iter_at_line(line_no).unwrap();
        end_iter.forward_to_line_end();
        (start_iter, end_iter)
    }

    fn remove_tag(&self, buffer: &TextBuffer, tag: &tags::TxtTag) {
        let view = self.get_view();
        if view.tag_is_added(tag) {
            let (start_iter, end_iter) =
                self.start_end_iters(buffer, view.line_no.get());
            buffer.remove_tag_by_name(tag.name(), &start_iter, &end_iter);
            view.tag_removed(tag);
        }
    }

    fn add_tag(&self, buffer: &TextBuffer, tag: &tags::TxtTag) {
        let view = self.get_view();
        if !view.tag_is_added(tag) {
            let (start_iter, end_iter) =
                self.start_end_iters(buffer, view.line_no.get());
            buffer.apply_tag_by_name(tag.name(), &start_iter, &end_iter);
            view.tag_added(tag);
        }
    }

    // ViewContainer
    fn apply_tags<'a>(
        &'a self,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'a>,
    ) {
        if self.is_empty(context) {
            // TAGS BECOME BROKEN ON EMPTY LINES!
            return;
        }
        for t in &self.tags(context) {
            self.add_tag(buffer, t);
        }
    }

    fn adjust_tags_on_cursor_change<'a>(
        &'a self,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'a>
    ) {
    }

    // ViewContainer
    fn render<'a>(
        &'a self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext<'a>,
    ) {
        self.prepare_context(context);

        // render_in_textview +++++++++++++++++++++++++++++++++++++++++++
        let line_no = iter.line();
        let view = self.get_view();
        let state = view.get_state_for(line_no);
        trace!("............ state in view {} {:?} {:?}", line_no, state, self.get_kind());
        match state {
            ViewState::RenderedInPlace => {
                trace!("..render MATCH rendered_in_line {:?}", line_no);
                iter.forward_lines(1);
            }
            ViewState::Deleted => {
                trace!("..render MATCH !rendered squashed {:?}", line_no);
            }
            ViewState::NotYetRendered => {
                trace!("..render MATCH insert {:?}", line_no);
                self.write_content(iter, buffer, context);
                buffer.insert(iter, "\n");

                view.line_no.replace(line_no);
                view.render(true);
                self.apply_tags(buffer, context);
            }
            ViewState::TagsModified => {
                trace!("..render MATCH TagsModified {:?}", line_no);
                self.apply_tags(buffer, context);
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
                self.write_content(iter, buffer, context);
                self.apply_tags(buffer, context);

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

        // recursive render @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
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
            context.cursor = self.get_view().line_no.get();
        }        
        self.fill_context(context);
        // post render @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
    }

    // ViewContainer
    /// returns if view is changed during cursor move
    fn cursor<'a>(
        &'a self,
        buffer: &TextBuffer,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext<'a>,
    ) -> bool {
        self.prepare_context(context);
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
            result = view.is_active() != active_before
                || view.is_current() != current_before;
        }
        for child in self.get_children() {
            result = child.cursor(buffer, line_no, self_active, context) || result;
        }
        // result here just means view is changed
        // it does not actually means that view is under cursor
        if view.is_current() {
            context.cursor = view.line_no.get();
        }
        self.fill_context(context);
        if result {
            self.adjust_tags_on_cursor_change(buffer, context);
        }
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
            self.walk_down(&mut |vc: &dyn ViewContainer| {
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

    fn erase(&self, buffer: &TextBuffer, context: &mut StatusRenderContext) {
        // CAUTION. ATTENTION. IMPORTANT
        // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        // after this operation all prev iters bevome INVALID!
        // it need to reobtain them!

        // this ONLY rendering. the data remains
        // unchaged. means it used to be called just
        // before replacing data in status struct.
        // CAUTION. ATTENTION. IMPORTANT

        let view = self.get_view();
        if !view.is_rendered() {
            return;
        }

        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let initial_line_offset = iter.line_offset();

        let view = self.get_view();

        // let mut line_no = view.line_no.get();
        let line_no = view.line_no.get() - context.erase_counter;
        let mut iter = buffer.iter_at_line(line_no).unwrap();
        let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();

        nel_iter.forward_lines(1);
        context.erase_counter += 1;
        // buffer.delete(&mut iter, &mut nel_iter);

        if view.is_expanded() {
            self.walk_down(&mut |vc: &dyn ViewContainer| {
                let view = vc.get_view();
                if !view.is_rendered() {
                    return
                }
                // what about expanded?
                // does not mater!
                // if view is not expanded, its child will be not rendered!

                // let line_no = view.line_no.get() - context.erase_counter;
                // let mut iter = buffer.iter_at_line(line_no).unwrap();
                // let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
                nel_iter.forward_lines(1);
                context.erase_counter += 1;
                // buffer.delete(&mut iter, &mut nel_iter);
            });
        }
        buffer.delete(&mut iter, &mut nel_iter);
        cursor_to_line_offset(buffer, initial_line_offset);
    }
}

impl ViewContainer for Diff {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        self.files.is_empty()
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::Diff
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    // Diff
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
        if !self.is_empty() {
            // let title = match self.kind {
            //     DiffKind::Staged => "<span weight=\"bold\" color=\"#8b6508\">Staged changes</span>_",
            //     DiffKind::Unstaged => "<span weight=\"bold\" color=\"#8b6508\">Unstaged changes</span>_",
            //     DiffKind::Conflicted => "<span weight=\"bold\" color=\"#ff0000\">Conflicts</span>_",
            // };
            // buffer.insert_markup(iter, title);
            buffer.insert(
                iter,
                match self.kind {
                    DiffKind::Staged => "Staged changes",
                    DiffKind::Unstaged => "Unstaged changes",
                    DiffKind::Conflicted => "Conflicts",
                }
            );
        }
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        self.files
            .iter()
            .map(|vh| vh as &dyn ViewContainer)
            .collect()
    }

    // diff
    fn cursor<'a>(
        &'a self,
        buffer: &TextBuffer,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext<'a>,
    ) -> bool {
        if self.kind == DiffKind::Conflicted && !self.has_conflicts() {
            // when all conflicts are resolved, Conflicted
            // highlights must behave just like Unstaged
            // (highlight normally instead of ours/theirs
            context.under_cursor_diff(&DiffKind::Unstaged);
        } else {
            context.under_cursor_diff(&self.kind);
        }
        let mut result = false;
        for file in &self.files {
            result = file.cursor(buffer, line_no, parent_active, context) || result;
        }
        result
    }

    // // Diff
    // fn render<'a>(
    //     &'a self,
    //     buffer: &TextBuffer,
    //     iter: &mut TextIter,
    //     context: &mut StatusRenderContext<'a>,
    // ) {
    //     if !self.is_empty() {
    //         debug!(" i am not empty, diiiiiiiiiiiff");
    //         ViewContainer::render(
    //             self,
    //             buffer,
    //             iter,
    //             context
    //         );
    //     }
    //     // why do i need it at all?
    //     self.view.line_no.replace(iter.line());
    //     context.update_screen_line_width(self.max_line_len);

    //     for file in &self.files {
    //         file.render(buffer, iter, context);
    //     }
    //     let start_iter = buffer.iter_at_line(self.view.line_no.get()).unwrap();
    //     let end_iter = buffer.iter_at_line(iter.line()).unwrap();
    //     for tag in self.tags(context) {
    //         buffer.apply_tag_by_name(tag.str(), &start_iter, &end_iter);
    //     }
    // }

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

    fn tags<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) -> Vec<tags::TxtTag> {
        match self.kind {
            DiffKind::Staged => vec![make_tag(tags::BOLD), make_tag(tags::STAGED)],
            // TODO! create separate tag for conflicted!
            DiffKind::Unstaged | DiffKind::Conflicted => {
                vec![make_tag(tags::BOLD), make_tag(tags::UNSTAGED)]
            }
        }
    }
}

impl ViewContainer for File {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::File
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_content_for_debug(
        &self,
        _context: &mut StatusRenderContext<'_>,
    ) -> String {
        format!(
            "file: {:?} at line {:?}",
            self.path,
            self.view.line_no.get()
        )
    }

    // File
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
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
    // File
    fn tags<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) -> Vec<tags::TxtTag> {
        let mut tags = vec![make_tag(tags::BOLD), make_tag(tags::POINTER)];
        if self.status == git2::Delta::Deleted {
            tags.push(make_tag(tags::REMOVED));
        }
        tags
    }

    // file
    fn fill_context(&self, context: &mut StatusRenderContext) {        
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
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::Hunk
    }

    fn get_content_for_debug(
        &self,
        _context: &mut StatusRenderContext<'_>,
    ) -> String {
        format!(
            "hunk: {:?} at line {:?}",
            self.header,
            self.view.line_no.get()
        )
    }
    // Hunk
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {

        let parts: Vec<&str> = self.header.split("@@").collect();
        // let line_no = match self.kind {
        //     DiffKind::Unstaged | DiffKind::Conflicted => self.old_start,
        //     DiffKind::Staged => self.new_start,
        // };
        let scope = parts.last().unwrap();
        buffer.insert(iter, "Line ");
        buffer.insert(iter, &format!("{}", self.new_start));// line_ no
        if !scope.is_empty() {
            buffer.insert(iter, &format!(" in {}", scope));
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
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) {
        ctx.current_hunk = Some(self);
    }

    // Hunk
    fn fill_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) {       
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
    // File
    fn tags<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) -> Vec<tags::TxtTag> {
        Vec::new()
    }

    fn is_expandable_by_child(&self) -> bool {
        true
    }

    fn fill_under_cursor(&self, context: &mut StatusRenderContext) {
        context.under_cursor_hunk(self);
    }
}

impl ViewContainer for Line {
    fn is_empty(&self, context: &mut StatusRenderContext<'_>) -> bool {
        if let Some(hunk) = context.current_hunk {
            return self.content(hunk).is_empty();
        }
        false
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::Line
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    fn get_content_for_debug(
        &self,
        context: &mut StatusRenderContext<'_>,
    ) -> String {
        format!(
            "Line: {:?} at line {:?}",
            self.content(context.current_hunk.unwrap()),
            self.view.line_no.get()
        )
    }

    // Line
    fn fill_context(&self, ctx: &mut StatusRenderContext) {        
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
    fn adjust_tags_on_cursor_change<'a>(
        &'a self,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'a>
    ) {
        let added = make_tag(tags::ADDED);
        let removed = make_tag(tags::REMOVED);
        let enhanced_added = added.enhance();
        let enhanced_removed = removed.enhance();
        match self.origin {
            DiffLineType::Addition => {
                if self.view.is_active() {
                    self.remove_tag(buffer, &added);
                    self.add_tag(buffer, &enhanced_added);
                } else {
                    self.remove_tag(buffer, &enhanced_added);
                    self.add_tag(buffer, &added);
                }
            }
            DiffLineType::Deletion => {
                if self.view.is_active() {
                    self.remove_tag(buffer, &removed);
                    self.add_tag(buffer, &enhanced_removed);
                } else {
                    self.remove_tag(buffer, &enhanced_removed);
                    self.add_tag(buffer, &removed);
                }
            }
            _ => {}
        }
    }

    // Line
    fn tags<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) -> Vec<tags::TxtTag> {
        match self.kind {
            //
            LineKind::ConflictMarker(_) => {
                return vec![make_tag(tags::CONFLICT_MARKER)]
            }
            // .............................................???? PERHAPS OURS?? was CONFLICT_MARKER
            LineKind::Ours(_) => return vec![make_tag(tags::OURS)],
            LineKind::Theirs(_) => {
                // return Vec::new();
                return vec![make_tag(tags::THEIRS)];
            }
            _ => {}
        }
        // TODO! ENHANCED_ADDED!!!!
        // context is not enough. because apply_tags is called ONLY
        // in render. but it need to change tags in cursor!
        // there is only highlight changes for now!
        // it need to add apply_tags in cursor!
        // first cleanup tags and then reapply!
        match self.origin {
            DiffLineType::Addition => {
                vec![make_tag(tags::ADDED)]
                // if ctx.current_hunk.unwrap().view.is_active() {
                //     debug!("eeeeeeeeeeeeenhanced");
                //     vec![make_tag(tags::ENHANCED_ADDED)]
                // } else {
                //     debug!("regular...............");
                //     vec![make_tag(tags::ADDED)]
                // }
            }
            DiffLineType::Deletion => {
                vec![make_tag(tags::REMOVED)]
                // if ctx.current_hunk.unwrap().view.is_active() {
                //     vec![make_tag(tags::ENHANCED_REMOVED)]
                // } else {
                //     vec![make_tag(tags::REMOVED)]
                // }
            }
            DiffLineType::Context => {
                vec![make_tag(tags::CONTEXT)]
            }
            _ => Vec::new(),
        }
    }

        // Line
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'_>,
    ) {

        let line_no = format!("{}", self.new_line_no.unwrap_or(self.old_line_no.unwrap_or(0)));
        match line_no.len() {
            1 => {
                buffer.insert(iter, "   ");
                buffer.insert(iter, &line_no);
            }
            2 => {
                buffer.insert(iter, "  ");
                buffer.insert(iter, &line_no);
            }
            3 => {
                buffer.insert(iter, " ");
                buffer.insert(iter, &line_no);
            }
            4 => {
                buffer.insert(iter, &line_no);
            }
            _ => {
                buffer.insert(iter, "..");
                buffer.insert(iter, &line_no[line_no.len() - 2 ..]);
            }
        }
        buffer.insert(iter, "  ");
        buffer.insert(iter, self.content(context.current_hunk.unwrap()));
    }

    // Line
    fn apply_tags<'a>(
        &'a self,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'a>,
    ) {
        // -----------------super-----------------
        // lines are always have line_no and cant be empty
        // if self.is_empty(context) {
        //     // TAGS BECOME BROKEN ON EMPTY LINES!
        //     return;
        // }
        for t in &self.tags(context) {
            self.add_tag(buffer, t);
        }
        // ---------------------------------------

        // line_no
        let line_no_tag = match self.origin {
            DiffLineType::Addition => make_tag(tags::LINE_NO_ADDED),
            DiffLineType::Deletion => make_tag(tags::LINE_NO_REMOVED),
            _ => make_tag(tags::LINE_NO_CONTEXT)
        };

        if !self.view.tag_is_added(&line_no_tag) {
            let (start_iter, mut end_iter) =
                self.start_end_iters(buffer, self.view.line_no.get());
            end_iter.set_line_offset(0);
            end_iter.forward_chars(LINE_NO_SPACE);
            buffer.apply_tag_by_name(
                line_no_tag.name(),
                &start_iter,
                &end_iter,
            );
            self.view.tag_added(&line_no_tag);
        }

        // highlight spaces
        let content = self.content(context.current_hunk.unwrap());
        let stripped = content
            .trim_end_matches(|c| -> bool { char::is_ascii_whitespace(&c) });
        let content_len = content.chars().count();
        let stripped_len = stripped.chars().count();

        if stripped_len < content_len
            && (self.origin == DiffLineType::Addition
                || self.origin == DiffLineType::Deletion)
        {
            // if will use here enhanced_added for now, but
            // spaces must have their separate tag!
            let spaces_tag = if self.origin == DiffLineType::Addition {
                make_tag(tags::SPACES_ADDED)
            } else {
                make_tag(tags::SPACES_REMOVED)
            };

            // do not add tag twice
            if !self.view.tag_is_added(&spaces_tag) {
                let (mut start_iter, end_iter) =
                    self.start_end_iters(buffer, self.view.line_no.get());
                start_iter.forward_chars(stripped_len as i32 + LINE_NO_SPACE);

                buffer.apply_tag_by_name(
                    spaces_tag.name(),
                    &start_iter,
                    &end_iter,
                );
                self.view.tag_added(&spaces_tag);
            }
        }
    }
}

impl ViewContainer for Label {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        self.content.is_empty()
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    // Label
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
        buffer.insert_markup(iter, &self.content);
    }
}

impl ViewContainer for Head {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    // Head
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
        buffer.insert_markup(
            iter,
            &format!(
                "{}<span color=\"#4a708b\">{}</span> {}",
                if !self.remote {
                    "Head:     "
                } else {
                    "Upstream: "
                },
                self.branch,
                self.log_message
            ),
        );
    }
}

impl ViewContainer for State {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::Label
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    // State
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
        buffer.insert(iter, "State:    ");
        match self.state {
            RepositoryState::Clean => {
                buffer.insert(iter, "Clean");
            }
            RepositoryState::Merge => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">Merge</span>",
                );
            }
            RepositoryState::Revert => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">Revert</span>",
                );
            }
            RepositoryState::RevertSequence => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">RevertSequence</span>",
                );
            }
            RepositoryState::CherryPick => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">CherryPick</span>",
                );
            }
            RepositoryState::CherryPickSequence => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">CherryPickSequence</span>",
                );
            }
            RepositoryState::Bisect => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">Bisect</span>",
                );
            }
            RepositoryState::Rebase => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">Rebase</span>",
                );
            }
            RepositoryState::RebaseInteractive => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">RebaseInteractive</span>",
                );
            }
            RepositoryState::RebaseMerge => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">RebaseMerge</span>",
                );
            }
            RepositoryState::ApplyMailbox => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">ApplyMailbox</span>",
                );
            }
            RepositoryState::ApplyMailboxOrRebase => {
                buffer.insert_markup(
                    iter,
                    "<span color=\"#ff0000\">ApplyMailboxOrRebase</span>",
                );
            }
        };
    }
}

impl ViewContainer for Untracked {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        self.files.is_empty()
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::Untracked
    }

    // untracked
    fn get_view(&self) -> &View {
        self.view.expand(true);
        &self.view
    }

    // Untracked (diff)
    fn write_content(
        &self,
        _iter: &mut TextIter,
        _buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
    }

    // Untracked (diff)
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

    // Untracked (diff)
    fn is_active_by_parent(
        &self,
        active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }

    // Untracked (diff)
    fn tags<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) -> Vec<tags::TxtTag> {
        Vec::new()
    }

    // Untracked (diff)
    fn render<'a>(
        &'a self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext<'a>,
    ) {
        self.view.line_no.replace(iter.line());
        for file in &self.files {
            file.render(buffer, iter, context);
        }
    }

    // Untracked (diff)
    fn cursor<'a>(
        &'a self,
        buffer: &TextBuffer,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext<'a>,
    ) -> bool {
        let mut result = false;
        for file in &self.files {
            result = file.cursor(buffer, line_no, parent_active, context) || result;
        }
        result
    }
}

impl ViewContainer for UntrackedFile {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn get_kind(&self) -> ViewKind {
        ViewKind::UntrackedFile
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    // Untracked file
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
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

    // Untracked (File)
    fn is_active_by_parent(
        &self,
        active: bool,
        _context: &mut StatusRenderContext,
    ) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        active
    }
    // Untracked (File)
    fn tags<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) -> Vec<tags::TxtTag> {
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
        //     result.push_str(&format!("FILE: {}", file.get_content_for_debug()));
        //     result.push_str("\n\t");
        //     result.push_str(&file.view.repr());
        //     result.push('\n');
        //     for hunk in &file.hunks {
        //         result.push_str(&format!("HUNK: {}", hunk.get_content_for_debug()));
        //         result.push_str("\n\t");
        //         result.push_str(&hunk.view.repr());
        //         result.push('\n');
        //         for line in &hunk.lines {
        //             result.push_str(&format!("LINE: {}", line.get_content_for_debug()));
        //             result.push_str("\n\t");
        //             result.push_str(&line.view.repr());
        //             result.push('\n');
        //         }
        //     }
        // }
        // result
    }
}

// pub trait MayBeViewContainer {
//     fn enrich_view(
//         &self,
//         rendered: Option<&dyn ViewContainer>,
//         buffer: &TextBuffer,
//         context: &mut crate::StatusRenderContext,
//     );
// }

// impl MayBeViewContainer for Option<&dyn ViewContainer> {
//     fn enrich_view(
//         &self,
//         rendered: Option<&dyn ViewContainer>,
//         buffer: &TextBuffer,
//         context: &mut crate::StatusRenderContext,
//     ){
//         if let Some(rendered) = rendered {
//             if let Some(new) = &self {
//                 new.enrich_view(rendered, buffer, context);
//             } else {
//                 rendered.erase(buffer, context);
//             }
//         }
//     }
// }
