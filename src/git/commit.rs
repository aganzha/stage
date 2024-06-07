use crate::git::{
    get_current_repo_status, get_head, make_diff, Diff, DiffKind,
};
use async_channel::Sender;
use chrono::{DateTime, FixedOffset, LocalResult, TimeZone};
use git2;
use gtk4::gio;
use log::debug;
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
        let message = self.message().unwrap_or("").replace('\n', "");
        let mut encoded = String::new();
        html_escape::encode_safe_to_string(&message, &mut encoded);
        format!("{} {}", &self.id().to_string()[..7], encoded)
    }

    fn message(&self) -> String {
        let mut message = self.body().unwrap_or("");
        if message.is_empty() {
            message = self.message().unwrap_or("");
        }
        let mut encoded = String::from("");
        html_escape::encode_safe_to_string(&message, &mut encoded);
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
    let git_diff =
        repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)?;
    Ok(CommitDiff::new(
        commit,
        make_diff(&git_diff, DiffKind::Staged),
    ))
}

pub fn get_parents_for_commit(path: PathBuf) -> Vec<git2::Oid> {
    let mut repo =
        git2::Repository::open(path.clone()).expect("can't open repo");
    let mut result = Vec::new();
    let id = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse")
        .id();
    result.push(id);
    match repo.state() {
        git2::RepositoryState::Clean => {}
        git2::RepositoryState::Merge => {
            repo.mergehead_foreach(|oid: &git2::Oid| -> bool {
                result.push(*oid);
                true
            })
            .expect("cant get merge heads");
        }
        _ => {
            todo!("commit in another state")
        }
    }
    result
}

pub fn create_commit(
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
    // let ob = repo.revparse_single("HEAD^{commit}")
    //     .expect("fail revparse");
    // let id = repo.revparse_single("HEAD^{commit}")
    //     .expect("fail revparse").id();
    // let parent_commit = repo.find_commit(id).expect("cant find parent commit");
    // update_ref: Option<&str>,
    // author: &Signature<'_>,
    // committer: &Signature<'_>,
    // message: &str,
    // tree: &Tree<'_>,
    // parents: &[&Commit<'_>]
    let tree_oid = repo.index()?.write_tree()?;

    let tree = repo.find_tree(tree_oid)?;

    let commits = get_parents_for_commit(path.clone())
        .into_iter()
        .map(|oid| repo.find_commit(oid).unwrap())
        .collect::<Vec<git2::Commit>>();

    match &commits[..] {
        [commit] => {
            let tree = repo.find_tree(tree_oid).expect("can't find tree");
            repo.commit(Some("HEAD"), &me, &me, &message, &tree, &[&commit])?;
        }
        [commit, merge_commit] => {
            let merge_message = match repo.message() {
                Ok(mut msg) => {
                    if !message.is_empty() {
                        msg.push('\n');
                        msg.push_str(&message);
                    }
                    msg
                }
                _error => message,
            };
            repo.commit(
                Some("HEAD"),
                &me,
                &me,
                &merge_message,
                &tree,
                &[&commit, &merge_commit],
            )?;
            repo.cleanup_state()?;
        }
        _ => {
            todo!("multiple parents")
        }
    }
    // update staged changes
    let ob = repo.revparse_single("HEAD^{tree}")?;
    let current_tree = repo.find_tree(ob.id())?;
    let git_diff = repo.diff_tree_to_index(Some(&current_tree), None, None)?;

    sender
        .send_blocking(crate::Event::Staged(make_diff(
            &git_diff,
            DiffKind::Staged,
        )))
        .expect("Could not send through channel");

    // get_unstaged
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            let repo = git2::Repository::open(path).expect("can't open repo");
            let git_diff = repo
                .diff_index_to_workdir(None, None)
                .expect("cant' get diff index to workdir");
            let diff = make_diff(&git_diff, DiffKind::Unstaged);
            sender
                .send_blocking(crate::Event::Unstaged(diff))
                .expect("Could not send through channel");
        }
    });
    get_head(path, sender);
    Ok(())
}

pub fn cherry_pick(
    path: PathBuf,
    oid: git2::Oid,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(oid)?;

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    repo.cherrypick(&commit, Some(&mut git2::CherrypickOptions::new()))?;
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

    debug!("cherry pick could not change the current branch, cause of merge conflict.
          So it need also update status.");
    // let state = repo.state();
    // let head_ref = repo.head()?;
    // assert!(head_ref.is_branch());
    // let ob = head_ref.peel(git2::ObjectType::Commit)?;
    // let commit = ob.peel_to_commit()?;
    // let branch = git2::Branch::wrap(head_ref);
    // let new_head = Head::new(&branch, &commit);
    // sender
    //     .send_blocking(crate::Event::State(State::new(state, oid.to_string())))
    //     .expect("Could not send through channel");
    // sender
    //     .send_blocking(crate::Event::Head(new_head))
    //     .expect("Could not send through channel");
    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            get_current_repo_status(Some(path), sender.clone());
        }
    });
    Ok(())
    // branch::BranchData::from_branch(branch, git2::BranchType::Local)
}

pub fn revert(
    path: PathBuf,
    oid: git2::Oid,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let commit = repo.find_commit(oid)?;

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    repo.revert(&commit, None)?;
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

    gio::spawn_blocking({
        let sender = sender.clone();
        let path = path.clone();
        move || {
            get_current_repo_status(Some(path), sender.clone());
        }
    });
    Ok(())
}
