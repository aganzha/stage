use crate::git::commit::{CommitLog, CommitRelation, CommitRepr};
use log::{debug, trace};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub const COMMIT_PAGE_SIZE: usize = 500;

pub fn revwalk(
    path: PathBuf,
    start: Option<git2::Oid>,
    search_term: Option<String>,
) -> Result<Vec<CommitLog>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let mut revwalk = repo.revwalk()?;
    // debug!("refwalk. search_term {:?}", search_term);
    if let Some(oid) = start {
        revwalk.push(oid)?;
    } else {
        revwalk.push_head()?;
    }

    let limit = {
        if search_term.is_some() {
            if start.is_some() {
                2
            } else {
                1
            }
        } else {
            COMMIT_PAGE_SIZE
        }
    };
    let commits = revwalk.scan(
        (HashMap::<git2::Oid, String>::new(), HashMap::<git2::Oid, String>::new()),
        |(left_commits, right_commits), oid| {
            if let Ok(oid) = oid {
                if let Ok(commit) = repo.find_commit(oid) {
                    trace!("scanning commits {:?} {:?}", oid, commit.dt());
                    match commit.parent_count() {
                        0 => {
                            // in the begining there was darkness
                            return Some(
                                (Some(commit), (left_commits.clone(), right_commits.clone()))
                            );
                        }
                        1 => {
                            if let Ok(parent) = commit.parent_id(0) {
                                if let Some(message) = left_commits.get(&commit.id()) {
                                    left_commits.insert(parent, message.to_string());
                                }
                                if let Some(message) = right_commits.get(&commit.id()) {
                                    right_commits.insert(parent, message.to_string());
                                }
                                // if parent got to left and right
                                // this means root of both branhes
                                if left_commits.contains_key(&parent) && right_commits.contains_key(&parent) {
                                    left_commits.remove(&parent);
                                    right_commits.remove(&parent);
                                }
                            }
                            return Some(
                                (Some(commit), (left_commits.clone(), right_commits.clone()))
                            );
                        }
                        2 => {
                            if let Ok(left) = commit.parent_id(0) {
                                left_commits.insert(left, commit.message().unwrap_or("").to_string());
                            }
                            if let Ok(right) = commit.parent_id(1) {
                                right_commits.insert(right, commit.message().unwrap_or("").to_string());
                            }
                            return Some((None, (left_commits.clone(), right_commits.clone())));
                        }
                        _ => {
                            panic!("got nor 1 nor 2 parents !!!!!!!!!!!! {:?}", commit);
                        }
                    }
                }
            }
            Some((None, (HashMap::new(), HashMap::new())))
        }).filter_map(|(commit, (left_commits, right_commits))| {
        if let Some(commit) = commit {
            // search by oid and author
            if let Some(ref term) = search_term {
                let mut found = false;
                for el in [
                    commit.message().unwrap_or("").to_lowercase(),
                    commit.author().name().unwrap_or("").to_lowercase(),
                    commit.id().to_string()
                ] {
                    if el.contains(term) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return None;
                }
            }

            let mut from = CommitRelation::None;

            if let Some(message) = left_commits.get(&commit.id()) {
                from = CommitRelation::Left(message.to_string())
            }
            if let Some(message) = right_commits.get(&commit.id()) {
                from = CommitRelation::Right(message.to_string())
            }
            return Some(CommitLog::from_log(commit, from));
        }
        None
    }).take(limit).collect::<Vec<CommitLog>>();
    Ok(commits)
}
