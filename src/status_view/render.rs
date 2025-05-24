// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::context::CursorPosition;
use crate::status_view::stage_view::cursor_to_line_offset;
use crate::status_view::tags;
use crate::status_view::view::{View, ViewState};
use crate::status_view::Label;
use crate::{
    Diff,
    DiffKind,
    File,
    Head,
    Hunk,
    Line,
    LineKind,
    State,
    StatusRenderContext, //, Untracked, UntrackedFile,
    MARKER_OURS,
    MARKER_THEIRS,
};
use git2::{DiffLineType, RepositoryState};
use gtk4::prelude::*;
use gtk4::{Align, Label as GtkLabel, TextBuffer, TextIter};
use libadwaita::StyleManager;
use log::trace;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
//pub const LINE_NO_SPACE: i32 = 6;

#[derive(PartialEq)]
pub enum TagChanges {
    Render,
    BecomeCurrent(bool),
    BecomeActive(bool),
}

pub trait ViewContainer {
    fn is_empty(&self, context: &mut StatusRenderContext<'_>) -> bool;

    fn get_children(&self) -> Vec<&dyn ViewContainer>;

    fn get_view(&self) -> &View;

    // ViewContainer
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'_>,
    );

    // method just for debugging
    fn _get_content_for_debug(&self, _context: &mut StatusRenderContext<'_>) -> String {
        String::from("unknown")
    }

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

    fn walk_down(&self, visitor: &mut dyn FnMut(&dyn ViewContainer)) {
        for child in self.get_children() {
            visitor(child);
            child.walk_down(visitor);
        }
    }

    // ViewContainer
    fn prepare_context<'a>(&'a self, _ctx: &mut StatusRenderContext<'a>) {}

    fn fill_cursor_position<'a>(&'a self, _context: &mut StatusRenderContext<'a>) {}

    fn fill_under_cursor<'a>(&'a self, _context: &mut StatusRenderContext<'a>) {}

    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, _ctx: &mut StatusRenderContext<'a>) {}

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

    fn add_tag(&self, buffer: &TextBuffer, tag: &str, offset_range: Option<(i32, i32)>) {
        let view = self.get_view();
        let (start_iter, end_iter) = if let Some((start, end)) = offset_range {
            (buffer.iter_at_offset(start), buffer.iter_at_offset(end))
        } else {
            self.start_end_iters(buffer, view.line_no.get())
        };
        buffer.apply_tag_by_name(tag, &start_iter, &end_iter);
    }
    fn remove_tag(&self, buffer: &TextBuffer, tag: &str, offset_range: Option<(i32, i32)>) {
        let view = self.get_view();
        let (start_iter, end_iter) = if let Some((start, end)) = offset_range {
            (buffer.iter_at_offset(start), buffer.iter_at_offset(end))
        } else {
            self.start_end_iters(buffer, view.line_no.get())
        };
        buffer.remove_tag_by_name(tag, &start_iter, &end_iter);
    }

    // ViewContainer
    fn apply_tags<'a>(
        &'a self,
        _changes: TagChanges,
        _buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'a>,
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

