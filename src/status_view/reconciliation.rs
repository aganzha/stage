// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::status_view::render::View;
use crate::status_view::ViewContainer;
use crate::{
    Diff, DiffKind, File, Head, Hunk, Line, State, Untracked, UntrackedFile,
};
use git2::RepositoryState;
use gtk4::TextBuffer;
use log::{trace, debug};
use std::collections::{HashSet, HashMap};
use std::iter::zip;

pub const MAX_LINES: i32 = 50000;

impl Line {
    // line
    pub fn enrich_view(
        &mut self,
        rendered: &Line,
        _context: &mut crate::StatusRenderContext,
    ) {
        self.view = rendered.transfer_view();
        if self.content != rendered.content || self.origin != rendered.origin {
            trace!("mark dirty while enrich view in line");
            self.view.dirty(true);
            // line.view.replace(View{rendered: true, ..line.view.get()});
            trace!("*************dirty content in reconciliation: {} <> {} origins: {:?} {:?}",
                   self.content,
                   rendered.content,
                   self.origin,
                   rendered.origin
            )
        }
    }
    // line
    pub fn transfer_view(&self) -> View {
        let clone = self.view.clone();
        clone.transfer(true);
        clone
    }
}

impl Hunk {
    // Hunk
    pub fn transfer_view(&self) -> View {
        let clone = self.view.clone();
        // hunk headers are changing always
        // during partial staging
        trace!("mark dirty 2. HUNK");
        clone.dirty(true);
        clone.transfer(true);
        clone
    }

    // hunk
    pub fn enrich_view(
        &mut self,
        rendered: &mut Hunk,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        trace!("enriching hunk {} with {}", self.header, rendered.header);
        self.view = rendered.transfer_view();
        if !self.view.is_expanded() {
            return
        }
        // TODO! expanded/collapsed!


        // trace!("---------------> NEW");
        // for line in &self.lines {
        //     trace!("{}", line.repr("", 5));
        // }
        // trace!("");
        // trace!("---------------> OLD");
        // for line in &rendered.lines {
        //     trace!("{}", line.repr("", 5));
        // }
        // trace!("");
        // trace!("GOOOOOOOOOOOOOOOOOOOOO");

        // iter over new lines. normalize line_nos to hunk start.
        let mut rendered_map:HashMap<(i32, i32), &Line> = HashMap::new();

        for line in &rendered.lines {
            let mut new: i32 = 0 - 1;
            let mut old: i32 = 0 - 1;
            if let Some(new_start) = line.new_line_no {
                new = (new_start - rendered.new_start) as i32;
            }
            if let Some(old_start) = line.old_line_no {
                old = (old_start - rendered.old_start) as i32;
            }
            rendered_map.insert((new, old), &line);
        }
        for line in &mut self.lines {
            let mut new: i32 = 0 - 1;
            let mut old: i32 = 0 - 1;
            if let Some(new_start) = line.new_line_no {
                new = (new_start - self.new_start) as i32;
            }
            if let Some(old_start) = line.old_line_no {
                old = (old_start - self.old_start) as i32;
            }
            if let Some(old_line) = rendered_map.remove(&(new, old)) {
                trace!("{}", line.repr("ENRICH", 5));
                line.enrich_view(old_line, context);
            } else {
                trace!("{}", line.repr("NEW", 5));
            }
        }
        let mut srt = rendered_map.into_iter().map(|x|x).collect::<Vec<((i32, i32), &Line)>>();
        srt.sort_by(|x, y| x.0.cmp(&y.0));
        for (_, line) in  &srt {
            trace!("{}", line.repr("ERASE", 5));
            line.erase(buffer, context);
        }
    }
}

impl File {

