use crate::status_view::render::View;
use crate::status_view::ViewContainer;
use crate::{
    Diff, DiffKind, File, Head, Hunk, Line, State, Untracked, UntrackedFile,
};
use git2::RepositoryState;
use gtk4::TextBuffer;
use log::trace;
use std::collections::HashSet;
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
            self.view.dirty = true;
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
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
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
        trace!("++++++++++++++++ line roconciliation");
        loop {
            trace!("++++++loop");
            guard += 1;
            if guard > MAX_LINES {
                panic!("guard");
            }
            let r_line = &rendered.lines[r_ind];
            let n_line = &self.lines[n_ind];
            // hunks could be shifted
            // and line_nos could differ a lot
            // it need to find first matched line
            // THIS IS ACTUAL ONLY FOR UNSTAGED
            match (r_line.old_line_no, n_line.old_line_no) {
                (Some(r_no), Some(n_no)) => {
                    trace!(
                        "both lines are changed"
                    );
                    trace!("r_no n_no {:?} {:?}", r_no, n_no);
                    let m_n_line = &mut self.lines[n_ind];
                    m_n_line.enrich_view(r_line, context);
                    r_ind += 1;
                    n_ind += 1;
                }
                (Some(r_no), None) => {
                    trace!(
                        "new line is added before old one"
                    );
                    trace!("r_no n_no {:?} _", r_no);
                    n_ind += 1;
                }
                (None, Some(n_no)) => {
                    trace!(
                        "rendered line is added before new one"
                    );
                    trace!("r_no n_no _ {:?}", n_no);
                    let m_r_line = &mut rendered.lines[r_ind];
                    m_r_line.erase(buffer, context);
                    r_ind += 1;
                }
                (None, None) => {
                    trace!(
                        "both lines are added",
                    );
                    trace!("r_no n_no _ _");
                    let m_n_line = &mut self.lines[n_ind];
                    m_n_line.enrich_view(r_line, context);
                    r_ind += 1;
                    n_ind += 1;
                }
            }
            trace!("");
            if r_ind == rendered.lines.len() {
                trace!("rendered lines are over");
                break;
            }
            if n_ind == self.lines.len() {
                trace!("new lines are over");
                for r_line in &mut rendered.lines[r_ind..] {
                    trace!("erase remainign rendered lines!");
                    r_line.erase(buffer, context);
                }
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
        buffer: &TextBuffer,
        context: &mut crate::StatusRenderContext,
    ) {
        self.view = rendered.transfer_view();

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
                    trace!("^^^^^^^^new hunk is BEFORE rendered hunk in STAGED");
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
                    trace!("^^^^^^^^new hunk is AFTER rendered hunk in STAGED");
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
                    trace!(
                        "erase AFTER rendered hunk {:?}",
                        m_r_hunk.header
                    );
                    m_r_hunk.erase(buffer, context);
                    r_ind += 1;
                } else if knd == &DiffKind::Unstaged
                    && n_hunk.old_start < r_hunk.old_start
                {
                    trace!("^^^^^^^^new hunk is BEFORE rendered hunk in UNSTAGED");
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
                    trace!(
                        "erase AFTER rendered hunk {:?}",
                        m_r_hunk.header
                    );
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
        let mut clone = self.view.clone();
        clone.transfered = true;
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
        let mut clone = self.view.clone();
        clone.transfered = true;
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
        self.view.squashed = self.state == RepositoryState::Clean;
    }
    // state
    pub fn transfer_view(&self) -> View {
        let mut clone = self.view.clone();
        clone.transfered = true;
        clone.dirty = true;
        clone
    }
}
