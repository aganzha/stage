use crate::status_view::ViewContainer;
use crate::{Diff, DiffKind, File, Head, Hunk, Line, Related, State, View};
use git2::RepositoryState;
use gtk4::TextView;
use log::{debug, trace};
use std::collections::HashSet;
use std::iter::zip;

impl Line {
    // line
    pub fn enrich_view(
        &mut self,
        rendered: &Line,
        _context: &mut Option<crate::StatusRenderContext>,
    ) {
        self.view = rendered.transfer_view();
        if self.content != rendered.content || self.origin != rendered.origin {
            self.view.dirty = true;
            debug!("*************dirty content in reconciliation: {} <> {} origins: {:?} {:?}", self.content, rendered.content, self.origin, rendered.origin)
        }
    }
    // line
    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone
    }
}

impl Hunk {
    // Hunk
    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        // hunk headers are changing always
        // during partial staging
        clone.dirty = true;
        clone.transfered = true;
        clone
    }
    // hunk
    pub fn enrich_view(
        &mut self,
        rendered: &mut Hunk,
        txt: &TextView,
        context: &mut Option<crate::StatusRenderContext>,
    ) {
        self.view = rendered.transfer_view();
        if self.lines.len() == rendered.lines.len() {
            for pair in zip(&mut self.lines, &rendered.lines) {
                pair.0.enrich_view(pair.1, context);
            }
            return;
        }
        // all lines are ordered
        let (mut r_ind, mut n_ind) = (0, 0);
        let mut guard = 0;
        debug!("++++++++++++++++ line roconciliation");
        loop {
            debug!("++++++loop");
            guard += 1;
            if guard > 20 {
                debug!("guard");
                break;
            }
            let r_line = &rendered.lines[r_ind];
            let n_line = &self.lines[n_ind];
            // hunks could be shifted
            // and line_nos could differ a lot
            // it need to find first matched line
            // THIS IS ACTUAL ONLY FOR UNSTAGED
            match (r_line.old_line_no, n_line.old_line_no) {
                (Some(r_no), Some(n_no)) => {
                    debug!(
                        "both lines are changed {:?} {:?}",
                        r_line.hash(),
                        n_line.hash()
                    );
                    debug!("r_no n_no {:?} {:?}", r_no, n_no);
                    let m_n_line = &mut self.lines[n_ind];
                    m_n_line.enrich_view(r_line, context);
                    r_ind += 1;
                    n_ind += 1;
                }
                (Some(r_no), None) => {
                    debug!(
                        "new line is added before old one {:} {:?}",
                        r_line.hash(),
                        n_line.hash()
                    );
                    debug!("r_no n_no {:?} _", r_no);
                    n_ind += 1;
                    continue;
                }
                (None, Some(n_no)) => {
                    debug!(
                        "rendered line is added before new one {:} {:?}",
                        r_line.hash(),
                        n_line.hash()
                    );
                    debug!("r_no n_no _ {:?}", n_no);
                    let m_r_line = &mut rendered.lines[r_ind];
                    m_r_line.erase(txt, context);
                    r_ind += 1;
                }
                (None, None) => {
                    debug!(
                        "both lines are added {:} {:?}",
                        r_line.hash(),
                        n_line.hash()
                    );
                    debug!("r_no n_no _ _");
                    let m_n_line = &mut self.lines[n_ind];
                    m_n_line.enrich_view(r_line, context);
                    r_ind += 1;
                    n_ind += 1;
                }
            }
            debug!("");
            if r_ind == rendered.lines.len() {
                debug!("rendered lines are over");
                break;
            }
            if n_ind == self.lines.len() {
                debug!("new lines are over");
                break;
            }
        }
    }
}

