// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{CursorPosition, Status};
use crate::dialogs::{alert, DangerDialog, YES};
use crate::git::merge;

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::{stage_untracked, stage_via_apply, Diff, DiffKind, Event, StageOp};

use gtk4::prelude::*;
use gtk4::{gio, glib, TextBuffer, TextIter};
use libadwaita::prelude::*;
use libadwaita::ApplicationWindow;
use log::{debug, info, trace};

#[derive(Debug, Clone, Copy)]
pub struct LastOp {
    pub op: StageOp,
    pub cursor_position: CursorPosition,
    pub desired_diff_kind: Option<DiffKind>,
}

#[derive(Debug, Clone)]
pub struct StageDiffs<'a> {
    pub untracked: &'a Option<Diff>,
    pub unstaged: &'a Option<Diff>,
    pub staged: &'a Option<Diff>,
}

impl CursorPosition {
    fn resolve_stage_op(
        &self,
        status: &Status,
        op: &StageOp,
    ) -> (Option<DiffKind>, Option<PathBuf>, Option<String>) {
        match (self, op) {
            (Self::CursorDiff(DiffKind::Unstaged), StageOp::Stage | StageOp::Kill) => {
                if let Some(unstaged) = &status.unstaged {
                    return (Some(unstaged.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Unstaged, Some(file_idx)),
                StageOp::Stage | StageOp::Kill,
            ) => {
                if let Some(unstaged) = &status.unstaged {
                    let file = &unstaged.files[*file_idx];
                    return (Some(unstaged.kind), Some(file.path.clone()), None);
                }
            }
            (
                Self::CursorHunk(DiffKind::Unstaged, Some(file_idx), Some(hunk_idx))
                | Self::CursorLine(DiffKind::Unstaged, Some(file_idx), Some(hunk_idx), _),
                StageOp::Stage | StageOp::Kill,
            ) => {
                if let Some(unstaged) = &status.unstaged {
                    let file = &unstaged.files[*file_idx];
                    let hunk = &file.hunks[*hunk_idx];
                    return (
                        Some(unstaged.kind),
                        Some(file.path.clone()),
                        Some(hunk.header.clone()),
                    );
                }
            }
            (Self::CursorDiff(DiffKind::Staged), StageOp::Unstage) => {
                if let Some(staged) = &status.staged {
                    return (Some(staged.kind), None, None);
                }
            }
            (Self::CursorFile(DiffKind::Staged, Some(file_idx)), StageOp::Unstage) => {
                if let Some(staged) = &status.staged {
                    let file = &staged.files[*file_idx];
                    return (Some(staged.kind), Some(file.path.clone()), None);
                }
            }
            (
                Self::CursorHunk(DiffKind::Staged, Some(file_idx), Some(hunk_idx))
                | Self::CursorLine(DiffKind::Staged, Some(file_idx), Some(hunk_idx), _),
                StageOp::Unstage,
            ) => {
                if let Some(staged) = &status.staged {
                    let file = &staged.files[*file_idx];
                    let hunk = &file.hunks[*hunk_idx];
                    return (
                        Some(staged.kind),
                        Some(file.path.clone()),
                        Some(hunk.header.clone()),
                    );
                }
            }
            (Self::CursorDiff(DiffKind::Untracked), StageOp::Stage | StageOp::Kill) => {
                if let Some(untracked) = &status.untracked {
                    return (Some(untracked.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Untracked, Some(file_idx)),
                StageOp::Stage | StageOp::Kill,
            ) => {
                if let Some(untracked) = &status.untracked {
                    let file = &untracked.files[*file_idx];
                    return (Some(untracked.kind), Some(file.path.clone()), None);
                }
            }
            (
                Self::CursorLine(DiffKind::Conflicted, Some(file_idx), Some(hunk_idx), _),
                StageOp::Stage,
            ) => {
                if let Some(conflicted) = &status.conflicted {
                    let file = &conflicted.files[*file_idx];
                    let hunk = &file.hunks[*hunk_idx];
                    return (
                        Some(conflicted.kind),
                        Some(file.path.clone()),
                        Some(hunk.header.clone()),
                    );
                }
            }
            (_, _) => {}
        }
        (None, None, None)
    }
}

impl Status {
    pub fn stage_op(
        &mut self,
        op: StageOp,
        window: &ApplicationWindow,
        gio_settings: &gio::Settings,
    ) {
        let (diff_kind, file_path, hunk_header) =
            self.cursor_position.get().resolve_stage_op(self, &op);

        let current_op = Some(LastOp {
            op,
            cursor_position: self.cursor_position.get(),
            desired_diff_kind: None,
        });

        match diff_kind {
            Some(DiffKind::Untracked) => match op {
                StageOp::Stage => {
                    self.last_op.replace(current_op);
                    glib::spawn_future_local({
                        let path = self.path.clone();
                        let sender = self.sender.clone();
                        let file_path = file_path.clone();
                        let window = window.clone();
                        async move {
                            gio::spawn_blocking({
                                move || stage_untracked(path.expect("no path"), file_path, sender)
                            })
                            .await
                            .unwrap_or_else(|e| {
                                alert(format!("{:?}", e)).present(Some(&window));
                                Ok(())
                            })
                            .unwrap_or_else(|e| {
                                alert(e).present(Some(&window));
                            });
                        }
                    });
                }
                StageOp::Kill => {
                    self.last_op.replace(current_op);
                    glib::spawn_future_local({
                        let window = window.clone();
                        let path = self.path.clone();
                        let gio_settings = gio_settings.clone();
                        let sender = self.sender.clone();
                        let untracked = self.untracked.clone();
                        let mut ignored = Vec::new();
                        let mut message = "This will hide all untracked files!".to_string();
                        if let Some(file_path) = &file_path {
                            let str_path = file_path.to_str().expect("wrong path");
                            ignored.push(str_path.to_string());
                            message = file_path.to_str().expect("wrong path").to_string();
                        } else if let Some(untracked) = &untracked {
                            for file in &untracked.files {
                                let str_path = file.path.to_str().expect("wrong path");
                                ignored.push(str_path.to_string());
                            }
                        }

                        let mut settings =
                            gio_settings.get::<HashMap<String, Vec<String>>>("ignored");
                        async move {
                            let response =
                                alert(DangerDialog("Hide Untracked files?".to_string(), message))
                                    .choose_future(&window)
                                    .await;
                            if response != YES {
                                return;
                            }
                            let repo_path = path.expect("no path");
                            let repo_path = repo_path.to_str().expect("wrong path");
                            if let Some(stored) = settings.get_mut(repo_path) {
                                stored.append(&mut ignored);
                                trace!("added ignore {:?}", settings);
                            } else {
                                settings.insert(repo_path.to_string(), ignored);
                                trace!("first ignored file {:?}", settings);
                            }
                            gio_settings
                                .set("ignored", settings)
                                .expect("cant set settings");
                            sender
                                .send_blocking(Event::Untracked(untracked))
                                .expect("Could not send through channel");
                        }
                    });
                }
                _ => {
                    debug!("unknow op for untracked");
                }
            },
            Some(DiffKind::Staged) | Some(DiffKind::Unstaged) => {
                self.last_op.replace(current_op);
                glib::spawn_future_local({
                    let window = window.clone();
                    let path = self.path.clone();
                    let sender = self.sender.clone();
                    async move {
                        gio::spawn_blocking({
                            move || {
                                stage_via_apply(
                                    path.expect("no path"),
                                    file_path,
                                    hunk_header,
                                    op,
                                    sender,
                                )
                            }
                        })
                        .await
                        .unwrap_or_else(|e| {
                            alert(format!("{:?}", e)).present(Some(&window));
                            Ok(())
                        })
                        .unwrap_or_else(|e| {
                            alert(e).present(Some(&window));
                        });
                    }
                });
            }
            Some(DiffKind::Conflicted) => {
                // if op is resolved, this means StageOp AND
                // CursorLine position
                self.last_op.replace(current_op);
                match self.cursor_position.get() {
                    CursorPosition::CursorLine(
                        DiffKind::Conflicted,
                        Some(file_idx),
                        Some(hunk_idx),
                        Some(line_idx),
                    ) => {
                        let conflicted = self.conflicted.as_ref().unwrap();
                        let file = &conflicted.files[file_idx];
                        let hunk = &file.hunks[hunk_idx];
                        let line = &hunk.lines[line_idx];
                        glib::spawn_future_local({
                            let path = self.path.clone().unwrap();
                            let sender = self.sender.clone();
                            let file_path = file.path.clone();
                            let hunk = hunk.clone();
                            let line = line.clone();
                            let window = window.clone();
                            async move {
                                if hunk.conflict_markers_count > 0 && line.is_side_of_conflict() {
                                    info!("choose_conflict_side_of_hunk");
                                    gio::spawn_blocking({
                                        move || {
                                            merge::choose_conflict_side_of_hunk(
                                                path, file_path, hunk, line, sender,
                                            )
                                        }
                                    })
                                    .await
                                    .unwrap_or_else(|e| {
                                        alert(format!("{:?}", e)).present(Some(&window));
                                        Ok(())
                                    })
                                    .unwrap_or_else(|e| {
                                        alert(e).present(Some(&window));
                                    });
                                } else {
                                    // this should be never called
                                    // conflicts are resolved in branch above
                                    info!("cleanup_last_conflict_for_file");
                                    gio::spawn_blocking({
                                        move || merge::try_finalize_conflict(path, sender)
                                    })
                                    .await
                                    .unwrap_or_else(|e| {
                                        alert(format!("{:?}", e)).present(Some(&window));
                                        Ok(())
                                    })
                                    .unwrap_or_else(|e| {
                                        alert(e).present(Some(&window));
                                    });
                                }
                            }
                        });
                    }
                    _ => {
                        debug!("wrong Op resolution");
                    }
                }
            }
            _ => {
                debug!("stage op is not resolved");
            }
        }
    }
}

impl StageDiffs<'_> {
    pub fn choose_cursor_position(
        &self,
        buffer: &TextBuffer,
        render_diff_kind: Option<DiffKind>,
        last_op: &Cell<Option<LastOp>>,
    ) -> TextIter {
        let this_pos = buffer.cursor_position();
        let mut iter = buffer.iter_at_offset(this_pos);
        if let (Some(op), Some(render_diff_kind)) = (&last_op.get(), render_diff_kind) {
            // both last_op and cursor_position in it are no longer actual,
            // cause update and render are already happened.
            // so, those are snapshot of previous state.
            // both will be changed right here!
            match op {
                // ----------------   Ops applied to whole Diff
                // TODO! squash in one!
                LastOp {
                    op: StageOp::Stage,
                    cursor_position: CursorPosition::CursorDiff(diff_kind),
                    desired_diff_kind: _,
                } => {
                    if !(*diff_kind == DiffKind::Unstaged || *diff_kind == DiffKind::Untracked) {
                        debug!("wrong diff_kind 1 {:?}", diff_kind);
                    }
                    if let Some(diff) = &self.staged {
                        iter.set_line(diff.view.line_no.get());
                        last_op.take();
                    }
                }
                LastOp {
                    op: StageOp::Unstage,
                    cursor_position: CursorPosition::CursorDiff(diff_kind),
                    desired_diff_kind: _,
                } => {
                    if !(*diff_kind == DiffKind::Staged) {
                        debug!("wrong diff_kind 2 {:?}", diff_kind);
                    }
                    if let Some(diff) = &self.unstaged {
                        iter.set_line(diff.view.line_no.get());
                        last_op.take();
                    }
                }
                LastOp {
                    op: StageOp::Kill,
                    cursor_position: CursorPosition::CursorDiff(diff_kind),
                    desired_diff_kind: _,
                } => {
                    if !(*diff_kind == DiffKind::Unstaged) {
                        debug!("wrong diff_kind 3 {:?}", diff_kind);
                    }
                    if let Some(diff) = &self.staged {
                        iter.set_line(diff.view.line_no.get());
                        last_op.take();
                    } else if let Some(diff) = &self.untracked {
                        iter.set_line(diff.view.line_no.get());
                        last_op.take();
                    }
                }
                // ----------------   Ops applied to whole Diff

                // if Diff was updated by StageOp while on hunk and it hunks file is rendered now (was already updated)
                // and this file has another hunks - put cursor on first remaining hunk
                LastOp {
                    op: _,
                    cursor_position: CursorPosition::CursorFile(cursor_diff_kind, Some(file_idx)),
                    desired_diff_kind,
                } if *cursor_diff_kind == render_diff_kind
                    || *desired_diff_kind == Some(render_diff_kind) =>
                {
                    for odiff in [&self.unstaged, &self.staged, &self.untracked] {
                        if let Some(diff) = odiff {
                            if diff.kind == render_diff_kind {
                                for i in (0..file_idx + 1).rev() {
                                    if let Some(file) = diff.files.get(i) {
                                        iter.set_line(file.view.line_no.get());
                                        last_op.take();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    // if last_op was not handled.
                    // this means there is nothing to put
                    // into changed diff. It need to put cursor
                    // to opposite diff
                    // BUT! if opposite diff is not here, the next render cycle this
                    // clause will not match! because its condition is to compare render_cursor_diff with
                    // last_op cursor position. BUT IT NEED TO MATCH IT WITH DESIRED DIFF ALSO!
                    self.cursor_on_opposite_diff(render_diff_kind, &mut iter, last_op);
                }
                LastOp {
                    op: _,
                    cursor_position:
                        CursorPosition::CursorHunk(cursor_diff_kind, Some(file_idx), Some(hunk_ids))
                        | CursorPosition::CursorLine(cursor_diff_kind, Some(file_idx), Some(hunk_ids), _),
                    desired_diff_kind,
                } if *cursor_diff_kind == render_diff_kind
                    || *desired_diff_kind == Some(render_diff_kind) =>
                {
                    for odiff in [&self.unstaged, &self.staged] {
                        if let Some(diff) = odiff {
                            if diff.kind == render_diff_kind {
                                'found: for i in (0..file_idx + 1).rev() {
                                    if let Some(file) = diff.files.get(i) {
                                        if file.view.is_expanded() {
                                            for j in (0..hunk_ids + 1).rev() {
                                                if let Some(hunk) = file.hunks.get(j) {
                                                    iter.set_line(hunk.view.line_no.get());
                                                    last_op.take();
                                                    break 'found;
                                                }
                                            }
                                        }
                                        iter.set_line(file.view.line_no.get());
                                        last_op.take();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    self.cursor_on_opposite_diff(render_diff_kind, &mut iter, last_op);
                }
                op => {
                    trace!("----------> NOT COVERED LastOp {:?}", op)
                }
            }
        } else {
            trace!("no any last_op....................");
        }
        iter
    }

    fn cursor_on_opposite_diff(
        &self,
        render_diff_kind: DiffKind,
        iter: &mut TextIter,
        last_op: &Cell<Option<LastOp>>,
    ) {
        // ONLY IF LAST_OP WAS NOT DROPPED BY PREVIOUS LOOP
        if let Some(op) = last_op.get() {
            match render_diff_kind {
                DiffKind::Unstaged | DiffKind::Untracked => {
                    if let Some(diff) = &self.staged {
                        iter.set_line(diff.files[0].view.line_no.get());
                        last_op.take();
                    } else {
                        last_op.replace(Some(LastOp {
                            op: op.op,
                            cursor_position: op.cursor_position,
                            desired_diff_kind: Some(DiffKind::Staged),
                        }));
                    }
                }
                DiffKind::Staged => {
                    if let Some(diff) = &self.unstaged {
                        let line_no = diff.files[0].view.line_no.get();
                        iter.set_line(line_no);
                        last_op.take();
                    } else if let Some(diff) = &self.untracked {
                        iter.set_line(diff.files[0].view.line_no.get());
                        last_op.take();
                    } else {
                        last_op.replace(Some(LastOp {
                            op: op.op,
                            cursor_position: op.cursor_position,
                            desired_diff_kind: Some(DiffKind::Unstaged),
                        }));
                    }
                }
                _ => {
                    debug!("does not matched in opposite_diff")
                }
            }
        }
    }
}