        let line_no = iter.line();
        let view = self.get_view();
        let state = view.get_state_for(line_no);
        trace!("............ state in view {} {:?}", line_no, state,);
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
                // before it was used only in cursor!
                self.apply_tags(TagChanges::Render, buffer, context);
            }
            ViewState::TagsModified => {
                // todo!("whats the case?");
                trace!("..render MATCH TagsModified {:?}", line_no);
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

        if view.is_expanded() || view.is_child_dirty() {
            for child in self.get_children() {
                child.render(buffer, iter, context);
            }
        }
        self.get_view().child_dirty(false);
    }

    fn cursor<'a>(
        &'a self,
        buffer: &TextBuffer,
        line_no: i32,
        parent_active: bool,
        context: &mut StatusRenderContext<'a>,
    ) {
        self.prepare_context(context);

        let view = self.get_view();

        let was_current = view.is_current();
        let was_active = view.is_active();

        let is_current = view.is_rendered_in(line_no);

        if is_current {
            self.fill_cursor_position(context);
        }

        let mut is_active = is_current;
        if !is_active {
            is_active = self.is_active_by_parent(parent_active, context);
        }
        if !is_active {
            let child_active = Rc::new(Cell::new(false));
            self.walk_down({
                let child_active = child_active.clone();
                &mut move |vc: &dyn ViewContainer| {
                    if vc.get_view().is_rendered_in(line_no) {
                        child_active.replace(true);
                    }
                }
            });
            is_active = child_active.get();
        }

        for child in self.get_children() {
            child.cursor(buffer, line_no, is_active, context);
        }

        view.activate(is_active);
        view.make_current(is_current);

        if is_active {
            self.fill_under_cursor(context);
        }

        if view.is_rendered() {
            if !self.is_empty(context) {
                if is_current != was_current {
                    self.apply_tags(TagChanges::BecomeCurrent(is_current), buffer, context);
                } else if is_active != was_active {
                    self.apply_tags(TagChanges::BecomeActive(is_active), buffer, context);
                }
            }
            self.after_cursor(buffer, context);
        }
    }

    // base
    fn is_active_by_parent(&self, parent_active: bool, _context: &mut StatusRenderContext) -> bool {
        parent_active
    }

    // ViewContainer
    fn expand(&self, line_no: i32, context: &mut StatusRenderContext) -> Option<i32> {
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

        // ALL ERASE AND RENDER PROCESSES MUST STRICLTY GO FROM TOP TO BOTTOM!
        let view = self.get_view();
        if !view.is_rendered() {
            return;
        }

        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let initial_line_offset = iter.line_offset();

        // it does not required here. erase will kill em all
        // let mut applied_tags = HashSet::new();

        let view = self.get_view();
        // it does not required here. erase will kill em all
        // for tag in view.added_tags() {
        //     applied_tags.insert(tag.name().to_string());
        // }

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
                // it does not required here. erase will kill em all
                // for tag in view.added_tags() {
                //     applied_tags.insert(tag.name().to_string());
                // }
                if !view.is_rendered() {
                    return;
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
        // it does not required here. erase will kill em all
        // for tag in applied_tags {
        //     buffer.remove_tag_by_name(&tag, &iter, &nel_iter);
        // }
        buffer.delete(&mut iter, &mut nel_iter);
        cursor_to_line_offset(buffer, initial_line_offset);
    }
    // container
    // clean_content is line_no: (content, offset)
    fn collect_clean_content<'a>(
        &'a self,
        from: i32,
        to: i32,
        content: &mut HashMap<i32, (String, i32)>,
        context: &mut StatusRenderContext<'a>,
    ) {
        for child in self.get_children() {
            child.prepare_context(context);
            child.collect_clean_content(from, to, content, context)
        }
    }
}

impl ViewContainer for Diff {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        self.files.is_empty()
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn _get_content_for_debug(&self, _context: &mut StatusRenderContext<'_>) -> String {
        match self.kind {
            DiffKind::Untracked => "Untracked files".to_string(),
            DiffKind::Staged => "Staged changes".to_string(),
            DiffKind::Unstaged => "Unstaged changes".to_string(),
            DiffKind::Conflicted => "Conflicts".to_string(),
            DiffKind::Commit => "Commit content".to_string(),
        }
    }

