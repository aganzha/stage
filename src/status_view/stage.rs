// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use super::{CursorPosition, LastOp, Status};
use crate::dialogs::{alert, DangerDialog, YES};
use crate::git::merge;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{stage_untracked, stage_via_apply, DiffKind, Event, StageOp};

use gtk4::prelude::*;
use gtk4::{gio, glib};
use libadwaita::prelude::*;
use libadwaita::ApplicationWindow;
use log::{debug, info, trace};

impl CursorPosition {
    fn resolve_stage_op(
        &self,
        status: &Status,
        op: &StageOp,
    ) -> (Option<DiffKind>, Option<PathBuf>, Option<String>) {
        // TODO! it is not string! it must be typed HunkHeader!
        match (self, op) {
            (
                Self::CursorDiff(DiffKind::Unstaged),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(unstaged) = &status.unstaged {
                    return (Some(unstaged.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Unstaged, file_idx),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(unstaged) = &status.unstaged {
                    let file = &unstaged.files[*file_idx];
                    return (
                        Some(unstaged.kind),
                        Some(file.path.clone()),
                        None,
                    );
                }
            }
            (
                Self::CursorHunk(DiffKind::Unstaged, file_idx, hunk_idx)
                | Self::CursorLine(DiffKind::Unstaged, file_idx, hunk_idx, _),
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
            (Self::CursorDiff(DiffKind::Staged), StageOp::Unstage(_)) => {
                if let Some(staged) = &status.staged {
                    return (Some(staged.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Staged, file_idx),
                StageOp::Unstage(_),
            ) => {
                if let Some(staged) = &status.staged {
                    let file = &staged.files[*file_idx];
                    return (Some(staged.kind), Some(file.path.clone()), None);
                }
            }
            (
                Self::CursorHunk(DiffKind::Staged, file_idx, hunk_idx)
                | Self::CursorLine(DiffKind::Staged, file_idx, hunk_idx, _),
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
                Self::CursorDiff(DiffKind::Untracked),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(untracked) = &status.untracked {
                    return (Some(untracked.kind), None, None);
                }
            }
            (
                Self::CursorFile(DiffKind::Untracked, file_idx),
                StageOp::Stage(_) | StageOp::Kill(_),
            ) => {
                if let Some(untracked) = &status.untracked {
                    let file = &untracked.files[*file_idx];
                    return (
                        Some(untracked.kind),
                        Some(file.path.clone()),
                        None,
                    );
                }
            }
            (
                Self::CursorLine(DiffKind::Conflicted, file_idx, hunk_idx, _),
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
    pub fn stage(
        &mut self,
        op: StageOp,
        window: &ApplicationWindow,
        gio_settings: &gio::Settings,
    ) {
        let (diff_kind, file_path, hunk_header) =
            self.cursor_position.get().resolve_stage_op(self, &op);
        trace!(
            "stage via apply ----------------------> {:?} {:?} {:?} {:?} === {:?}",
            op, diff_kind, file_path, hunk_header, self.cursor_position
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
                                move || {
                                    stage_untracked(
                                        path.expect("no path"),
                                        file_path,
                                        sender,
                                    )
                                }
                            })
                            .await
                            .unwrap_or_else(|e| {
                                alert(format!("{:?}", e)).present(&window);
                                Ok(())
                            })
                            .unwrap_or_else(
                                |e| {
                                    alert(e).present(&window);
                                },
                            );
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
                        let mut message =
                            "This will kill all untracked files!".to_string();
                        if let Some(file_path) = &file_path {
                            let str_path =
                                file_path.to_str().expect("wrong path");
                            ignored.push(str_path.to_string());
                            message = file_path
                                .to_str()
                                .expect("wrong path")
                                .to_string();
                        } else if let Some(untracked) = &untracked {
                            for file in &untracked.files {
                                let str_path =
                                    file.path.to_str().expect("wrong path");
                                ignored.push(str_path.to_string());
                            }
                        }

                        let mut settings =
                            gio_settings.get::<HashMap<String, Vec<String>>>(
                                "ignored",
                            );
                        async move {
                            let response = alert(DangerDialog(
                                "Kill Untracked files?".to_string(),
                                message,
                            ))
                            .choose_future(&window)
                            .await;
                            if response != YES {
                                return;
                            }
                            let repo_path = path.expect("no path");
                            let repo_path =
                                repo_path.to_str().expect("wrong path");
                            if let Some(stored) = settings.get_mut(repo_path) {
                                stored.append(&mut ignored);
                                trace!("added ignore {:?}", settings);
                            } else {
                                settings
                                    .insert(repo_path.to_string(), ignored);
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
                    self.last_op.replace(LastOp {
                        op: op.clone(),
                        file_path: file_path.clone(),
                        hunk_header: hunk_header.clone(),
                    });
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
                        file_idx,
                        hunk_idx,
                        line_idx,
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
                                if hunk.conflict_markers_count > 0
                                    && line.is_side_of_conflict()
                                {
                                    info!("choose_conflict_side_of_hunk");
                                    gio::spawn_blocking({
                                        move || {
                                            merge::choose_conflict_side_of_hunk(
                                                path, file_path, hunk, line,
                                                interhunk, sender,
                                            )
                                        }
                                    }).await.unwrap_or_else(|e| {
                                        alert(format!("{:?}", e)).present(&window);
                                        Ok(())
                                    }).unwrap_or_else(|e| {
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
                                    }).await.unwrap_or_else(|e| {
                                        alert(format!("{:?}", e)).present(&window);
                                        Ok(())
                                    }).unwrap_or_else(|e| {
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
}
