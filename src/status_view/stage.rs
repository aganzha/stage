// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{CursorPosition, Status};
use crate::dialogs::{alert, DangerDialog, YES};
use crate::git::merge;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{stage_untracked, stage_via_apply, DiffKind, Event, StageOp};

use gtk4::prelude::*;
use gtk4::{gio, glib, TextBuffer, TextIter};
use libadwaita::prelude::*;
use libadwaita::ApplicationWindow;
use log::{debug, info, trace};

#[derive(Debug, Clone, Copy)]
pub struct LastOp {
    op: StageOp,
    cursor_position: CursorPosition,
    desired_diff_kind: Option<DiffKind>,
}

impl CursorPosition {
    fn resolve_stage_op(
        &self,
        status: &Status,
        op: &StageOp,
    ) -> (Option<DiffKind>, Option<PathBuf>, Option<String>) {
        // TODO! it is not string! it must be typed HunkHeader!
        // TODO! squash matches as in choose cursor position!
        match (self, op) {
            (
                Self::CursorDiff(DiffKind::Unstaged, None, None, None),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(unstaged) = &status.unstaged {
                    return (Some(unstaged.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Unstaged, Some(file_idx), None, None),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(unstaged) = &status.unstaged {
                    let file = &unstaged.files[*file_idx];
                    return (Some(unstaged.kind), Some(file.path.clone()), None);
                }
            }
            (
                Self::CursorHunk(DiffKind::Unstaged, Some(file_idx), Some(hunk_idx), None)
                | Self::CursorLine(DiffKind::Unstaged, Some(file_idx), Some(hunk_idx), _),
                StageOp::Stage(_) | StageOp::Kill(_),
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
            (Self::CursorDiff(DiffKind::Staged, None, None, None), StageOp::Unstage(_)) => {
                if let Some(staged) = &status.staged {
                    return (Some(staged.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Staged, Some(file_idx), None, None),
                StageOp::Unstage(_),
            ) => {
                if let Some(staged) = &status.staged {
                    let file = &staged.files[*file_idx];
                    return (Some(staged.kind), Some(file.path.clone()), None);
                }
            }
            (
                Self::CursorHunk(DiffKind::Staged, Some(file_idx), Some(hunk_idx), None)
                | Self::CursorLine(DiffKind::Staged, Some(file_idx), Some(hunk_idx), _),
                StageOp::Unstage(_),
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
            (
                Self::CursorDiff(DiffKind::Untracked, None, None, None),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(untracked) = &status.untracked {
                    return (Some(untracked.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Untracked, Some(file_idx), None, None),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(untracked) = &status.untracked {
                    let file = &untracked.files[*file_idx];
                    return (Some(untracked.kind), Some(file.path.clone()), None);
                }
            }
            (
                Self::CursorLine(DiffKind::Conflicted, Some(file_idx), Some(hunk_idx), _),
                StageOp::Stage(_),
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
    pub fn stage(&mut self, op: StageOp, window: &ApplicationWindow, gio_settings: &gio::Settings) {
        let (diff_kind, file_path, hunk_header) =
            self.cursor_position.get().resolve_stage_op(self, &op);
        self.last_op.replace(Some(LastOp {
            op: op,
            cursor_position: self.cursor_position.get(),
            desired_diff_kind: None,
        }));
        trace!(
            "stage via apply ----------------------> {:?} {:?} {:?} {:?} === {:?}",
            op,
            diff_kind,
            file_path,
            hunk_header,
            self.cursor_position
        );

        match diff_kind {
            Some(DiffKind::Untracked) => match op {
                StageOp::Stage(_) => {
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
                                alert(format!("{:?}", e)).present(&window);
                                Ok(())
                            })
                            .unwrap_or_else(|e| {
                                alert(e).present(&window);
                            });
                        }
                    });
                }
                StageOp::Kill(_) => {
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
                            alert(format!("{:?}", e)).present(&window);
                            Ok(())
                        })
                        .unwrap_or_else(|e| {
                            alert(e).present(&window);
                        });
                    }
                });
            }
            Some(DiffKind::Conflicted) => {
                // if op is resolved, this means StageOp AND
                // CursorLine position
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
                            let interhunk = conflicted.interhunk;
                            async move {
                                if hunk.conflict_markers_count > 0 && line.is_side_of_conflict() {
                                    info!("choose_conflict_side_of_hunk");
                                    gio::spawn_blocking({
                                        move || {
                                            merge::choose_conflict_side_of_hunk(
                                                path, file_path, hunk, line, interhunk, sender,
                                            )
                                        }
                                    })
                                    .await
                                    .unwrap_or_else(|e| {
                                        alert(format!("{:?}", e)).present(&window);
                                        Ok(())
                                    })
                                    .unwrap_or_else(|e| {
                                        alert(e).present(&window);
                                    });
                                } else {
                                    // this should be never called
                                    // conflicts are resolved in branch above
                                    info!("cleanup_last_conflict_for_file");
                                    gio::spawn_blocking({
                                        move || {
                                            merge::cleanup_last_conflict_for_file(
                                                path, file_path, interhunk, sender,
                                            )
                                        }
                                    })
                                    .await
                                    .unwrap_or_else(|e| {
                                        alert(format!("{:?}", e)).present(&window);
                                        Ok(())
                                    })
                                    .unwrap_or_else(|e| {
                                        alert(e).present(&window);
                                    });
                                }
                            }
                        });
                    }
                    _ => {
                        panic!("wrong Op resolution");
                    }
                }
            }
            _ => {
                debug!("stage op is not resolved");
            }
        }
    }

    pub fn choose_cursor_position(
        &self,
        buffer: &TextBuffer,
        render_diff_kind: Option<DiffKind>,
    ) -> TextIter {
        debug!(
            "...................choose cursor position self.last_op {:?} cursor position {:?} render_diff_kind {:?}",
            self.last_op, self.cursor_position, render_diff_kind
        );
        let this_pos = buffer.cursor_position();
        let mut iter = buffer.iter_at_offset(this_pos);
        if let (Some(last_op), Some(render_diff_kind)) = (&self.last_op.get(), render_diff_kind) {
            // both last_op and cursor_position in it are no longer actual,
            // cause update and render are already happened.
            // so, those are snapshot of previous state.
            // both will be changed right here!
            match (last_op) {
                // ----------------   Ops applied to whole Diff
                // TODO! squash in one!
                (LastOp {
                    op: StageOp::Stage(_),
                    cursor_position: CursorPosition::CursorDiff(diff_kind, None, None, None),
                    desired_diff_kind: _,
                }) => {
                    assert!(*diff_kind == DiffKind::Unstaged || *diff_kind == DiffKind::Untracked);
                    if let Some(diff) = &self.staged {
                        iter.set_line(diff.view.line_no.get());
                        self.last_op.take();
                    }
                }
                (LastOp {
                    op: StageOp::Unstage(_),
                    cursor_position: CursorPosition::CursorDiff(diff_kind, None, None, None),
                    desired_diff_kind: _,
                }) => {
                    assert!(*diff_kind == DiffKind::Staged);
                    if let Some(diff) = &self.unstaged {
                        iter.set_line(diff.view.line_no.get());
                        self.last_op.take();
                    }
                }
                (LastOp {
                    op: StageOp::Kill(_),
                    cursor_position: CursorPosition::CursorDiff(diff_kind, None, None, None),
                    desired_diff_kind: _,
                }) => {
                    assert!(*diff_kind == DiffKind::Unstaged);
                    if let Some(diff) = &self.staged {
                        iter.set_line(diff.view.line_no.get());
                        self.last_op.take();
                    } else if let Some(diff) = &self.untracked {
                        iter.set_line(diff.view.line_no.get());
                        self.last_op.take();
                    }
                }
                // ----------------   Ops applied to whole Diff

                // if Diff was updated by StageOp while on hunk and it hunks file is rendered now (was already updated)
                // and this file has another hunks - put cursor on first remaining hunk
                (LastOp {
                    op: _,
                    cursor_position:
                        CursorPosition::CursorFile(cursor_diff_kind, Some(file_idx), None, _),
                    desired_diff_kind: desired_diff_kind,
                }) if *cursor_diff_kind == render_diff_kind
                    || *desired_diff_kind == Some(render_diff_kind) =>
                {
                    for odiff in [&self.unstaged, &self.staged, &self.untracked] {
                        if let Some(diff) = odiff {
                            debug!("enter loop {:?} {:?}", diff.kind, render_diff_kind);
                            if diff.kind == render_diff_kind {
                                debug!("matched rendered diff! {:?}", render_diff_kind);
                                for i in (0..file_idx + 1).rev() {
                                    if let Some(file) = diff.files.get(i) {
                                        debug!("1. FIIIIIIIIIIIIIIIIIIILE! {:?}", file.path);
                                        iter.set_line(file.view.line_no.get());
                                        self.last_op.take();
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

                    debug!("missing file in original diff++++++++++++++++++++++");
                    // ONLY IF LAST_OP WAS NOT DROPPED BY PREVIOUS LOOP
                    if let Some(last_op) = self.last_op.get() {
                        match render_diff_kind {
                            DiffKind::Unstaged | DiffKind::Untracked => {
                                if let Some(staged) = &self.staged {
                                    iter.set_line(staged.view.line_no.get());
                                    self.last_op.take();
                                    debug!("STAGED IS HERE. PUST Cursor on itttttttttttttttttttt");
                                } else {
                                    // let op = last_op.op;
                                    debug!("wait for fuuuuuuuuuuuuuture");
                                    self.last_op.replace(Some(LastOp {
                                        op: last_op.op,
                                        cursor_position: last_op.cursor_position,
                                        desired_diff_kind: Some(DiffKind::Staged),
                                    }));
                                }
                            }
                            DiffKind::Staged => {
                                debug!(
                                    "||||||||||||| where to put cursor - unstaged or untracked?"
                                );
                            }
                            _ => {}
                        }
                    }
                }
                (LastOp {
                    op: _,
                    cursor_position:
                        CursorPosition::CursorHunk(cursor_diff_kind, Some(file_idx), Some(hunk_ids), _)
                        | CursorPosition::CursorLine(cursor_diff_kind, Some(file_idx), Some(hunk_ids), _),
                    desired_diff_kind: _,
                }) if *cursor_diff_kind == render_diff_kind => {
                    for odiff in [&self.unstaged, &self.staged] {
                        if let Some(diff) = odiff {
                            if diff.kind == render_diff_kind {
                                'found: for i in (0..file_idx + 1).rev() {
                                    if let Some(file) = diff.files.get(i) {
                                        if file.view.is_expanded() {
                                            for j in (0..hunk_ids + 1).rev() {
                                                if let Some(hunk) = file.hunks.get(j) {
                                                    debug!("HUUUUUUUUUUUUUUUUUNK! {:?} line {:?} rendered {:?}",
                                                           hunk.header,
                                                           hunk.view.line_no.get(),
                                                           hunk.view.is_rendered()
                                                    );
                                                    iter.set_line(hunk.view.line_no.get());
                                                    self.last_op.take();
                                                    break 'found;
                                                }
                                            }
                                        }
                                        debug!("2. FIIIIIIIIIIIIIIIIIIILE! {:?}", file.path);
                                        iter.set_line(file.view.line_no.get());
                                        self.last_op.take();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                (op) => {
                    debug!("----------> NOT COVERED LastOp {:?}", op)
                }
            }
        } else {
            debug!("no any last_op....................");
        }
        iter
    }
}
