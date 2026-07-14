// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::stage_view::cursor_to_line_offset;
use crate::status_view::tags;
use crate::status_view::view::{Display, RenderOp, State, Switch, View};
use crate::status_view::Label;
use crate::{
    Diff,
    DiffKind,
    File,
    Head,
    Hunk,
    Line,
    LineKind,
    State as GitState,
    StatusRenderContext, //, Untracked, UntrackedFile,
    MARKER_OURS,
    MARKER_THEIRS,
};
use git2::{DiffLineType, RepositoryState};
use gtk4::prelude::*;
use gtk4::{Label as GtkLabel, TextBuffer, TextIter};
use libadwaita::StyleManager;
use log::{error, trace};
use std::fmt;
//pub const LINE_NO_SPACE: i32 = 6;
//ss
#[derive(PartialEq, Debug)]
pub enum TagChanges {
    Render,
    BecomeCurrent(bool),
    BecomeActive(bool),
}

pub trait ViewContainer: fmt::Display {
    fn is_expanded(&self) -> bool {
        self.get_view().is_expanded()
    }

    fn is_rendered(&self) -> bool {
        self.get_view().is_rendered()
    }

    fn is_active(&self) -> bool {
        self.get_view().is_active()
    }
    fn is_current(&self) -> bool {
        self.get_view().is_current()
    }
    fn is_transfered(&self) -> bool {
        self.get_view().is_transfered()
    }

    fn is_rendered_in(&self, line_no: i32) -> bool {
        self.get_view().is_rendered_in(line_no)
    }
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
        //view.line_no.replace(other_rendered_view.line_no.get());
        //view.flags.replace(other_rendered_view.flags.get());
        view.tag_indexes
            .replace(other_rendered_view.tag_indexes.get());
        //view.transfer(true);
        view.display.replace(other_rendered_view.display.get());
        view.switch.replace(other_rendered_view.switch.get());
        view.state.replace(other_rendered_view.state.get());
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
    fn prepare_context<'a>(&'a self, _ctx: &mut StatusRenderContext<'a>, _line_no: Option<i32>) {}

