// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::status_view::view::Display;
use crate::status_view::ViewContainer;
use crate::{Diff, DiffKind, File, Head, Hunk, HunkLineNo, Line, State};
use gtk4::TextBuffer;
use log::trace;
use std::collections::HashSet;

impl Hunk {
    pub fn new_workdir_line(&self) -> Option<HunkLineNo> {
        match self.kind {
            DiffKind::Unstaged | DiffKind::Conflicted => Some(self.new_start),
            DiffKind::Staged => None,
            DiffKind::Commit | DiffKind::Untracked => {
                panic!("whats the case?")
            }
        }
    }
    pub fn old_tree_line(&self) -> Option<HunkLineNo> {
        match self.kind {
            DiffKind::Unstaged | DiffKind::Conflicted => None,
            DiffKind::Staged => Some(self.old_start),
            DiffKind::Commit | DiffKind::Untracked => {
                panic!("whats the case?")
            }
        }
    }

    // Hunk
    pub fn enrich_view(
        &self,
        rendered: &Hunk,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        // so, they are the same except, may be header (new_start/old_start could be changed
        // because of hunk moving) and line numbers.
        // header just made Pending. linenumbers will be rendered via Layout
        // right from data. we are good to go. lets leave rendered lines as is
        self.adopt_view(&rendered.view);
        if self.header != rendered.header {
            self.view.display.replace(Display::Pending);
        }
        if !self.is_expanded() {
            return;
        }
        self.lines
            .iter()
            .zip(rendered.lines.iter())
            .for_each(|lines: (&Line, &Line)| {
                trace!("zip on lines {:?} {:?}", context, lines);
                lines.0.enrich_view(lines.1, buffer, context);
            });
    }
}

impl File {
    // File
    pub fn enrich_view(
        &self,
        rendered: &File,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        self.adopt_view(&rendered.view);
        if !self.is_expanded() {
            return;
        }
        let mut to_remain = Vec::new();
        for hunk in self.hunks.iter() {
            // so, how to match hunk in unstaged, when index changed?
            // newStart could be changed, cause file is edited in workdir!
            // oldStart could be changed, cause Index is changed during staging!
            // where is the truth?
            if let Some(rendered_index) = rendered.hunks.iter().position(|rendered_hunk| {
                rendered_hunk
                    .old_tree_line()
                    .zip(hunk.old_tree_line())
                    .is_some_and(|(rl, nl)| rl == nl)
                    || rendered_hunk
                        .new_workdir_line()
                        .zip(hunk.new_workdir_line())
                        .is_some_and(|(rl, nl)| rl == nl)
            }) {
                // copy expansion/collapsing
                let rendered_hunk = &rendered.hunks[rendered_index];
                hunk.view.switch.replace(rendered_hunk.view.switch.get());
                if hunk.buf == rendered_hunk.buf {
                    // so, they are the same except, may be header (new_start could be changed
                    // because of hunk moving) and line numbers.
                    // header just made Pending. linenumbers will be rendered via Layout
                    // right from data. we are good to go. lets leave rendered lines as is
                    hunk.enrich_view(rendered_hunk, buffer, context);
                    to_remain.push(rendered_index);
                } else {
                    // lines were changed. lets erase old hunk then and rerender all lines
                    // for new hunk.
                }
            }
        }
        for (i, hunk) in rendered.hunks.iter().enumerate() {
            if to_remain.contains(&i) {
            } else {
                hunk.erase(buffer, context);
            }
        }
    }
}

impl Diff {
    // Diff
    pub fn enrich_view(
        &self,
        rendered: &Diff,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        self.adopt_view(&rendered.view);

        trace!(
            "---------------enrich {:?} view in diff. my files {:?}, rendered files {:?}",
            &self.kind,
            self.files.len(),
            rendered.files.len(),
        );
        let mut replaces_by_new = HashSet::new();
        for file in &self.files {
            for of in &rendered.files {
                if file.path == of.path {
                    file.enrich_view(of, buffer, context);
                    replaces_by_new.insert(file.path.clone());
                }
            }
        }
        // erase all stale views
        trace!(
            "before erasing files. replaced by new {:?} for total files count: {:?}",
            replaces_by_new,
            rendered.files.len()
        );
        rendered
            .files
            .iter()
            .filter(|f| !replaces_by_new.contains(&f.path))
            .for_each(|f| {
                trace!("context on final lines of diff render view {:?}", context);
                f.erase(buffer, context)
            });
    }
}

impl State {
    // State
    pub fn enrich_view(
        &self,
        rendered: &State,
        _buffer: &TextBuffer,
        _context: &mut crate::StatusRenderContext,
    ) {
        self.adopt_view(&rendered.view);
        // always dirty if updated!
        // self.view.dirty(true);
        self.view.display.replace(Display::Pending);
    }
}

impl Head {
    // Head
    pub fn enrich_view(
        &self,
        rendered: &Head,
        _buffer: &TextBuffer,
        _context: &mut crate::StatusRenderContext,
    ) {
        self.adopt_view(&rendered.view);
        // always dirty if updated!
        //self.view.dirty(true);
        self.view.display.replace(Display::Pending);
    }
}