    // Diff
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'_>,
    ) {
        if !self.is_empty() {
            buffer.insert_markup(
                iter,
                match self.kind {
                    DiffKind::Untracked => "Untracked files",
                    DiffKind::Staged => "Staged changes",
                    DiffKind::Unstaged => "Unstaged changes",
                    DiffKind::Conflicted => "<span color=\"#ff0000\">Conflicts</span>",
                    DiffKind::Commit => "Commit content",
                },
            );
        }
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        self.files
            .iter()
            .map(|vh| vh as &dyn ViewContainer)
            .collect()
    }

    // Diff
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) {
        ctx.current_diff = Some(self);
    }

    // Diff
    fn apply_tags<'a>(
        &'a self,
        tag_changes: TagChanges,
        buffer: &TextBuffer,
        ctx: &mut StatusRenderContext<'a>,
    ) {
        // used to wrap all diff in tags.
        // is it necessary? yes, it is used
        // while handling user clicks inside stage_view
        if tag_changes == TagChanges::Render {
            self.add_tag(buffer, tags::DIFF, None);

            let start_line = self.view.line_no.get();
            let mut end_line = start_line;
            if let Some(file) = ctx.current_file {
                if file.view.is_rendered() {
                    end_line = file.view.line_no.get();
                }
            }
            if let Some(hunk) = ctx.current_diff {
                if hunk.view.is_rendered() {
                    end_line = hunk.view.line_no.get()
                }
            }
            if let Some(line) = ctx.current_line {
                if line.view.is_rendered() {
                    end_line = line.view.line_no.get();
                }
            }
            match self.kind {
                DiffKind::Unstaged | DiffKind::Staged => {
                    let tag = if self.kind == DiffKind::Staged {
                        tags::STAGED
                    } else {
                        tags::UNSTAGED
                    };
                    let start_iter = buffer.iter_at_line(start_line).unwrap();
                    let end_iter = buffer.iter_at_line(end_line).unwrap();
                    let offsets = Some((start_iter.offset(), end_iter.offset()));
                    self.remove_tag(buffer, tag, offsets);
                    self.add_tag(buffer, tag, offsets);
                }
                _ => {}
            }
        }
    }

    // Diff
    fn expand(&self, line_no: i32, context: &mut StatusRenderContext) -> Option<i32> {
        if self.kind == DiffKind::Untracked {
            return None;
        }
        let mut result: Option<i32> = None;
        let expand_all = self.get_view().is_rendered_in(line_no);
        if expand_all {
            result.replace(line_no);
        }
        for file in &self.files {
            if expand_all {
                file.expand(file.view.line_no.get(), context);
            } else if let Some(line) = file.expand(line_no, context) {
                result.replace(line);
            }
        }
        result
    }

    // Diff
    fn fill_cursor_position<'a>(&'a self, context: &mut StatusRenderContext<'a>) {
        context.cursor_position = CursorPosition::CursorDiff(self);
        self.fill_under_cursor(context);
    }

    // Diff
    fn fill_under_cursor<'a>(&'a self, context: &mut StatusRenderContext<'a>) {
        context.selected_diff = Some(self);
    }
}

impl ViewContainer for File {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn _get_content_for_debug(&self, _context: &mut StatusRenderContext<'_>) -> String {
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
    fn apply_tags<'a>(
        &'a self,
        changes: TagChanges,
        buffer: &TextBuffer,
        _ctx: &mut StatusRenderContext<'a>,
    ) {
        if changes == TagChanges::Render && self.kind != DiffKind::Untracked {
            self.add_tag(buffer, tags::FILE, None);
            self.add_tag(buffer, tags::POINTER, None);
            self.add_tag(buffer, tags::BOLD, None);
            if self.status == git2::Delta::Deleted {
                self.add_tag(buffer, tags::REMOVED, None);
            }
        }
    }

    // File
    fn is_active_by_parent(&self, active: bool, context: &mut StatusRenderContext) -> bool {
        if active {
            // files are active when cursor is on Diff
            match context.cursor_position {
                CursorPosition::CursorDiff(_) => {
                    return true;
                }
                _ => {
                    return false;
                }
            }
        }
        active
    }

    // file
    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, _context: &mut StatusRenderContext<'a>) {}

    // File
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) {
        ctx.current_file = Some(self);
    }

    // File
    fn fill_cursor_position<'a>(&'a self, context: &mut StatusRenderContext<'a>) {
        context.cursor_position = CursorPosition::CursorFile(self);
        self.fill_under_cursor(context);
    }

    // File
    fn fill_under_cursor<'a>(&'a self, context: &mut StatusRenderContext<'a>) {
        context.selected_file = Some(self);
    }
}

impl ViewContainer for Hunk {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn _get_content_for_debug(&self, _context: &mut StatusRenderContext<'_>) -> String {
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
        let scope = parts.last().unwrap();
        buffer.insert(iter, "Line ");
        buffer.insert(iter, &format!("{}", self.new_start));
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
    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, ctx: &mut StatusRenderContext<'a>) {
        ctx.collect_hunk_highlights(self.view.line_no.get());
    }

    // Hunk
    fn is_active_by_parent(&self, active: bool, context: &mut StatusRenderContext) -> bool {
        if active {
            // hunks are active when cursor is on File
            // or is on Diff
            match context.cursor_position {
                CursorPosition::CursorFile(_) | CursorPosition::CursorDiff(_) => {
                    return true;
                }
                _ => {
                    return false;
                }
            }
        }
        active
    }