    fn fill_selected<'a>(&'a self, _context: &mut StatusRenderContext<'a>, _parent_index: usize) {}

    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, _ctx: &mut StatusRenderContext<'a>) {}
    fn after_render<'a>(
        &'a self,
        _buffer: &TextBuffer,
        _iter: &mut TextIter,
        _ctx: &mut StatusRenderContext<'a>,
    ) {
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

    fn add_tag(&self, buffer: &TextBuffer, tag: &'static str, offset_range: Option<(i32, i32)>) {
        let view = self.get_view();
        let (start_iter, end_iter) = if let Some((start, end)) = offset_range {
            (buffer.iter_at_offset(start), buffer.iter_at_offset(end))
        } else if let Display::Settled(line_no) = view.display.get() {
            self.start_end_iters(buffer, line_no)
        } else {
            panic!("tag on not settled view!");
        };
        buffer.apply_tag_by_name(tag, &start_iter, &end_iter);
        view.tag_added(tag);
    }
    fn remove_tag(&self, buffer: &TextBuffer, tag: &'static str) {
        let view = self.get_view();
        if let Some(line_no) = self.get_line_no() {
            let (start_iter, end_iter) = self.start_end_iters(buffer, line_no);
            buffer.remove_tag_by_name(tag, &start_iter, &end_iter);
            view.tag_removed(tag);
        }
    }

    // ViewContainer
    fn apply_tags<'a>(
        &'a self,
        _changes: TagChanges,
        _buffer: &TextBuffer,
        _context: &mut StatusRenderContext<'a>,
    ) {
    }

    fn get_line_no(&self) -> Option<i32> {
        let view = self.get_view();
        if let Display::Settled(line_no) = view.display.get() {
            return Some(line_no);
        }
        None
    }
    fn put_line_onto(&self, iter: &mut TextIter) {
        let view = self.get_view();
        println!("🎯 put line into {:?}", view,);
        if let Display::Settled(line_no) = view.display.get() {
            iter.set_line(line_no);
        } else {
            panic!("😲 put line on hidden view");
        }
    }

    fn render<'a>(
        &'a self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        context: &mut StatusRenderContext<'a>,
    ) {
        self.prepare_context(context, None);
        let line_no = iter.line();
        let view = self.get_view();
        let snapshot = view.snapshot(line_no);
        println!("🧄 snapshot {:?}............{}", snapshot, self);
        match snapshot {
            RenderOp::Insert => {
                self.write_content(iter, buffer, context);
                buffer.insert(iter, "\n");
                // TODO! line!
                // view.line_no.replace(line_no);
                // view.render(true);
                // before it was used only in cursor!
                // todo! tags!
                self.apply_tags(TagChanges::Render, buffer, context);
            }
            RenderOp::Skip => {
                iter.forward_lines(1);
            }
            RenderOp::Rewrite => {
                let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
                eol_iter.forward_to_line_end();
                // if content is empty - eol iter will drop onto next line!
                // no need to delete in this case!
                if iter.line() == eol_iter.line() {
                    // TODO! tags!
                    // buffer.remove_all_tags(iter, &eol_iter);
                    buffer.delete(iter, &mut eol_iter);
                }
                // TODO! tags!
                //view.cleanup_tags();
                self.write_content(iter, buffer, context);
                // before it was used only in cursor!
                // TODO! tags!
                self.apply_tags(TagChanges::Render, buffer, context);
                self.force_forward(buffer, iter);
            }
            RenderOp::Delete => {
                let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
                nel_iter.forward_lines(1);
                buffer.delete(iter, &mut nel_iter);
            }
            RenderOp::None => {}
        }
        let child_required = view.needs_children_snapshot();
        if child_required {
            println!("🧪 go expand childs {} {}", self, view);
            for child in self.get_children() {
                child.render(buffer, iter, context);
            }
        }
        self.after_render(buffer, iter, context);
    }

    // ViewContainer
    // fn old_render<'a>(
    //     &'a self,
    //     buffer: &TextBuffer,
    //     iter: &mut TextIter,
    //     context: &mut StatusRenderContext<'a>,
    // ) {
    //     self.prepare_context(context, None);

    //     let line_no = iter.line();
    //     let view = self.get_view();
    //     let state = view.get_state_for(line_no);
    //     match state {
    //         ViewState::RenderedInPlace => {
    //             trace!("..render MATCH rendered_in_line {:?}", line_no);
    //             iter.forward_lines(1);
    //         }
    //         ViewState::Deleted => {
    //             trace!("..render MATCH !rendered squashed {:?}", line_no);
    //         }
    //         ViewState::NotYetRendered => {
    //             trace!("..render MATCH insert {:?}", line_no);
    //             self.write_content(iter, buffer, context);
    //             buffer.insert(iter, "\n");

    //             view.line_no.replace(line_no);
    //             view.render(true);
    //             // before it was used only in cursor!
    //             self.apply_tags(TagChanges::Render, buffer, context);
    //         }
    //         ViewState::DirtyNotInPlace => {
    //             // just updating tags in place or not
    //             println!(
    //                 "🦴 apply tags for dirty NOT in place view {:?} curremt {:?}",
    //                 view.line_no.get(),
    //                 line_no
    //             );
    //             println!("{}", self._get_content_for_debug(context));
    //             self.apply_tags(TagChanges::Render, buffer, context);
    //             view.render(true);
    //             view.line_no.replace(line_no);
    //             self.force_forward(buffer, iter);
    //         }
    //         ViewState::DirtyInPlace => {
    //             println!("🦴 apply tags for dirty IN place");
    //             self.apply_tags(TagChanges::Render, buffer, context);
    //             view.render(true);
    //             self.force_forward(buffer, iter);
    //         }
    //         ViewState::MarkedForDeletion => {
    //             trace!("..render MATCH squashed {:?}", line_no); // kij
    //             let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();
    //             nel_iter.forward_lines(1);
    //             buffer.delete(iter, &mut nel_iter);
    //             view.render(false);
    //             view.activate(false);
    //             view.cleanup_tags();
    //         }
    //         ViewState::UpdatedFromGit(l) => {
    //             trace!(".. render MATCH UpdatedFromGit {:?}", l);
    //             view.line_no.replace(line_no);

    //             let mut eol_iter = buffer.iter_at_line(iter.line()).unwrap();
    //             eol_iter.forward_to_line_end();

    //             // if content is empty - eol iter will drop onto next line!
    //             // no need to delete in this case!
    //             if iter.line() == eol_iter.line() {
    //                 buffer.remove_all_tags(iter, &eol_iter);
    //                 buffer.delete(iter, &mut eol_iter);
    //             }
    //             view.cleanup_tags();
    //             self.write_content(iter, buffer, context);
    //             // before it was used only in cursor!
    //             self.apply_tags(TagChanges::Render, buffer, context);
    //             self.force_forward(buffer, iter);
    //         }
    //         ViewState::RenderedNotInPlace(l) => {
    //             // TODO: somehow it is related to transfered!
    //             trace!(".. render match not in place {:?}", l);
    //             view.line_no.replace(line_no);
    //             self.force_forward(buffer, iter);
    //         }
    //     }

    //     view.dirty(false);
    //     view.squash(false);
    //     view.transfer(false);

    //     if view.is_expanded() || view.is_child_dirty() {
    //         for child in self.get_children() {
    //             child.render(buffer, iter, context);
    //         }
    //     }
    //     self.get_view().child_dirty(false);
    //     self.after_render(buffer, iter, context);
    // }

    /// called before cursor to fill context.selected...
    /// ONLY FROM DIFF. so, each Diff pass all childs
    /// recursivelly, before calculating all views "active"
    /// it is require for further processing
    fn search_cursor_position<'a>(
        &'a self,
        line_no: i32,
        parent_index: usize,
        context: &mut StatusRenderContext<'a>,
    ) -> bool {
        if self.is_rendered_in(line_no) {
            self.fill_selected(context, parent_index);
            return true;
        } else {
            for (i, child) in self.get_children().iter().enumerate() {
                if child.search_cursor_position(line_no, i, context) {
                    self.fill_selected(context, parent_index);
                    return true;
                }
            }
        }
        false
    }

    fn cursor<'a>(
        &'a self,
        buffer: &TextBuffer,
        line_no: i32,
        context: &mut StatusRenderContext<'a>,
    ) {
        if !self.is_rendered() {
            return;
        }
        self.prepare_context(context, Some(line_no));

        let was_current = self.is_current();
        let was_active = self.is_active();

        let is_current = self.is_rendered_in(line_no);
        let is_active = if is_current {
            true
        } else {
            self.get_is_active(context)
        };
        let view = self.get_view();
        match (is_current, is_active) {
            (true, _) => {
                view.state.replace(State::Current);
            }
            (_, true) => {
                view.state.replace(State::Active);
            }
            _ => {
                view.state.replace(State::None);
            }
        }

        for child in self.get_children() {
            child.cursor(buffer, line_no, context);
        }

        if !self.is_empty(context) {
            if is_current != was_current {
                self.apply_tags(TagChanges::BecomeCurrent(is_current), buffer, context);
            }
            if is_active != was_active {
                self.apply_tags(TagChanges::BecomeActive(is_active), buffer, context);
            }
        }
        self.after_cursor(buffer, context);
    }

    // base
    fn get_is_active(&self, _context: &mut StatusRenderContext) -> bool {
        false
    }

    fn expand(&self, line_no: i32) -> Option<i32> {
        let view = self.get_view();
        view.toggle(line_no);
        match view.switch.get() {
            // hey
            Switch::PendingExpansion => {
                println!("🌻 Switch::PendingExpansion {}", self);
                self.walk_down(&mut |vc: &dyn ViewContainer| {
                    let view = vc.get_view();
                    view.display.replace(Display::None);
                });
                Some(line_no)
            }
            Switch::PendingCollapsion => {
                println!("🌻 Switch::PendingCollapsion {}", self);
                self.walk_down(&mut |vc: &dyn ViewContainer| {
                    let view = vc.get_view();
                    view.display.replace(Display::Trashed);
                });
                Some(line_no)
            }
            Switch::Expanded => {
                println!("🌻 Switch::Expanded");
                let mut result: Option<i32> = None;
                self.walk_down(&mut |vc: &dyn ViewContainer| {
                    if let Some(vc_line_no) = vc.expand(line_no) {
                        result.replace(vc_line_no);
                    }
                });
                result
            }
            _ => None,
        }
    }
    // ViewContainer
    // fn old_expand(&self, line_no: i32, context: &mut StatusRenderContext) -> Option<i32> {
    //     // check
    //     let mut found_line: Option<i32> = None;
    //     let v = self.get_view();
    //     if v.is_rendered_in(line_no) {
    //         let view = self.get_view();
    //         found_line = Some(line_no);
    //         view.expand(!view.is_expanded());
    //         view.child_dirty(true);
    //         let expanded = view.is_expanded();
    //         self.walk_down(&mut |vc: &dyn ViewContainer| {
    //             let view = vc.get_view();
    //             if expanded {
    //                 view.squash(false);
    //                 view.render(false);
    //             } else {
    //                 view.squash(true);
    //                 //view.child_dirty(true);  // ADD THIS deepseek
    //             }
    //         });
    //     } else if v.is_expanded() && v.is_rendered() {
    //         // go deeper for self.children
    //         for child in self.get_children() {
    //             found_line = child.expand(line_no, context);
    //             if found_line.is_some() {
    //                 break;
    //             }
    //         }
    //         if found_line.is_some() && self.is_expandable_by_child() {
    //             let line_no = self.get_view().line_no.get();
    //             return self.expand(line_no, context);
    //         }
    //     }
    //     found_line
    // }

    fn is_expandable_by_child(&self) -> bool {
        false
    }

    fn erase(&self, buffer: &TextBuffer, context: &mut StatusRenderContext) {
        // CAUTION. ATTENTION. IMPORTANT
        // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
        // after this operation all prev iters become INVALID!
        // it need to reobtain them!

        // this ONLY rendering. the data remains
        // unchaged. means it used to be called just
        // before replacing data in status struct.
        // CAUTION. ATTENTION. IMPORTANT

        // ALL ERASE AND RENDER PROCESSES MUST STRICLTY GO FROM TOP TO BOTTOM!
        if !self.is_rendered() {
            return;
        }

        let iter = buffer.iter_at_offset(buffer.cursor_position());
        let initial_line_offset = iter.line_offset();

        // it does not required here. erase will kill em all
        // let mut applied_tags = HashSet::new();

        // it does not required here. erase will kill em all
        // for tag in view.added_tags() {
        //     applied_tags.insert(tag.name().to_string());
        // }

        // let mut line_no = view.line_no.get();
        let line_no = self.get_line_no().unwrap() - context.erase_counter;
        let mut iter = buffer.iter_at_line(line_no).unwrap();
        let mut nel_iter = buffer.iter_at_line(iter.line()).unwrap();

        nel_iter.forward_lines(1);
        context.erase_counter += 1;
        // buffer.delete(&mut iter, &mut nel_iter);

        if self.is_expanded() {
            self.walk_down(&mut |vc: &dyn ViewContainer| {
                // it does not required here. erase will kill em all
                // for tag in view.added_tags() {
                //     applied_tags.insert(tag.name().to_string());
                // }
                if !vc.is_rendered() {
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
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>, line_no: Option<i32>) {
        ctx.current_diff = Some(self);
        if let Some(line_no) = line_no {
            // line_no passed in calling from cursor
            self.search_cursor_position(line_no, 0, ctx);
        }
    }

    /// Diff is active when cursor is on Diff
    /// or something inside Diff
    fn get_is_active(&self, context: &mut StatusRenderContext) -> bool {
        if let Some(diff) = context.selected_diff {
            return std::ptr::eq(diff, self);
        }
        false
    }

    // Diff
    fn apply_tags<'a>(
        &'a self,
        _tag_changes: TagChanges,
        buffer: &TextBuffer,
        _ctx: &mut StatusRenderContext<'a>,
    ) {
        self.add_tag(buffer, tags::DIFF, None);
    }

    // Diff
    fn expand(&self, line_no: i32) -> Option<i32> {
        if self.kind == DiffKind::Untracked {
            return None;
        }
        let mut result: Option<i32> = None;
        let expand_whole_diff = self.is_rendered_in(line_no);
        if expand_whole_diff {
            result.replace(line_no);
        }
        let all_expanded = self.files.iter().all(|f| f.is_expanded());
        let all_collapsed = self.files.iter().all(|f| !f.is_expanded());
        let mut expand_all_files = true;
        if !all_expanded && !all_collapsed {
            // if some files are expanded and some are collapsed
            // lets look at first file. if it is expanded - expand all
            // (this is for the case of automatic expansion of first hunk)
            expand_all_files = self.files[0].is_expanded();
        }
        for file in &self.files {
            if expand_whole_diff {
                if all_expanded || all_collapsed {
                    println!("💨 all equal");
                    if let Some(line_no) = file.get_line_no() {
                        file.expand(line_no);
                    }
                } else {
                    println!(
                        "‼️ expand collapsed vs collaps expanded {:?}",
                        self.is_expanded()
                    );
                    if expand_all_files {
                        if !file.is_expanded() {
                            if let Some(line_no) = file.get_line_no() {
                                file.expand(line_no);
                            }
                        }
                    } else if file.is_expanded() {
                        if let Some(line_no) = file.get_line_no() {
                            file.expand(line_no);
                        }
                    }
                }
            } else if let Some(line) = file.expand(line_no) {
                result.replace(line);
            }
        }
        result
    }

    // Diff
    fn fill_selected<'a>(&'a self, context: &mut StatusRenderContext<'a>, _parent_index: usize) {
        context.selected_diff = Some(self);
    }

    // Diff
    fn after_render<'a>(
        &'a self,
        buffer: &TextBuffer,
        iter: &mut TextIter,
        _ctx: &mut StatusRenderContext<'a>,
    ) {
        // used to wrap all diff in tags.
        // it is used
        // while handling user clicks inside stage_view
        if let Some(start_line) = self.get_line_no() {
            let end_line = iter.line();
            match self.kind {
                DiffKind::Unstaged | DiffKind::Staged => {
                    let tag = if self.kind == DiffKind::Staged {
                        tags::STAGED
                    } else {
                        tags::UNSTAGED
                    };
                    let start_iter = buffer.iter_at_line(start_line);
                    let end_iter = buffer.iter_at_line(end_line);
                    if let (Some(start_iter), Some(mut end_iter)) = (start_iter, end_iter) {
                        end_iter.forward_to_line_end();
                        let offsets = Some((start_iter.offset(), end_iter.offset()));
                        self.remove_tag(buffer, tag);
                        self.add_tag(buffer, tag, offsets);
                    }
                }
                _ => {}
            }
        }
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
        format!("file: {:?} at line {:?}", self.path, self.get_line_no())
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

    /// Files are active when cursor is on Diff or File
    /// or something inside File
    fn get_is_active(&self, context: &mut StatusRenderContext) -> bool {
        if let Some((file, _)) = context.selected_file {
            return std::ptr::eq(file, self);
        } else if let Some(diff) = context.selected_diff {
            // no selected file but selected diff means cursor is on diff
            // and diff for this file is current_diff
            return std::ptr::eq(diff, context.current_diff.unwrap());
        }
        false
    }

    // file
    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, _context: &mut StatusRenderContext<'a>) {}

    // File
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>, _line_no: Option<i32>) {
        ctx.current_file = Some(self);
    }

    // File
    fn fill_selected<'a>(&'a self, context: &mut StatusRenderContext<'a>, parent_index: usize) {
        context.selected_file = Some((self, parent_index));
    }
}

