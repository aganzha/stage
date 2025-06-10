// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::git::{get_head, make_diff, make_diff_options, DeferRefresh, Diff, DiffKind, Hunk};
use anyhow::Result;
use async_channel::Sender;
use chrono::{DateTime, FixedOffset, LocalResult, TimeZone};
use git2;
use gtk4::gio;
use log::info;
use std::path::PathBuf;

pub trait CommitRepr {
    fn dt(&self) -> DateTime<FixedOffset>;
    fn log_message(&self) -> String;
    fn message(&self) -> String;
    fn raw_message(&self) -> String;
    fn author(&self) -> String;
}

impl CommitRepr for git2::Commit<'_> {
    fn dt(&self) -> DateTime<FixedOffset> {
        let tz = FixedOffset::east_opt(self.time().offset_minutes() * 60).unwrap();
        match tz.timestamp_opt(self.time().seconds(), 0) {
            LocalResult::Single(dt) => dt,
            LocalResult::Ambiguous(dt, _) => dt,
            _ => todo!("not implemented"),
        }
    }
    fn log_message(&self) -> String {
        let message = self
            .message()
            .unwrap_or("")
            .split('\n')
            .next()
            .unwrap_or("");
        let mut encoded = String::new();
        html_escape::encode_safe_to_string(message, &mut encoded);
        encoded
    }

    // TODO rename to encoded message!
    fn message(&self) -> String {
        let mut message = self.body().unwrap_or("");
        if message.is_empty() {
            message = self.message().unwrap_or("");
        }
        let mut encoded = String::from("");
        html_escape::encode_safe_to_string(message, &mut encoded);
        encoded
    }

    fn raw_message(&self) -> String {
        let mut message = self.body().unwrap_or("");
        if message.is_empty() {
            message = self.message().unwrap_or("");
        }
        message.to_string()
    }

    fn author(&self) -> String {
        let author = self.author();
        format!(
            "{} {}",
            author.name().unwrap_or(""),
            author.email().unwrap_or("")
        )
    }
}

#[derive(Debug, Clone)]
pub enum CommitRelation {
    Right(String),
    Left(String),
    None,
}

#[derive(Debug, Clone)]
pub struct CommitLog {
    pub oid: git2::Oid,
    pub message: String,
    pub commit_dt: DateTime<FixedOffset>,
    pub author: String,
    pub from: CommitRelation,
}

impl CommitLog {
    pub fn from_log(commit: git2::Commit, from: CommitRelation) -> Self {
        Self {
            oid: commit.id(),
            message: CommitRepr::log_message(&commit),
            commit_dt: CommitRepr::dt(&commit),
            author: CommitRepr::author(&commit),
            from,
        }
    }
}
impl Default for CommitLog {
    fn default() -> Self {
        Self {
            oid: git2::Oid::zero(),
            message: String::from(""),
            commit_dt: DateTime::<FixedOffset>::MIN_UTC.into(),
            author: String::from(""),
            from: CommitRelation::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommitDiff {
    pub oid: git2::Oid,
    pub message: String,
    pub commit_dt: DateTime<FixedOffset>,
    pub author: String,
    pub diff: Diff,
}

impl Default for CommitDiff {
    fn default() -> Self {
        Self {
            oid: git2::Oid::zero(),
            message: String::from(""),
            commit_dt: DateTime::<FixedOffset>::MIN_UTC.into(),
            author: String::from(""),
            diff: Diff::new(DiffKind::Unstaged),
        }
    }
}

impl CommitDiff {
    pub fn new(commit: git2::Commit, diff: Diff) -> Self {
        Self {
            oid: commit.id(),
            message: CommitRepr::message(&commit),
            commit_dt: CommitRepr::dt(&commit),
            author: CommitRepr::author(&commit),
            diff,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.diff.is_empty()
    }
}

pub fn get_commit_diff(path: PathBuf, oid: git2::Oid) -> Result<CommitDiff, git2::Error> {
    let repo = git2::Repository::open(path)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let mut parent_tree: Option<git2::Tree> = None;
    if let Ok(parent) = commit.parent(0) {
        let tree = parent.tree()?;
        parent_tree.replace(tree);
    }
    let git_diff = repo.diff_tree_to_tree(
        parent_tree.as_ref(),
        Some(&tree),
        Some(&mut make_diff_options()),
    )?;
    Ok(CommitDiff::new(
        commit,
        make_diff(&git_diff, DiffKind::Commit), // was Staged
    ))
}

pub fn create(
    path: PathBuf,
    message: String,
    amend: bool,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let me = repo.signature()?;
    if message.is_empty() {
        return Err(git2::Error::from_str("Commit message is required"));
    }
    let tree_oid = repo.index()?.write_tree()?;

    let tree = repo.find_tree(tree_oid)?;

    if let Ok(ob) = repo.revparse_single("HEAD^{commit}") {
        let parent_commit = repo.find_commit(ob.id())?;
        if amend {
            parent_commit.amend(
                Some("HEAD"),
                Some(&me),
                Some(&me),
                None, // message encoding
                Some(&message),
                Some(&tree),
            )?;
        } else {
            repo.commit(Some("HEAD"), &me, &me, &message, &tree, &[&parent_commit])?;
        }
    } else {
        repo.commit(Some("HEAD"), &me, &me, &message, &tree, &[])?;
    }

    // update staged changes.
    let ob = repo.revparse_single("HEAD^{tree}")?;
    let current_tree = repo.find_tree(ob.id())?;
    let git_diff =
        repo.diff_tree_to_index(Some(&current_tree), None, Some(&mut make_diff_options()))?;

    let diff = make_diff(&git_diff, DiffKind::Staged);
    sender
        .send_blocking(crate::Event::Staged(if diff.is_empty() {
            None
        } else {
            Some(diff)
        }))
        .expect("Could not send through channel");

    // get_unstaged
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            let repo = git2::Repository::open(path).expect("can't open repo");
            let git_diff = repo
                .diff_index_to_workdir(None, Some(&mut make_diff_options()))
                .expect("cant' get diff index to workdir");
            let diff = make_diff(&git_diff, DiffKind::Unstaged);
            sender
                .send_blocking(crate::Event::Unstaged(if diff.is_empty() {
                    None
                } else {
                    Some(diff)
                }))
                .expect("Could not send through channel");
        }
    });
    let head = get_head(path).expect("cant get head");
    sender
        .send_blocking(crate::Event::Head(Some(head)))
        .expect("Could not send through channel");
    Ok(())
}

pub fn apply(
    path: PathBuf,
    oid: git2::Oid,
    revert: bool,
    file_path: Option<PathBuf>,
    nocommit: bool,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    info!(
        "git apply commit {:?} {:?} {:?} {:?}",
        oid, revert, file_path, nocommit
    );
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);

    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(oid)?;

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");