    // Hunk
    fn apply_tags<'a>(
        &'a self,
        changes: TagChanges,
        buffer: &TextBuffer,
        _ctx: &mut StatusRenderContext<'a>,
    ) {
        if changes == TagChanges::Render {
            self.add_tag(buffer, tags::HUNK, None);
            self.add_tag(buffer, tags::POINTER, None)
        }
    }

    fn is_expandable_by_child(&self) -> bool {
        true
    }

    // Hunk
    fn fill_cursor_position<'a>(&'a self, context: &mut StatusRenderContext<'a>) {
        context.cursor_position = CursorPosition::CursorHunk(self);
        self.fill_under_cursor(context);
    }

    // Hunk
    fn fill_under_cursor<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) {
        ctx.selected_hunk = Some(self);
    }
}

impl ViewContainer for Line {
    fn is_empty(&self, context: &mut StatusRenderContext<'_>) -> bool {
        if let Some(hunk) = context.current_hunk {
            return self.content(hunk).is_empty();
        }
        false
    }

    fn get_view(&self) -> &View {
        &self.view
    }

    fn get_children(&self) -> Vec<&dyn ViewContainer> {
        Vec::new()
    }

    fn _get_content_for_debug(&self, context: &mut StatusRenderContext<'_>) -> String {
        format!(
            "Line: {:?} at line {:?}",
            self.content(context.current_hunk.unwrap()),
            self.view.line_no.get()
        )
    }