impl ViewContainer for Hunk {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        false
    }

    fn _get_content_for_debug(&self, _context: &mut StatusRenderContext<'_>) -> String {
        format!("hunk: {:?} at line {:?}", self.header, self.get_line_no())
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
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>, _line_no: Option<i32>) {
        ctx.current_hunk = Some(self);
    }

    // Hunk
    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, ctx: &mut StatusRenderContext<'a>) {
        if let Some(line_no) = self.get_line_no() {
            ctx.collect_hunk_highlights(line_no);
        }
    }

    /// Hunk is active when cursor is on Diff or File or self
    /// or something inside self
    fn get_is_active(&self, context: &mut StatusRenderContext) -> bool {
        if let Some((hunk, _)) = context.selected_hunk {
            return std::ptr::eq(hunk, self);
        } else if let Some((file, _)) = context.selected_file {
            // cursor is on file
            return std::ptr::eq(file, context.current_file.unwrap());
        } else if let Some(diff) = context.selected_diff {
            // cursor is on diff
            return std::ptr::eq(diff, context.current_diff.unwrap());
        }
        false
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
    fn fill_selected<'a>(&'a self, ctx: &mut StatusRenderContext<'a>, parent_index: usize) {
        ctx.selected_hunk = Some((self, parent_index));
    }
}

