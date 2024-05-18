use std::path::{PathBuf};
use async_channel::Sender;
use chrono::{DateTime, FixedOffset, LocalResult, TimeZone};
use crate::git::{Diff, DiffKind, make_diff};
use git2;

pub fn commit_dt(c: &git2::Commit) -> DateTime<FixedOffset> {
    let tz = FixedOffset::east_opt(c.time().offset_minutes() * 60).unwrap();
    match tz.timestamp_opt(c.time().seconds(), 0) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        _ => todo!("not implemented"),
    }
}


pub fn commit_string(c: &git2::Commit) -> String {
    let message = c.message().unwrap_or("").replace('\n', "");
    let mut encoded = String::new();
    html_escape::encode_safe_to_string(&message, &mut encoded);
    format!("{} {}", &c.id().to_string()[..7], encoded)
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
        CommitDiff {
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
        CommitDiff {
            oid: commit.id(),
            message: commit.message().unwrap_or("").replace('\n', ""),
            commit_dt: commit_dt(&commit),
            author: String::from(commit.author().name().unwrap_or("")),
            diff,
        }
    }
    pub fn from_commit(commit: git2::Commit) -> Self {
        CommitDiff {
            oid: commit.id(),
            message: commit.message().unwrap_or("").replace('\n', ""),
            commit_dt: commit_dt(&commit),
            author: String::from(commit.author().name().unwrap_or("")),
            diff: Diff::new(DiffKind::Unstaged),
        }
    }
}


pub fn get_commit_diff(path: PathBuf, oid: git2::Oid, sender: Sender<crate::Event>) {
    let repo = git2::Repository::open(path).expect("can't open repo");
    let commit = repo.find_commit(oid).expect("cant find commit");
    let tree = commit.tree().expect("no get tree from commit");
    let parent = commit.parent(0).expect("cant get commit parent");

    let parent_tree = parent.tree().expect("no get tree from PARENT commit");
    let git_diff = repo
        .diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)
        .expect("can't get diff tree to index");
    let commit_diff =
        CommitDiff::new(commit, make_diff(&git_diff, DiffKind::Staged));
    sender
        .send_blocking(crate::Event::CommitDiff(commit_diff))
        .expect("Could not send through channel");
}

const COMMIT_PAGE_SIZE: i32 = 500;

pub fn revwalk(
    path: PathBuf,
    start: Option<git2::Oid>,
    search_term: Option<String>,
) -> Vec<CommitDiff> {
    let repo = git2::Repository::open(path.clone()).expect("cant open repo");
    let mut revwalk = repo.revwalk().expect("cant get revwalk");
    revwalk.simplify_first_parent().expect("cant simplify");
    let mut i = 0;
    if let Some(oid) = start {
        revwalk.push(oid).expect("cant push oid to revlog");
    } else {
        revwalk.push_head().expect("no head for refwalk?");
    }
    let mut result: Vec<CommitDiff> = Vec::new();
    for oid in revwalk {
        let oid = oid.expect("no oid in rev");
        let commit = repo.find_commit(oid).expect("can't find commit");
        if let Some(ref term) = search_term {
            let mut found = false;
            for el in [
                commit.message().unwrap_or("").to_lowercase(),
                commit.author().name().unwrap_or("").to_lowercase(),
            ] {
                if el.contains(term) {
                    found = true;
                    break;
                }
            }
            if !found {
                continue;
            }
        }
        result.push(CommitDiff::from_commit(commit));
        i += 1;
        if i == COMMIT_PAGE_SIZE {
            break;
        }
    }
    result
}
