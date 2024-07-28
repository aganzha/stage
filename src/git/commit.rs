// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::git::{
    get_head, make_diff, make_diff_options, DeferRefresh, Diff, DiffKind,
};
use async_channel::Sender;
use chrono::{DateTime, FixedOffset, LocalResult, TimeZone};
use git2;
use gtk4::gio;
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
        let tz =
            FixedOffset::east_opt(self.time().offset_minutes() * 60).unwrap();
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

pub fn get_commit_diff(
    path: PathBuf,
    oid: git2::Oid,
) -> Result<CommitDiff, git2::Error> {
    let repo = git2::Repository::open(path)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let parent = commit.parent(0)?;

    let parent_tree = parent.tree()?;
    let git_diff = repo.diff_tree_to_tree(
        Some(&parent_tree),
        Some(&tree),
        Some(&mut make_diff_options()),
    )?;
    Ok(CommitDiff::new(
        commit,
        make_diff(&git_diff, DiffKind::Staged),
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
    let parent_oid = repo.revparse_single("HEAD^{commit}")?.id();

    let parent_commit = repo.find_commit(parent_oid)?;
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
        repo.commit(
            Some("HEAD"),
            &me,
            &me,
            &message,
            &tree,
            &[&parent_commit],
        )?;
    }
    // update staged changes
    let ob = repo.revparse_single("HEAD^{tree}")?;
    let current_tree = repo.find_tree(ob.id())?;
    let git_diff = repo.diff_tree_to_index(
        Some(&current_tree),
        None,
        Some(&mut make_diff_options()),
    )?;

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
    get_head(path, sender);
    Ok(())
}

pub fn cherry_pick(
    path: PathBuf,
    oid: git2::Oid,
    file_path: Option<PathBuf>,
    _hunk_header: Option<String>,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);

    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(oid)?;

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");

    let mut cherry_pick_options = git2::CherrypickOptions::new();
    if let Some(file_path) = file_path {
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.path(file_path);
        cherry_pick_options.checkout_builder(cb);
    };
    repo.cherrypick(&commit, Some(&mut cherry_pick_options))?;
    Ok(())
}

pub fn revert(
    path: PathBuf,
    oid: git2::Oid,
    file_path: Option<PathBuf>,
    _hunk_header: Option<String>,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(oid)?;

    let mut revert_options = git2::RevertOptions::new();
    if let Some(file_path) = file_path {
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.path(file_path);
        revert_options.checkout_builder(cb);
    };
    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");

    repo.revert(&commit, Some(&mut revert_options))?;

    Ok(())
}