    // Line
    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, ctx: &mut StatusRenderContext<'a>) {
        // hm. collecting lines for highlight.
        if self.view.is_active() {
            ctx.collect_line_highlights(self.view.line_no.get());
        }
    }

    // Line
    fn expand(&self, line_no: i32, _context: &mut StatusRenderContext) -> Option<i32> {
        // here we want to expand hunk
        if self.get_view().line_no.get() == line_no {
            return Some(line_no);
        }
        None
    }

    // Line
    // it is useless. rendering_x is sliding variable during render
    // and there is nothing to render after line
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>) {
        ctx.current_line = Some(self);
    }

    // Line
    fn fill_cursor_position<'a>(&'a self, context: &mut StatusRenderContext<'a>) {
        context.cursor_position = CursorPosition::CursorLine(self);
    }

    // Line
    fn fill_under_cursor<'a>(&'a self, _ctx: &mut StatusRenderContext<'a>) {
        // there are multiple selected lines,
        // and storing some in context does not make sense
    }

    // Line
    fn is_active_by_parent(&self, active: bool, context: &mut StatusRenderContext) -> bool {
        // if HUNK is active (cursor on some line in it or on it)
        // this line is active
        // Except conflicted lines

        // conflicted lines become active by choosing
        // ours/theirs
        // they use under cursor for it.
        if !self.view.is_rendered() {
            return false;
        }

        if let Some(diff) = context.selected_diff {
            if diff.kind == DiffKind::Conflicted {
                match context.cursor_position {
                    CursorPosition::CursorLine(line) => match (&line.kind, &self.kind) {
                        (LineKind::Ours(_), LineKind::Ours(_)) => {
                            return active;
                        }
                        (LineKind::ConflictMarker(marker), LineKind::Ours(_))
                            if marker == MARKER_OURS =>
                        {
                            return active;
                        }
                        (LineKind::Theirs(_), LineKind::Theirs(_)) => {
                            return active;
                        }
                        (LineKind::ConflictMarker(marker), LineKind::Theirs(_))
                            if marker == MARKER_THEIRS =>
                        {
                            return active;
                        }
                        _ => {
                            return false;
                        }
                    },
                    _ => {
                        return false;
                    }
                }
            }
        }
        active
    }

    // Line
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'_>,
    ) {
        let anchor = iter
            .child_anchor()
            .unwrap_or(buffer.create_child_anchor(iter));

        let line_no = self
            .new_line_no
            .map(|num| num.as_u32())
            .unwrap_or(self.old_line_no.map(|num| num.as_u32()).unwrap_or(0));

        let line_no_text = format!(
            "<span size=\"small\" line_height=\"0.5\">{}</span>",
            line_no
        );
        if !anchor.widgets().is_empty() {
            let w = &anchor.widgets()[0];
            let l = w.downcast_ref::<GtkLabel>().unwrap();
            l.set_label(&line_no_text);
        } else {
            let lbl: GtkLabel = GtkLabel::builder()
                .use_markup(true)
                .hexpand(false)
                .vexpand(false)
                .label(line_no_text)
                .max_width_chars(3)
                .width_chars(3)
                .width_request(25)
                .halign(Align::Start)
                .xalign(0.0)
                .opacity(0.3)
                .css_classes(["line_no"])
                .build();
            context.stage.add_child_at_anchor(&lbl, &anchor);
        }

        let content = self.content(context.current_hunk.unwrap());
        if content.is_empty() {
            buffer.insert(iter, " ");
        } else {
            buffer.insert(iter, content);
        }
    }

    // Line
    fn apply_tags<'a>(
        &'a self,
        tag_changes: TagChanges,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'a>,
    ) {
        let (mut start_iter, end_iter) = self.start_end_iters(buffer, self.view.line_no.get());
        let start_offset = start_iter.offset();
        match tag_changes {
            TagChanges::Render => {
                // highlight spaces
                let content = self.content(context.current_hunk.unwrap());
                let stripped =
                    content.trim_end_matches(|c| -> bool { char::is_ascii_whitespace(&c) });
                let content_len = content.chars().count();
                let stripped_len = stripped.chars().count();

                if stripped_len < content_len
                    && (self.origin == DiffLineType::Addition
                        || self.origin == DiffLineType::Deletion)
                {
                    // if will use here enhanced_added for now, but
                    // spaces must have their separate tag!
                    let spaces_tag = if self.origin == DiffLineType::Addition {
                        tags::SPACES_ADDED
                    } else {
                        tags::SPACES_REMOVED
                    };
                    // do not add tag twice
                    // magic 1 is for label
                    start_iter.forward_chars(stripped_len as i32 + 1);
                    self.add_tag(
                        buffer,
                        spaces_tag,
                        Some((start_iter.offset(), end_iter.offset())),
                    );
                }
                // highlight syntax keywords
                for (start, end) in &self.keyword_ranges {
                    let tag = self.choose_syntax_tag();
                    let start = start_offset + start + (if *start == 0 { 0 } else { 1 });
                    let end = start_offset + end + 1;
                    self.add_tag(buffer, tag.0, Some((start, end)));
                }
                match self.kind {
                    LineKind::ConflictMarker(_) => {
                        self.add_tag(buffer, tags::CONFLICT_MARKER, None)
                    }
                    // no need to mark theirs/ours. use regular colors downwhere
                    LineKind::Ours(_) | LineKind::Theirs(_) => {
                        match self.origin {
                            DiffLineType::Addition => self.add_tag(buffer, tags::ADDED, None),
                            DiffLineType::Deletion => {
                                //  |  DiffLineType::Context
                                // this is a hack. in Ours lines got Context origin
                                // while Theirs got Addition
                                self.add_tag(buffer, tags::REMOVED, None)
                            }
                            _ => {}
                        }
                    }
                    _ => self.add_tag(buffer, self.choose_tag().0, None),
                }
            }

            TagChanges::BecomeCurrent(_) => {
                let mut iter = buffer.iter_at_offset(0);
                iter.set_line(self.view.line_no.get());
                if let Some(anchor) = iter.child_anchor() {
                    if !anchor.widgets().is_empty() {
                        let w = &anchor.widgets()[0];
                        let l = w.downcast_ref::<GtkLabel>().unwrap();
                        if self.view.is_current() {
                            l.set_opacity(1.0);
                        } else {
                            l.set_opacity(0.3);
                        }
                    }
                }
            }
            TagChanges::BecomeActive(is_active) => {
                // kill main tags first
                if is_active {
                    self.remove_tag(buffer, self.choose_tag().0, None);
                } else {
                    self.remove_tag(buffer, self.choose_tag().enhance().0, None);
                }
                // add syntax tags
                for (start, end) in &self.keyword_ranges {
                    let start = start_offset + start + (if *start == 0 { 0 } else { 1 });
                    let end = start_offset + end + 1;
                    if is_active {
                        self.remove_tag(buffer, self.choose_syntax_tag().0, Some((start, end)));
                        self.add_tag(
                            buffer,
                            self.choose_syntax_tag().enhance().0,
                            Some((start, end)),
                        );
                    } else {
                        self.remove_tag(buffer, self.choose_tag().enhance().0, Some((start, end)));
                        self.add_tag(buffer, self.choose_tag().0, Some((start, end)));
                    }
                }
                // put new main tags
                if is_active {
                    self.add_tag(buffer, self.choose_tag().enhance().0, None);
                } else {
                    self.add_tag(buffer, self.choose_tag().0, None);
                }
            }
        }
    }
    // Line
    fn collect_clean_content(
        &self,
        from: i32,
        to: i32,
        content_map: &mut HashMap<i32, (String, i32)>,
        context: &mut StatusRenderContext<'_>,
    ) {
        if !self.view.is_rendered() {
            return;
        }
        let line_no = self.view.line_no.get();
        if line_no >= from && line_no <= to {
            let content = self.content(context.current_hunk.unwrap()).to_string();
            content_map.insert(line_no, (content, 6));
        }
    }
}