    pub fn enrich_view(
        &mut self,
        rendered: &mut File,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        self.view = rendered.transfer_view();
        if !self.view.is_expanded() {
            return
        }
        for h in &rendered.hunks {
            trace!("RENDERED: {}", h.header);
        }
        for h in &self.hunks {
            trace!("NEW: {}", h.header);
        }

        // @@@@@@@@@@@@@@@@@ there are FEWER NEW ones than old ones
        // have 3 hunks in unstaged
        // @@ -11,7 +11,8 @@ const path = require('path');
        // @@ -106,9 +107,9 @@ function getDepsList() {
        // @@ -128,7 +129,8 @@ function getDepsList() {


        // 1.1 kill top one
        // will have new
        // @@ -106,9 +106,9 @@ function getDepsList() {
        // @@ -128,7 +128,8 @@ function getDepsList() {

        // 1.2 stage top one
        // will get new in staged
        // @@ -107,9 +107,9 @@ function getDepsList() {
        // @@ -129,7 +129,8 @@ function getDepsList() {


        // @@@@@@@@@@@@@@@@ there are MORE NEW ones than old ones
        // have 2 hunks
        // @@ -107,9 +107,9 @@ function getDepsList() {
        // @@ -129,7 +129,8 @@ function getDepsList() {

        // 2.1 unstage one for top and will have
        // @@ -11,7 +11,8 @@ const path = require('path');
        // @@ -106,9 +107,9 @@ function getDepsList() {
        // @@ -128,7 +129,8 @@ function getDepsList() {

        // have 2 hunks
        // @@ -106,9 +106,9 @@ function getDepsList() {
        // @@ -128,7 +128,8 @@ function getDepsList() {

        // 2.2 intoduce (via editing) top one
        // @@ -11,7 +11,8 @@ const path = require('path');
        // @@ -106,9 +107,9 @@ function getDepsList() {
        // @@ -128,7 +129,8 @@ function getDepsList() {

        // so. the hunk which is not matched first, determine next cycle.
        // if first is rendered - erase it and use rendered_delta. delta compared to new
        // if first is new, use new_delta. delta compared to rendered.

        // case 3 - different number of lines
        // new header      @@ -1876,7 +1897,8 @@ class DutyModel(WarehouseEdiDocument, LinkedNomEDIMixin):
        // rendered header @@ -1876,7 +1897,7 @@ class DutyModel(WarehouseEdiDocument, LinkedNomEDIMixin):


        let mut in_rendered = 0;
        let mut in_new = 0;
        let mut rendered_delta: i32 = 0;
        let mut new_delta: i32 = 0;
        let mut guard = 0;

        pub fn increment_delta(delta: i32, line_from: u32, line_to: u32) {
        }
        loop {
            guard += 1;
            if guard > 1000 {
                panic!("infinite loop in reconciliation rendered {:?} in_rendered {:?} new {:?} in_new {:?}",
                       rendered.hunks.len(),
                       in_rendered,
                       self.hunks.len(),
                       in_new
                );
            }
            if in_rendered == rendered.hunks.len() {
                trace!("rendered hunks are over!");
                break;
            }
            if in_new == self.hunks.len() {
                trace!("new hunks are over!");
                loop {
                    let rndrd = &mut rendered.hunks[in_rendered];
                    rndrd.erase(buffer, context);
                    in_rendered += 1;
                    if in_rendered == rendered.hunks.len() {
                        break;
                    }
                }
                break;
            }
            let rendered = &mut rendered.hunks[in_rendered];
            let new = &mut self.hunks[in_new];
            if rendered_delta != 0 {
                trace!("A.....has rendered delta");
                // rendered was erased
                if rendered.header == Hunk::shift_new_start(&new.header, rendered_delta)  // 1.1
                    ||
                    rendered.header == Hunk::shift_old_start(&new.header, 0 - rendered_delta) { // 1.2
                        // matched!
                        trace!("++++++enrich case 1.1 or 1.2");
                        new.enrich_view(rendered, buffer, context);
                        in_new += 1;
                        in_rendered += 1;
                    } else {
                        // proceed with erasing
                        trace!("------erase case 1.1 or 1.2");
                        in_rendered += 1;
                        rendered.erase(buffer, context);
                        // @delta
                        let new_lines = rendered.new_lines as i32;
                        let old_lines = rendered.old_lines as i32;
                        rendered_delta += new_lines - old_lines;
                    }
            } else if new_delta != 0 {
                // new was inserted
                trace!("B..... has new delta");
                if new.header == Hunk::shift_new_start(&rendered.header, new_delta)
                    ||
                    new.header == Hunk::shift_old_start(&rendered.header, 0 - new_delta) {
                        trace!("++++++++ enrich cases 2.1 or 2.2 ");
                        new.enrich_view(rendered, buffer, context);
                        in_new += 1;
                        in_rendered += 1;
                    } else {
                        trace!("++++++++ skip cases 2.1 or 2.2 ");
                        in_new += 1;
                        // @delta
                        let new_lines = new.new_lines as i32;
                        let old_lines = new.old_lines as i32;
                        new_delta += new_lines - old_lines;
                    }

            } else {
                // first loop or loop on equal hunks
                trace!("C.....first loop or loop on equal hunks");
                if rendered.header == new.header {
                    // same hunks
                    trace!("just free first match");
                    new.enrich_view(rendered, buffer, context);
                    in_new += 1;
                    in_rendered += 1;
                } else if rendered.new_start == new.new_start && rendered.old_start == new.old_start {
                    trace!("hunks are same, but number of lines are changed");

                    // @delta
                    let new_lines = new.new_lines as i32;
                    let rendered_lines = rendered.new_lines as i32;
                    rendered_delta += new_lines - rendered_lines;
                    
                    trace!("changed rendered delta {}", rendered_delta);

                    new.enrich_view(rendered, buffer, context);
                    in_new += 1;
                    in_rendered += 1;
                } else {
                    trace!("hunks are not equal r_start {} r_lines {} n_start {} n_lines {}",
                           rendered.new_start,
                           rendered.new_lines,
                           new.new_start,
                           new.new_lines);
                    if new.new_start < rendered.new_start && new.old_start < rendered.old_start {
                        // cases 2.1 and 2.1 - insert first new hunk
                        trace!("first new hunk without rendered. SKIP");
                        in_new += 1;

                        // @delta
                        let new_lines = new.new_lines as i32;
                        let old_lines = new.old_lines as i32;
                        new_delta = new_lines - old_lines;
                        
                    } else if new.new_start > rendered.new_start && new.old_start > rendered.old_start {
                        // cases 1.1 and 1.2 - delete first rendered hunk
                        trace!("first rendered hunk without new. ERASE");
                        in_rendered += 1;

                        // @delta
                        let new_lines = rendered.new_lines as i32;
                        let old_lines = rendered.old_lines as i32;
                        rendered_delta = new_lines - old_lines;

                        rendered.erase(buffer, context);
                        
                    } else {
                        panic!("UNKNOWN CASE IN RECONCILIATION {} {}", new.header, rendered.header);
                    }
                }
            }
        }
    }

