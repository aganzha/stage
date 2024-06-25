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
        self.view = rendered.transfer_view();
        if !self.view.is_expanded() {
            return
        }
        // TODO! expanded/collapsed!


        // debug!("---------------> NEW");
        // for line in &self.lines {
        //     debug!("{}", line.repr("", 5));
        // }
        // debug!("");
        // debug!("---------------> OLD");
        // for line in &rendered.lines {
        //     debug!("{}", line.repr("", 5));
        // }
        // debug!("");
        // debug!("GOOOOOOOOOOOOOOOOOOOOO");

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
            debug!("{}", line.repr("ERASE", 5));
            line.erase(buffer, context);
        }
    }


    // pub fn enrich_view_old(
    //     &mut self,
    //     rendered: &mut Hunk,
    //     buffer: &TextBuffer,
    //     context: &mut crate::StatusRenderContext,
    // ) {
    //     self.view = rendered.transfer_view();
    //     if self.lines.len() == rendered.lines.len() {
    //         for pair in zip(&mut self.lines, &rendered.lines) {
    //             pair.0.enrich_view(pair.1, context);
    //         }
    //         return;
    //     }
    //     // all lines are ordered
    //     let (mut r_ind, mut n_ind) = (0, 0);
    //     let mut guard = 0;
    //     trace!("++++++++++++++++ line roconciliation");
    //     loop {
    //         trace!("++++++loop");
    //         guard += 1;
    //         if guard > MAX_LINES {
    //             panic!("guard");
    //         }
    //         let r_line = &rendered.lines[r_ind];
    //         let n_line = &self.lines[n_ind];
    //         // hunks could be shifted
    //         // and line_nos could differ a lot
    //         // it need to find first matched line
    //         // THIS IS ACTUAL ONLY FOR UNSTAGED
    //         match (r_line.old_line_no, n_line.old_line_no) {
    //             (Some(r_no), Some(n_no)) => {
    //                 trace!("both lines are changed");
    //                 trace!("r_no n_no {:?} {:?}", r_no, n_no);
    //                 let m_n_line = &mut self.lines[n_ind];
    //                 m_n_line.enrich_view(r_line, context);
    //                 r_ind += 1;
    //                 n_ind += 1;
    //             }
    //             (Some(r_no), None) => {
    //                 trace!("new line is added before old one");
    //                 trace!("r_no n_no {:?} _", r_no);
    //                 n_ind += 1;
    //             }
    //             (None, Some(n_no)) => {
    //                 trace!("rendered line is added before new one");
    //                 trace!("r_no n_no _ {:?}", n_no);
    //                 let m_r_line = &mut rendered.lines[r_ind];
    //                 m_r_line.erase(buffer, context);
    //                 r_ind += 1;
    //             }
    //             (None, None) => {
    //                 trace!("both lines are added",);
    //                 trace!("r_no n_no _ _");
    //                 let m_n_line = &mut self.lines[n_ind];
    //                 m_n_line.enrich_view(r_line, context);
    //                 r_ind += 1;
    //                 n_ind += 1;
    //             }
    //         }
    //         trace!("");
    //         if r_ind == rendered.lines.len() {
    //             trace!("rendered lines are over");
    //             break;
    //         }
    //         if n_ind == self.lines.len() {
    //             trace!("new lines are over");
    //             for r_line in &mut rendered.lines[r_ind..] {
    //                 trace!("erase remainign rendered lines!");
    //                 r_line.erase(buffer, context);
    //             }
    //             break;
    //         }
    //     }
    // }
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
            debug!("RENDERED: {}", h.header);
        }
        for h in &self.hunks {
            debug!("NEW: {}", h.header);
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
        loop {
            guard += 1;
            if guard > 100 {
                debug!("wtf????????????????????????????????? rendered {:?} in_rendered {:?} new {:?} in_new {:?}",
                       rendered.hunks.len(),
                       in_rendered,
                       self.hunks.len(),
                       in_new
                );
                break;
            }
            if in_rendered == rendered.hunks.len() {
                debug!("rendered hunks are over!");
                break;
            }
            if in_new == self.hunks.len() {
                debug!("new hunks are over!");
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
            if rendered_delta > 0 {
                debug!("1.....");
                // rendered was erased
                if rendered.header == Hunk::shift_new_start(&new.header, rendered_delta)  // 1.1
                    ||
                    rendered.header == Hunk::shift_old_start(&new.header, 0 - rendered_delta) { // 1.2
                        // matched!
                        debug!("++++++enrich case 1.1 or 1.2");
                        new.enrich_view(rendered, buffer, context);
                        in_new += 1;
                        in_rendered += 1;
                    } else {
                        // proceed with erasing
                        debug!("------erase case 1.1 or 1.2");
                        in_rendered += 1;
                        rendered.erase(buffer, context);
                        let new_lines = rendered.new_lines as i32;
                        let old_lines = rendered.old_lines as i32;
                        rendered_delta += new_lines - old_lines;
                    }
            } else if new_delta > 0 {
                // new was inserted
                debug!("2.....");
                if new.header == Hunk::shift_new_start(&rendered.header, new_delta)
                    ||
                    new.header == Hunk::shift_old_start(&rendered.header, 0 - new_delta) {
                        debug!("++++++++ enrich cases 2.1 or 2.2 ");
                        new.enrich_view(rendered, buffer, context);
                        in_new += 1;
                        in_rendered += 1;
                    } else {
                        debug!("++++++++ skip cases 2.1 or 2.2 ");
                        in_new += 1;
                        let new_lines = new.new_lines as i32;
                        let old_lines = new.old_lines as i32;
                        new_delta += new_lines - old_lines;
                    }

            } else {
                // first loop or loop on equal hunks
                debug!("3.....");
                if rendered.header == new.header {
                    // same hunks
                    debug!("just free first match");
                    new.enrich_view(rendered, buffer, context);
                    in_new += 1;
                    in_rendered += 1;
                } else if rendered.new_start == new.new_start && rendered.old_start == new.old_start {
                    // hunks are same, but number of lines are changed

                    // this is tricky.test is required
                    let new_lines = new.new_lines as i32;
                    let rendered_lines = rendered.new_lines as i32;
                    rendered_delta += new_lines - rendered_lines;
                    debug!("changed rendered delta {}", rendered_delta);
                    // ??????
                    // if new.new_lines > rendered.new_lines {
                    //     rendered_delta += new.new_lines as i32 - rendered.new_lines as i32;
                    // } else {
                        
                    // }
                    new.enrich_view(rendered, buffer, context);
                    in_new += 1;
                    in_rendered += 1;
                } else {
                    if new.new_start < rendered.new_start && new.old_start < rendered.old_start {
                        // cases 2.1 and 2.1 - insert first new hunk
                        debug!("first new hunk without rendered. SKIP");
                        in_new += 1;
                        let new_lines = new.new_lines as i32;
                        let old_lines = new.old_lines as i32;
                        new_delta = new_lines - old_lines;
                    } else if new.new_start > rendered.new_start && new.old_start > rendered.old_start {
                        // cases 1.1 and 1.2 - delete first rendered hunk
                        debug!("first rendered hunk without new. ERASE");
                        in_rendered += 1;
                        rendered.erase(buffer, context);
                        let new_lines = rendered.new_lines as i32;
                        let old_lines = rendered.old_lines as i32;
                        rendered_delta = new_lines - old_lines;
                    } else {
                        debug!("++++++++++++++++++++++++++++++++++");
                        debug!("new header {}", new.header);
                        debug!("rendered header {}", rendered.header);
                        panic!("STOP");
                    }
                }
            }
            // ******************** concept *************************
            // let header1 = Hunk::shift_old_start(&new.header, 0 - rendered_delta);
            // let header2 = Hunk::shift_new_start(&new.header, 0 - rendered_delta);
            // let header3 = Hunk::shift_old_start(&new.header, 0 + rendered_delta);
            // let header4 = Hunk::shift_new_start(&new.header, 0 + rendered_delta);
            // if [header, header1, header2, header3, header4].contains(&rendered.header) {
            //     debug!("++++++++++enrich! {}", new.header);
            //     new.enrich_view(rendered, buffer, context);
            //     in_rendered += 1;
            //     in_new += 1;
            // } else {
            //     // how to check that it need erase rendered?
            //     if is_rendered_before {
            //         // rendered is before
            //         rendered.erase(buffer, context);
            //         let new_lines = rendered.new_lines as i32;
            //         let old_lines = rendered.old_lines as i32;
            //         rendered_delta += new_lines - old_lines;
            //         in_rendered += 1;
            //     } else {
            //         // new hunk is before
            //         // ??????????????
            //         let new_lines = new.new_lines as i32;
            //         let old_lines = new.old_lines as i32;
            //         new_delta += new_lines - old_lines;
            //         in_new += 1;
            //     }
            // }
            // ******************** concept *************************

        }

        // &&&&&&&&&&&&&&&&&&&&&& this worked!!!!!!!!!!!!!!
        // if self.hunks.len() >= rendered.hunks.len() {

        //     // [INFO  stage] Staged
        //     // RENDERED: @@ -63,6 +63,12 @@ impl Hunk {
        //     // RENDERED: @@ -102,116 +108,122 @@ impl Hunk {
        //     // NEW: @@ -63,6 +63,12 @@ impl Hunk {
        //     // NEW: @@ -74,21 +80,24 @@ impl Hunk {
        //     // NEW: @@ -102,116 +111,122 @@ impl Hunk {

        //     let mut in_render = 0;
        //     let mut delta: i32 = 0;
        //     for h in &mut self.hunks {
        //         trace!("....delta {}", delta);
        //         let rendered = &mut rendered.hunks[in_render];
        //         let header = rendered.header.clone();
        //         let header1 = Hunk::shift_old_start(&rendered.header, 0 - delta);
        //         let header2 = Hunk::shift_new_start(&rendered.header, 0 - delta);
        //         let header3 = Hunk::shift_old_start(&rendered.header, 0 + delta);
        //         let header4 = Hunk::shift_new_start(&rendered.header, 0 + delta);
        //         trace!("headers in_rendered");
        //         trace!("{}", header1);
        //         trace!("{}", header2);
        //         trace!("{}", header3);
        //         trace!("{}", header4);
        //         trace!("vssss {}", h.header);
        //         if [header, header1, header2, header3, header4].contains(&h.header) {
        //             debug!("in_rendered++++++++++enrich! {}", h.header);
        //             h.enrich_view(rendered, buffer, context);
        //             in_render += 1;
        //         } else {
        //             // this is new hunk in self!
        //             debug!("in_rendered----------erase! {}", h.header);
        //             rendered.erase(buffer, context);
        //             let new_lines = h.new_lines as i32;
        //             let old_lines = h.old_lines as i32;
        //             delta += new_lines - old_lines;
        //         }
        //     }
        // } else if self.hunks.len() < rendered.hunks.len() {

        //     // e.g. stage hunk
        //     // RENDERED: @@ -70,7 +70,8 @@ function findDepsInFile(filePath, setOfDeps, findImport, findAsync) {
        //     // RENDERED: @@ -98,8 +99,8 @@ function getLibDirPath(acc, value, index, array) {
        //     // RENDERED: @@ -128,7 +129,8 @@ function getDepsList() {
        //     // NEW: @@ -99,8 +99,8 @@ function getLibDirPath(acc, value, index, array) {
        //     // NEW: @@ -129,7 +129,8 @@ function getDepsList() {
        //     // e.g. kill hunk
        //     // RENDERED: @@ -75,7 +75,8 @@ function findDepsInDirectory(dir, setOfDeps, findImport, findAsync) {
        //     // RENDERED: @@ -106,9 +107,9 @@ function getDepsList() {
        //     // RENDERED: @@ -128,7 +129,8 @@ function getDepsList() {
        //     // NEW: @@ -106,9 +106,9 @@ function getDepsList() {
        //     // NEW: @@ -128,7 +128,8 @@ function getDepsList() {

        //     let mut in_self = 0;
        //     let mut delta: i32 = 0;
        //     for h in &mut rendered.hunks {
        //         trace!("....delta {}", delta);
        //         let new = &mut self.hunks[in_self];
        //         let header = new.header.clone();
        //         let header1 = Hunk::shift_old_start(&new.header, 0 - delta);
        //         let header2 = Hunk::shift_new_start(&new.header, 0 - delta);
        //         let header3 = Hunk::shift_old_start(&new.header, 0 + delta);
        //         let header4 = Hunk::shift_new_start(&new.header, 0 + delta);
        //         trace!("headers in_self");
        //         trace!("{}", header1);
        //         trace!("{}", header2);
        //         trace!("{}", header3);
        //         trace!("{}", header4);
        //         trace!("vssss {}", h.header);
        //         if [header, header1, header2, header3, header4].contains(&h.header) {
        //             // if [&new.header, &header1, &header2, &header3, &header4].contains(h.header[..]) {
        //             debug!("in_self++++++++++enrich! {}", h.header);
        //             new.enrich_view(h, buffer, context);
        //             in_self += 1;
        //         } else {
        //             debug!("in_self----------erase! {}", h.header);
        //             h.erase(buffer, context);
        //             let new_lines = h.new_lines as i32;
        //             let old_lines = h.old_lines as i32;
        //             delta += new_lines - old_lines;
        //         }
        //     }
        // }
        // &&&&&&&&&&&&&&&&&&&&&& this worked!!!!!!!!!!!!!!
    }

    // file
    pub fn enrich_view_old(
        &mut self,
        rendered: &mut File,
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        self.view = rendered.transfer_view();
        // aganzha!
        if !self.view.is_expanded() {
            return
        }
        trace!("-- enrich_view for file {:?} hunks {:?}, rendered {:?}, hunks {:?}, context {:?}",
               self.path, self.hunks.len(), rendered.path, rendered.hunks.len(), context);

        if self.hunks.len() == rendered.hunks.len() {
            for pair in zip(&mut self.hunks, &mut rendered.hunks) {
                pair.0.enrich_view(pair.1, buffer, context);
            }
            return;
        }
        // all hunks are ordered!!!
        // either will be hunks removed from rendered
        // or added to rendered
        let (mut n_ind, mut r_ind) = (0, 0);
        let mut guard = 0;
        let r_le = rendered.hunks.len();
        loop {
            trace!("loop........................");
            guard += 1;
            if guard >= MAX_LINES {
                panic!("guard");
            }
            let n_hunk = &self.hunks[n_ind];

            let r_hunk = &rendered.hunks[r_ind];
            let r_delta = r_hunk.delta_in_lines();

            // why kind here is required?????
            // it is required to compare old_new/new_lines and thats it
            // it could be refactored to some method, which will return
            // proper start to compare to
            if let Some(knd) = &context.diff_kind {
                if (n_hunk.new_start == r_hunk.new_start
                    && (knd == &DiffKind::Staged
                        || knd == &DiffKind::Conflicted))
                    || (n_hunk.old_start == r_hunk.old_start
                        && knd == &DiffKind::Unstaged)
                {
                    trace!(
                        "HUNKS MATCHED new: {:?} old: {:?}",
                        n_hunk.header,
                        r_hunk.header
                    );
                    let m_n_hunk = &mut self.hunks[n_ind];
                    let m_r_hunk = &mut rendered.hunks[r_ind];
                    m_n_hunk.enrich_view(m_r_hunk, buffer, context);
                    n_ind += 1;
                    r_ind += 1;
                } else if (knd == &DiffKind::Staged
                    || knd == &DiffKind::Conflicted)
                    && n_hunk.new_start < r_hunk.new_start
                {
                    trace!(
                        "^^^^^^^^new hunk is BEFORE rendered hunk in STAGED"
                    );
                    for hunk in &mut rendered.hunks[r_ind..] {
                        trace!(
                            "-> move forward hunk {:?} by {:?} lines",
                            hunk.header,
                            n_hunk.delta_in_lines()
                        );
                        hunk.new_start = ((hunk.new_start as i32)
                            + n_hunk.delta_in_lines())
                            as u32;
                    }
                    n_ind += 1;
                } else if (knd == &DiffKind::Staged
                    || knd == &DiffKind::Conflicted)
                    && n_hunk.new_start > r_hunk.new_start
                {
                    trace!(
                        "^^^^^^^^new hunk is AFTER rendered hunk in STAGED"
                    );
                    // hunk was unstaged and must be erased. means all other rendered hunks
                    // must increment their new lines cause in erased hunk its lines
                    // are no longer new
                    if r_ind < r_le {
                        let ind = r_ind + 1;
                        for hunk in &mut rendered.hunks[ind..] {
                            trace!("<- before erasing staged hunk add delta to remaining hunks {:?} by {:?} lines",
                                   hunk.header,
                                   r_delta
                            );
                            hunk.new_start = ((hunk.new_start as i32) - // - !
                                              r_delta)
                                as u32;
                        }
                    }
                    let m_r_hunk = &mut rendered.hunks[r_ind];
                    trace!("erase AFTER rendered hunk {:?}", m_r_hunk.header);
                    m_r_hunk.erase(buffer, context);
                    r_ind += 1;
                } else if knd == &DiffKind::Unstaged
                    && n_hunk.old_start < r_hunk.old_start
                {
                    trace!(
                        "^^^^^^^^new hunk is BEFORE rendered hunk in UNSTAGED"
                    );
                    for hunk in &mut rendered.hunks[r_ind..] {
                        trace!(
                            "<- move backward hunk {:?} by {:?} lines",
                            hunk.header,
                            n_hunk.delta_in_lines()
                        );
                        // the minus here in old_start
                        // means when inserting hunk before
                        // it need to REDUCE old lines in next hunks!
                        // old_lines in each hunk are independent on each other.
                        // so when unstage in previous position means LESS old_lines
                        // (when staged there are more old lines - those are considered already added!
                        hunk.old_start = ((hunk.old_start as i32) - // !
                                          n_hunk.delta_in_lines())
                            as u32;
                    }
                    n_ind += 1;
                } else if knd == &DiffKind::Unstaged
                    && n_hunk.old_start > r_hunk.old_start
                {
                    trace!("^^^^^^^^new hunk is AFTER rendered hunk in UNSTAGED (erasing hunk which was staged)");
                    // hunk was staged and must be erased. means all other rendered hunks
                    // must increment their old lines cause in erased hunk its lines are no
                    // longer old.
                    if r_ind < r_le {
                        let ind = r_ind + 1;
                        for hunk in &mut rendered.hunks[ind..] {
                            trace!("<- before erasing UNstaged hunk add delta to remaining hunks {:?} by {:?} lines",
                                   hunk.header,
                                   r_delta
                            );
                            hunk.old_start = ((hunk.old_start as i32) + // + !
                                              r_delta)
                                as u32;
                        }
                    }
                    let m_r_hunk = &mut rendered.hunks[r_ind];
                    trace!("erase AFTER rendered hunk {:?}", m_r_hunk.header);
                    m_r_hunk.erase(buffer, context);
                    r_ind += 1;
                } else {
                    panic!(
                        "whats the case here? {:?} {:?} {:?}",
                        knd, r_hunk.header, n_hunk.header
                    );
                }
            }

            // completed all new hunks
            // all remained rendered hunks must be erased
            if n_ind == self.hunks.len() {
                trace!("new hunks are over");
                // handle all new hunks
                for r_hunk in &mut rendered.hunks[r_ind..] {
                    trace!(
                        "erase remaining rendered hunk {:?}",
                        r_hunk.header
                    );
                    r_hunk.erase(buffer, context);
                }
                break;
            }
            if r_ind == rendered.hunks.len() {
                // old hunks are over.
                // there is nothing to enrich for new hunks
                trace!("rendered hunks are over");
                break;
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