pub const LINENO_MARGIN: &str = "     ";

impl ViewContainer for Line {
    fn is_empty(&self, _context: &mut StatusRenderContext<'_>) -> bool {
        // lines could not be empty
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
            self.get_line_no()
        )
    }

    // Line
    fn after_cursor<'a>(&'a self, _buffer: &TextBuffer, ctx: &mut StatusRenderContext<'a>) {
        if let Some(line_no) = self.get_line_no() {
            if self.is_active() {
                ctx.collect_line_highlights(line_no);
            }
            let source_line_no = self
                .new_line_no
                .map(|num| num.as_i32())
                .unwrap_or(self.old_line_no.map(|num| num.as_i32()).unwrap_or(0));

            let line_no_text = match self.origin {
                DiffLineType::Deletion => match source_line_no {
                    0..10 => "-".to_string(),
                    10..100 => " -".to_string(),
                    100..1000 => "  -".to_string(),
                    _ => "   -".to_string(),
                },
                _ => format!("{}", line_no),
            };
            ctx.linenos
                .insert(line_no, (line_no_text, self.origin, self.kind.clone()));
        }
    }

    // Line
    fn expand(&self, line_no: i32) -> Option<i32> {
        if let Some(my_line_no) = self.get_line_no() {
            if my_line_no == line_no {
                // looks like its just used to break loop
                return Some(line_no);
            }
        }
        None
    }

    // Line
    // it is useless. rendering_x is sliding variable during render
    // and there is nothing to render after line
    fn prepare_context<'a>(&'a self, ctx: &mut StatusRenderContext<'a>, _line_no: Option<i32>) {
        ctx.previous_line = ctx.current_line;
        ctx.current_line = Some(self);
    }

    // Line
    fn fill_selected<'a>(&'a self, ctx: &mut StatusRenderContext<'a>, parent_index: usize) {
        ctx.selected_line = Some((self, parent_index));
    }

    /// Lines are active when cursor is on their Diff or file or hunk
    /// or
    /// 1. in normal case - on any line inside hunk
    /// 2. in case of conflict - on any line of same side
    fn get_is_active(&self, context: &mut StatusRenderContext) -> bool {
        if context.current_diff.unwrap().kind == DiffKind::Conflicted {
            // case 2.
            if let Some((line, _)) = context.selected_line {
                if std::ptr::eq(line, self) {
                    return true;
                }
                let (selected_hunk, _) = context.selected_hunk.unwrap();
                if std::ptr::eq(selected_hunk, context.current_hunk.unwrap()) {
                    // selected line is in the same hunk as me
                    return match (&line.kind, &self.kind) {
                        (LineKind::Ours(_), LineKind::Ours(_)) => true,
                        (LineKind::ConflictMarker(marker), LineKind::Ours(_))
                            if marker == MARKER_OURS =>
                        {
                            true
                        }
                        (LineKind::Theirs(_), LineKind::Theirs(_)) => true,
                        (LineKind::ConflictMarker(marker), LineKind::Theirs(_))
                            if marker == MARKER_THEIRS =>
                        {
                            true
                        }
                        _ => false,
                    };
                }
            }
        } else {
            // case 1 - normal
            if let Some((line, _)) = context.selected_line {
                if std::ptr::eq(line, self) {
                    return true;
                }
                let (selected_hunk, _) = context.selected_hunk.unwrap();
                if std::ptr::eq(selected_hunk, context.current_hunk.unwrap()) {
                    return true;
                }
            } else if let Some((hunk, _)) = context.selected_hunk {
                // cursor is on hunk
                return std::ptr::eq(hunk, context.current_hunk.unwrap());
            } else if let Some((file, _)) = context.selected_file {
                // cursor is on file
                return std::ptr::eq(file, context.current_file.unwrap());
            } else if let Some(diff) = context.selected_diff {
                // cursor is on diff
                return std::ptr::eq(diff, context.current_diff.unwrap());
            }
        }
        false
    }

    // Line
    fn write_content(
        &self,
        iter: &mut TextIter,
        buffer: &TextBuffer,
        context: &mut StatusRenderContext<'_>,
    ) {
        let content = self.content(context.current_hunk.unwrap());
        if content.is_empty() {
            // when add or delete single line, mark it somehow to be visible
            match (self.origin, context.previous_line.map(|l| l.origin)) {
                (DiffLineType::Deletion, Some(DiffLineType::Context))
                | (DiffLineType::Addition, Some(DiffLineType::Context)) => {
                    buffer.insert(iter, "\\n");
                }
                _ => {
                    buffer.insert(iter, " ");
                }
            }
        } else {
            // MARGIN FOR LINENO
            buffer.insert(iter, LINENO_MARGIN);
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
        if let Some(line_no) = self.get_line_no() {
            let (mut start_iter, end_iter) = self.start_end_iters(buffer, line_no);
            let start_offset = start_iter.offset();
            let hunk = context.current_hunk.unwrap();
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
                        start_iter.forward_chars((stripped_len + LINENO_MARGIN.len()) as i32);
                        self.add_tag(
                            buffer,
                            spaces_tag,
                            Some((start_iter.offset(), end_iter.offset())),
                        );
                    }

                    self.fill_syntax_tags(
                        self.choose_syntax_tag().0,
                        &hunk.keyword_ranges,
                        buffer,
                        start_offset,
                    );
                    self.fill_syntax_tags(
                        self.choose_syntax_1_tag().0,
                        &hunk.identifier_ranges,
                        buffer,
                        start_offset,
                    );

                    // cleanup previous search tags
                    self.remove_tag(buffer, tags::MATCH_HIGHLIGHT);
                    self.remove_tag(buffer, tags::CURRENT_MATCH_HIGHLIGHT);
                    //println!("🪛 LINE fill syntax tags {:?} found? {:?}", self.new_line_no, &hunk.search_ranges.len());
                    if self.fill_syntax_tags(
                        tags::MATCH_HIGHLIGHT,
                        &hunk.search_ranges,
                        buffer,
                        start_offset,
                    ) {
                        if let Some(line_no) = self.get_line_no() {
                            context.search_matched_lines.push(line_no);
                        }
                    }
                    match self.kind {
                        LineKind::ConflictMarker(_) => self.add_tag(buffer, tags::REMOVED, None),
                        // no need to mark theirs/ours. use regular colors downwhere
                        LineKind::Ours(_) | LineKind::Theirs(_) => {
                            self.add_tag(buffer, tags::ADDED, None)
                        }
                        _ => self.add_tag(buffer, self.choose_tag().0, None),
                    }
                }

                TagChanges::BecomeCurrent(_) => {
                    if let Some(line_no) = self.get_line_no() {
                        let mut iter = buffer.iter_at_offset(0);
                        iter.set_line(line_no);
                        if let Some(anchor) = iter.child_anchor() {
                            if !anchor.widgets().is_empty() {
                                let w = &anchor.widgets()[0];
                                let l = w.downcast_ref::<GtkLabel>().unwrap();
                                if self.is_current() {
                                    l.set_opacity(1.0);
                                } else {
                                    l.set_opacity(0.3);
                                }
                            }
                        }
                    }
                }
                TagChanges::BecomeActive(is_active) => {
                    self.remove_tag(buffer, self.choose_tag().0);
                    self.remove_tag(buffer, self.choose_tag().enhance().0);
                    self.remove_tag(buffer, self.choose_syntax_tag().0);
                    self.remove_tag(buffer, self.choose_syntax_tag().enhance().0);
                    self.remove_tag(buffer, self.choose_syntax_1_tag().0);
                    self.remove_tag(buffer, self.choose_syntax_1_tag().enhance().0);

                    if is_active {
                        self.fill_syntax_tags(
                            self.choose_syntax_tag().enhance().0,
                            &hunk.keyword_ranges,
                            buffer,
                            start_offset,
                        );
                        self.fill_syntax_tags(
                            self.choose_syntax_1_tag().enhance().0,
                            &hunk.identifier_ranges,
                            buffer,
                            start_offset,
                        );
                    } else {
                        self.fill_syntax_tags(
                            self.choose_syntax_tag().0,
                            &hunk.keyword_ranges,
                            buffer,
                            start_offset,
                        );
                        self.fill_syntax_tags(
                            self.choose_syntax_1_tag().0,
                            &hunk.identifier_ranges,
                            buffer,
                            start_offset,
                        );
                    }
                    match self.kind {
                        LineKind::ConflictMarker(_) => {
                            if is_active {
                                self.add_tag(buffer, tags::Tag(tags::REMOVED).enhance().0, None)
                            } else {
                                self.add_tag(buffer, tags::REMOVED, None)
                            }
                        }
                        // no need to mark theirs/ours. use regular colors downwhere
                        LineKind::Ours(_) | LineKind::Theirs(_) => {
                            if is_active {
                                self.add_tag(buffer, tags::Tag(tags::ADDED).enhance().0, None)
                            } else {
                                self.add_tag(buffer, tags::ADDED, None)
                            }
                        }
                        _ => {
                            if is_active {
                                self.add_tag(buffer, self.choose_tag().enhance().0, None);
                            } else {
                                self.add_tag(buffer, self.choose_tag().0, None);
                            }
                        }
                    }
                }
            }
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
            if let Some(line_no) = self.get_line_no() {
                let iter = buffer.iter_at_line(line_no).unwrap();
                let range = Some((iter.offset() + 11, iter.offset() + 18));
                self.add_tag(buffer, tags::POINTER, range);
                self.add_tag(buffer, tags::OID, range);
            }
        }
    }
}