impl File {
    // file
    pub fn enrich_view(
        &mut self,
        rendered: &mut File,
        txt: &TextView,
        context: &mut Option<crate::StatusRenderContext>,
    ) {
        self.view = rendered.transfer_view();

        debug!("-- enrich_view for file {:?} hunks {:?}, rendered {:?}, hunks {:?}, context {:?}",
               self.path, self.hunks.len(), rendered.path, rendered.hunks.len(), context);

        if self.hunks.len() == rendered.hunks.len() {
            for pair in zip(&mut self.hunks, &mut rendered.hunks) {
                pair.0.enrich_view(pair.1, txt, context);
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
            debug!("loop........................");
            guard += 1;
            if guard >= 100000 {
                break;
            }
            let n_hunk = &self.hunks[n_ind];

            let r_hunk = &rendered.hunks[r_ind];
            let r_delta = r_hunk.delta_in_lines();

            if let Some(ctx) = context {
                if let Some(knd) = &ctx.diff_kind {
                    if (n_hunk.new_start == r_hunk.new_start
                        && knd == &DiffKind::Staged)
                        || (n_hunk.old_start == r_hunk.old_start
                            && knd == &DiffKind::Unstaged)
                    {
                        debug!(
                            "HUNKS MATCHED new: {:?} old: {:?}",
                            n_hunk.header, r_hunk.header
                        );
                        let m_n_hunk = &mut self.hunks[n_ind];
                        let m_r_hunk = &mut rendered.hunks[r_ind];
                        m_n_hunk.enrich_view(m_r_hunk, txt, context);
                        n_ind += 1;
                        r_ind += 1;
                    } else if knd == &DiffKind::Staged
                        && n_hunk.new_start < r_hunk.new_start
                    {
                        debug!("^^^^^^^^new hunk is BEFORE rendered hunk in STAGED");
                        for hunk in &mut rendered.hunks[r_ind..] {
                            debug!(
                                "-> move forward hunk {:?} by {:?} lines",
                                hunk.header,
                                n_hunk.delta_in_lines()
                            );
                            hunk.new_start = ((hunk.new_start as i32)
                                + n_hunk.delta_in_lines())
                                as u32;
                        }
                        n_ind += 1;
                    } else if knd == &DiffKind::Staged
                        && n_hunk.new_start > r_hunk.new_start
                    {
                        debug!("^^^^^^^^new hunk is AFTER rendered hunk in STAGED");
                        // hunk was unstaged and must be erased. means all other rendered hunks
                        // must increment their new lines cause in erased hunk its lines
                        // are no longer new
                        if r_ind < r_le {
                            let ind = r_ind + 1;
                            for hunk in &mut rendered.hunks[ind..] {
                                debug!("<- before erasing staged hunk add delta to remaining hunks {:?} by {:?} lines",
                                       hunk.header,
                                       r_delta
                                );
                                hunk.new_start = ((hunk.new_start as i32) - // - !
                                                  r_delta)
                                    as u32;
                            }
                        }
                        let m_r_hunk = &mut rendered.hunks[r_ind];
                        debug!(
                            "erase AFTER rendered hunk {:?}",
                            m_r_hunk.header
                        );
                        m_r_hunk.erase(txt, context);
                        r_ind += 1;
                    } else if knd == &DiffKind::Unstaged
                        && n_hunk.old_start < r_hunk.old_start
                    {
                        debug!("^^^^^^^^new hunk is BEFORE rendered hunk in UNSTAGED");
                        for hunk in &mut rendered.hunks[r_ind..] {
                            debug!(
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
                        debug!("^^^^^^^^new hunk is AFTER rendered hunk in UNSTAGED (erasing hunk which was staged)");
                        // hunk was staged and must be erased. means all other rendered hunks
                        // must increment their old lines cause in erased hunk its lines are no
                        // longer old.
                        if r_ind < r_le {
                            let ind = r_ind + 1;
                            for hunk in &mut rendered.hunks[ind..] {
                                debug!("<- before erasing UNstaged hunk add delta to remaining hunks {:?} by {:?} lines",
                                       hunk.header,
                                       r_delta
                                );
                                hunk.old_start = ((hunk.old_start as i32) + // + !
                                                  r_delta)
                                    as u32;
                            }
                        }
                        let m_r_hunk = &mut rendered.hunks[r_ind];
                        debug!(
                            "erase AFTER rendered hunk {:?}",
                            m_r_hunk.header
                        );
                        m_r_hunk.erase(txt, context);
                        r_ind += 1;
                    } else {
                        panic!(
                            "whats the case here? {:?} {:?} {:?}",
                            knd, r_hunk.header, n_hunk.header
                        );
                    }
                }
            }

            // // relation of rendered hunk to new one
            // let mut kind: Option<&DiffKind> = None;
            // if let Some(ctx) = context {
            //     if let Some(knd) = &ctx.diff_kind {
            //         kind.replace(knd);
            //     }
            // }

            // let relation = n_hunk.related_to(r_hunk, kind);
            // match relation {
            //     Related::Matched => {
            //         debug!(
            //             "HUNKS MATCHED new: {:?} old: {:?}",
            //             n_hunk.header,
            //             r_hunk.header
            //         );
            //         let m_n_hunk = &mut self.hunks[n_ind];
            //         let m_r_hunk = &mut rendered.hunks[r_ind];
            //         m_n_hunk.enrich_view(m_r_hunk, txt, context);
            //         n_ind += 1;
            //         r_ind += 1;
            //     }
            //     Related::Before => {
            //         // if new hunk is before old one
            //         // it need to shift all old hunks
            //         // by lines_co of new one
            //         debug!(
            //             "new hunk is before rendered one. new: {:?} old: {:?}",
            //             n_hunk.header,
            //             r_hunk.header
            //         );
            //         match kind {
            //             Some(DiffKind::Staged) => {
            //                 debug!("^^^^^^^^new hunk is before rendered hunk in STAGED");
            //                 // this is doubtfull...
            //                 for hunk in &mut rendered.hunks[r_ind..] {
            //                     debug!(
            //                         "-> move forward hunk {:?} by {:?} lines",
            //                         hunk.header,
            //                         n_hunk.delta_in_lines()
            //                     );
            //                     hunk.new_start = ((hunk.new_start as i32)
            //                         + n_hunk.delta_in_lines())
            //                         as u32;
            //                 }
            //             }
            //             Some(DiffKind::Unstaged) => {
            //                 //staged back to unstaged
            //                 debug!("^^^^^^^^new hunk is before rendered hunk in UNSTAGED");
            //                 for hunk in &mut rendered.hunks[r_ind..] {
            //                     debug!(
            //                         "<- move backward hunk {:?} by {:?} lines",
            //                         hunk.header,
            //                         n_hunk.delta_in_lines()
            //                     );
            //                     // the minus here in old_start
            //                     // means when inserting hunk before
            //                     // it need to REDUCE old lines in next hunks!
            //                     // old_lines in each hunk are independent on each other.
            //                     // so when unstage in previous position means LESS old_lines
            //                     // (when staged there are more old lines - those are considered already added!
            //                     hunk.old_start = ((hunk.old_start as i32) - // !
            //                             n_hunk.delta_in_lines())
            //                         as u32;
            //                 }
            //             }
            //             _ => panic!("no kind in file enrich_view1"),
            //         }
            //         debug!(
            //             "proceed to next new hunk, but do not touch old_ones"
            //         );
            //         n_ind += 1;
            //     }
            //     Related::After => {
            //         debug!("new hunk is AFTER rendered one.  new: {:?} rendered: {:?}", n_hunk.header, r_hunk.header);
            //         // if new hunk is after rendered one, then rendered must be erased!
            //         match kind {
            //             Some(DiffKind::Staged) => {
            //                 debug!("^^^^^^^^new hunk is AFTER rendered hunk in STAGED");
            //                 // hunk was unstaged and must be erased. means all other rendered hunks
            //                 // must increment their new lines cause in erased hunk its lines
            //                 // are no longer new
            //                 if r_ind < r_le {
            //                     let ind = r_ind + 1;
            //                     for hunk in &mut rendered.hunks[ind..] {
            //                         debug!("<- before erasing staged hunk add delta to remaining hunks {:?} by {:?} lines",
            //                                hunk.header,
            //                                r_delta
            //                         );
            //                         hunk.new_start = ((hunk.new_start as i32) - // - !
            //                                 r_delta)
            //                             as u32;
            //                     }
            //                 }
            //             }
            //             Some(DiffKind::Unstaged) => {
            //                 debug!("^^^^^^^^new hunk is AFTER rendered hunk in UNSTAGED (erasing hunk which was staged)");
            //                 // hunk was staged and must be erased. means all other rendered hunks
            //                 // must increment their old lines cause in erased hunk its lines are no
            //                 // longer old.
            //                 if r_ind < r_le {
            //                     let ind = r_ind + 1;
            //                     for hunk in &mut rendered.hunks[ind..] {
            //                         debug!("<- before erasing UNstaged hunk add delta to remaining hunks {:?} by {:?} lines",
            //                                hunk.header,
            //                                r_delta
            //                         );
            //                         hunk.old_start = ((hunk.old_start as i32) + // + !
            //                                 r_delta)
            //                             as u32;
            //                     }
            //                 }
            //             }
            //             _ => panic!("no kind in file enrich_view2"),
            //         }
            //         let m_r_hunk = &mut rendered.hunks[r_ind];
            //         debug!("erase AFTER rendered hunk {:?}", m_r_hunk.header);
            //         m_r_hunk.erase(txt, context);
            //         r_ind += 1;
            //     }
            //     _ => {
            //         // i think i dont need overlaps at all!
            //         // it either before or after or matched!
            //         // but how can i compare them at all....
            //         // new files, i mean. those are completelly different...
            //         // or not????
            //         panic!(
            //             "no way! {:?} {:?} {:?}",
            //             relation, n_hunk.header, r_hunk.header
            //         );
            //     }
            // }

            // completed all new hunks
            // all remained rendered hunks must be erased
            if n_ind == self.hunks.len() {
                debug!("new hunks are over");
                // handle all new hunks
                for r_hunk in &mut rendered.hunks[r_ind..] {
                    debug!(
                        "erase remaining rendered hunk {:?}",
                        r_hunk.header
                    );
                    r_hunk.erase(txt, context);
                }
                break;
            }
            if r_ind == rendered.hunks.len() {
                // old hunks are over.
                // there is nothing to enrich for new hunks
                debug!("rendered hunls are over");
                break;
            }
        }
    }

    // // File
    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone
    }
}

impl Diff {
    pub fn enrich_view(
        &mut self,
        rendered: &mut Diff,
        txt: &TextView,
        context: &mut Option<crate::StatusRenderContext>,
    ) {
        if let Some(ctx) = context {
            ctx.diff_kind.replace(self.kind.clone());
        }
        debug!("---------------enrich {:?} view in diff. my files {:?}, rendered files {:?}",
               &self.kind,
               self.files.len(),
               rendered.files.len(),
        );
        let mut replaces_by_new = HashSet::new();
        for file in &mut self.files {
            for of in &mut rendered.files {
                if file.path == of.path {
                    file.enrich_view(of, txt, context);
                    replaces_by_new.insert(file.path.clone());
                }
            }
        }
        // erase all stale views
        debug!("before erasing files. replaced by new {:?} for total files count: {:?}", replaces_by_new, rendered.files.len());
        rendered
            .files
            .iter_mut()
            .filter(|f| !replaces_by_new.contains(&f.path))
            .for_each(|f| {
                debug!(
                    "context on final lines of diff render view {:?}",
                    context
                );
                f.erase(txt, context)
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
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone.dirty = true;
        clone
    }
}

impl State {
    // state
    pub fn enrich_view(&mut self, rendered: &Self) {
        self.view = rendered.transfer_view();
        if self.state == RepositoryState::Clean {
            self.view.hidden = true;
        } else {
            self.view.hidden = false;
        }
    }
    // state
    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone.dirty = true;
        clone
    }
}