impl ViewContainer for Label {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        self.content.is_empty()
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
        let title = {
            if let Some(branch_data) = &self.branch {
                branch_data.name.to_string()
            } else {
                "Detached head".to_string()
            }
        };
        let short = self.oid.to_string()[..7].to_string();
        let color = if StyleManager::default().is_dark() {
            "#839daf"
        } else {
            "#4a708b"
        };
        buffer.insert_markup(
            iter,
            &format!(
                "{} <span color=\"#1C71D8\">{}</span> <span color=\"{}\">{}</span> {}",
                if !self.is_upstream {
                    "Head:     "
                } else {
                    "Upstream: "
                },
                short,
                color,
                title,
                self.log_message
            ),
        );
    }

    // Head
    fn apply_tags<'a>(
        &'a self,
        tag_changes: TagChanges,
        buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'a>,
    ) {
        if tag_changes == TagChanges::Render {
            let line_no = self.view.line_no.get();
            let iter = buffer.iter_at_line(line_no).unwrap();
            let range = Some((iter.offset() + 11, iter.offset() + 18));
            self.add_tag(buffer, tags::POINTER, range);
            self.add_tag(buffer, tags::OID, range);
        }
    }
}

impl ViewContainer for State {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
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
                buffer.insert_markup(iter, "<span color=\"#ff0000\">Merge</span>");
            }
            RepositoryState::Revert => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">Revert</span>");
            }
            RepositoryState::RevertSequence => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">RevertSequence</span>");
            }
            RepositoryState::CherryPick => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">CherryPick</span>");
            }
            RepositoryState::CherryPickSequence => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">CherryPickSequence</span>");
            }
            RepositoryState::Bisect => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">Bisect</span>");
            }
            RepositoryState::Rebase => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">Rebase</span>");
            }
            RepositoryState::RebaseInteractive => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">RebaseInteractive</span>");
            }
            RepositoryState::RebaseMerge => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">RebaseMerge</span>");
            }
            RepositoryState::ApplyMailbox => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">ApplyMailbox</span>");
            }
            RepositoryState::ApplyMailboxOrRebase => {
                buffer.insert_markup(iter, "<span color=\"#ff0000\">ApplyMailboxOrRebase</span>");
            }
        };
    }
}

impl Diff {
    pub fn last_visible_line(&self) -> i32 {
        let le = self.files.len() - 1;
        let last_file = &self.files[le];
        if !last_file.view.is_expanded() {
            return last_file.view.line_no.get();
        }
        let le = last_file.hunks.len() - 1;
        let last_hunk = &last_file.hunks[le];
        if !last_hunk.view.is_expanded() {
            return last_hunk.view.line_no.get();
        }
        let le = last_hunk.lines.len() - 1;
        let last_line = &last_hunk.lines[le];
        last_line.view.line_no.get()
    }

    pub fn dump(&self) -> String {
        String::from("dump")
    }

    pub fn has_view_on(&self, line_no: i32) -> bool {
        if !self.view.is_rendered() {
            return false;
        }
        let my_line = self.view.line_no.get();
        if my_line > line_no {
            return false;
        }
        if my_line == line_no {
            return true;
        }
        self.last_visible_line() >= line_no
    }
}

impl Line {
    fn choose_tag(&self) -> tags::Tag {
        match self.origin {
            DiffLineType::Addition => tags::Tag(tags::ADDED),
            DiffLineType::Deletion => tags::Tag(tags::REMOVED),
            DiffLineType::Context => tags::Tag(tags::CONTEXT),
            _ => {
                todo!("syntax in non code line");
            }
        }
    }
    fn choose_syntax_tag(&self) -> tags::Tag {
        match self.origin {
            DiffLineType::Addition => tags::Tag(tags::SYNTAX_ADDED),
            DiffLineType::Deletion => tags::Tag(tags::SYNTAX_REMOVED),
            DiffLineType::Context => tags::Tag(tags::SYNTAX),
            _ => {
                todo!("syntax in non code line");
            }
        }
    }
}