impl ViewContainer for GitState {
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
        buffer.insert(iter, "State:     ");
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
    pub fn auto_expand(&self) {
        if self.is_empty() {
            error!("EXPANDING EMPTY DIFF");
            return;
        }
        //self.files[0].view.expand(true);
        self.files[0].view.set_switch(true);
    }
}

impl Line {
    fn choose_tag(&self) -> tags::Tag {
        match self.origin {
            DiffLineType::Addition => tags::Tag(tags::ADDED),
            DiffLineType::Deletion => tags::Tag(tags::REMOVED),
            _ => tags::Tag(tags::CONTEXT),
        }
    }
    fn choose_syntax_tag(&self) -> tags::Tag {
        match self.origin {
            DiffLineType::Addition => tags::Tag(tags::SYNTAX_ADDED),
            DiffLineType::Deletion => tags::Tag(tags::SYNTAX_REMOVED),
            _ => tags::Tag(tags::SYNTAX),
        }
    }
    fn choose_syntax_1_tag(&self) -> tags::Tag {
        match self.origin {
            DiffLineType::Addition => tags::Tag(tags::SYNTAX_1_ADDED),
            DiffLineType::Deletion => tags::Tag(tags::SYNTAX_1_REMOVED),
            _ => tags::Tag(tags::SYNTAX_1),
        }
    }

    fn fill_syntax_tags(
        &self,
        tag: &'static str,
        ranges: &[(usize, usize)],
        buffer: &TextBuffer,
        start_offset: i32,
    ) -> bool {
        let mut tag_added = false;
        for (start, end) in self.byte_indexes_to_char_indexes(ranges) {
            // MARGIN FOR LINENO
            let line_no_margin = 4;
            // if tag == tags::MATCH_HIGHLIGHT {
            //     println!("💰 LINE fill seatch tags {:?}", self.new_line_no);
            // }
            self.add_tag(
                buffer,
                tag,
                Some((
                    start_offset + start + line_no_margin,
                    start_offset + end + 1 + line_no_margin,
                )),
            );
            tag_added = true
        }
        tag_added
    }
}