    // // File
    pub fn transfer_view(&self) -> View {
        let clone = self.view.clone();
        clone.transfer(true);
        clone
    }
}

impl Diff {
    pub fn enrich_view(
        &mut self,
        rendered: &mut Diff,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        context.diff_kind.replace(self.kind.clone());

        trace!("---------------enrich {:?} view in diff. my files {:?}, rendered files {:?}",
               &self.kind,
               self.files.len(),
               rendered.files.len(),
        );
        let mut replaces_by_new = HashSet::new();
        for file in &mut self.files {
            for of in &mut rendered.files {
                if file.path == of.path {
                    file.enrich_view(of, buffer, context);
                    replaces_by_new.insert(file.path.clone());
                }
            }
        }
        // erase all stale views
        trace!("before erasing files. replaced by new {:?} for total files count: {:?}", replaces_by_new, rendered.files.len());
        rendered
            .files
            .iter_mut()
            .filter(|f| !replaces_by_new.contains(&f.path))
            .for_each(|f| {
                trace!(
                    "context on final lines of diff render view {:?}",
                    context
                );
                f.erase(buffer, context)
            });
    }
}

impl UntrackedFile {
    pub fn enrich_view(
        &mut self,
        rendered: &UntrackedFile,
        _context: &mut crate::StatusRenderContext,
    ) {
        self.view = rendered.transfer_view();
    }

    pub fn transfer_view(&self) -> View {
        let clone = self.view.clone();
        clone.transfer(true);
        clone
    }
}

impl Untracked {
    pub fn enrich_view(
        &mut self,
        rendered: &mut Untracked,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        let mut replaces_by_new = HashSet::new();
        for file in &mut self.files {
            for of in &mut rendered.files {
                if file.path == of.path {
                    file.enrich_view(of, context);
                    replaces_by_new.insert(file.path.clone());
                }
            }
        }
        rendered
            .files
            .iter_mut()
            .filter(|f| !replaces_by_new.contains(&f.path))
            .for_each(|f| {
                trace!(
                    "context on final lines of diff render view {:?}",
                    context
                );
                f.erase(buffer, context)
            });
    }
}

impl Head {
    // head
    pub fn enrich_view(&mut self, rendered: &Head) {
        self.view = rendered.transfer_view();
    }
    // head
    pub fn transfer_view(&self) -> View {
        let clone = self.view.clone();
        clone.transfer(true);
        clone.dirty(true);
        clone
    }
}

impl State {
    // state
    pub fn enrich_view(&mut self, rendered: &Self) {
        self.view = rendered.transfer_view();
        self.view.squash(self.state == RepositoryState::Clean);
    }
    // state
    pub fn transfer_view(&self) -> View {
        let clone = self.view.clone();
        clone.transfer(true);
        clone.dirty(true);
        clone
    }
}