    if nocommit {
        let head_ref = repo.head()?;
        let ob = head_ref.peel(git2::ObjectType::Commit)?;
        let our_commit = ob.peel_to_commit()?;
        let mut memory_index = if revert {
            repo.revert_commit(&commit, &our_commit, 0, None)?
        } else {
            repo.cherrypick_commit(&commit, &our_commit, 0, None)?
        };
        let mut cb = git2::build::CheckoutBuilder::new();
        if let Some(file_path) = file_path {
            cb.path(file_path);
        };
        repo.checkout_index(Some(&mut memory_index), Some(&mut cb))?;
    } else {
        let mut cb: Option<git2::build::CheckoutBuilder> = None;
        if let Some(file_path) = file_path {
            let mut cbuilder = git2::build::CheckoutBuilder::new();
            cbuilder.path(file_path);
            cb = Some(cbuilder)
        };
        if revert {
            let mut opts = git2::RevertOptions::new();
            if let Some(cb) = cb {
                opts.checkout_builder(cb);
            }
            repo.revert(&commit, Some(&mut opts))?;
        } else {
            let mut opts = git2::CherrypickOptions::new();
            if let Some(cb) = cb {
                opts.checkout_builder(cb);
            }
            repo.cherrypick(&commit, Some(&mut opts))?;
        }
    }
    Ok(())
}

pub fn from_short_sha(path: PathBuf, short_sha: String) -> Result<git2::Oid> {
    let repo = git2::Repository::open(path.clone())?;
    let object = repo.revparse_single(&short_sha)?;
    Ok(object.id())
}

pub fn partial_apply(
    path: PathBuf,
    oid: git2::Oid,
    revert: bool,
    file_path: PathBuf,
    hunk_header: Option<String>,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let _defer = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    info!(
        "partial apply {:?} {:?} {:?}",
        file_path, hunk_header, revert
    );
    let repo = git2::Repository::open(path.clone())?;

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    let commit = repo.find_commit(oid)?;

    let head_ref = repo.head()?;
    let ob = head_ref.peel(git2::ObjectType::Commit)?;
    let our_commit = ob.peel_to_commit()?;
    let memory_index = if revert {
        repo.revert_commit(&commit, &our_commit, 0, None)?
    } else {
        repo.cherrypick_commit(&commit, &our_commit, 0, None)?
    };
    let mut diff_opts = make_diff_options();
    diff_opts.reverse(true);

    let git_diff = repo.diff_index_to_workdir(Some(&memory_index), Some(&mut diff_opts))?;

    let mut options = git2::ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if let Some(hunk_header) = &hunk_header {
            if let Some(dh) = odh {
                let mut header = Hunk::get_header_from(&dh);
                if revert {
                    header = Hunk::reverse_header(&header);
                }
                return *hunk_header == header;
            }
        }
        true
    });

    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: PathBuf = dd.new_file().path().unwrap().into();
            return file_path == path;
        }
        true
    });
    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut options))?;
    Ok(())
}
