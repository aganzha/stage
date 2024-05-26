use std::path::PathBuf;
use log::debug;
use std::collections::HashSet;
use crate::git::{commit::CommitLog};

const COMMIT_PAGE_SIZE: usize = 500;

pub fn revwalk(
    path: PathBuf,
    start: Option<git2::Oid>,
    search_term: Option<String>,
) -> Result<Vec<CommitLog>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let mut revwalk = repo.revwalk()?;
    // revwalk.simplify_first_parent()?;
    if let Some(oid) = start {
        revwalk.push(oid)?;
    } else {
        revwalk.push_head()?;
    }

    let commits = revwalk.enumerate().scan(HashSet::<git2::Oid>::new(), |right_commits, (i, oid)| {
        if i == COMMIT_PAGE_SIZE {
            return None;
        }
        if let Ok(oid) = oid {
            if let Ok(commit) = repo.find_commit(oid) {
                match commit.parent_count() {
                    0 => {
                        // in the begining there was darkness
                        return Some((Some(commit), right_commits.clone()));
                    }
                    1 => {
                        return Some((Some(commit), right_commits.clone()));
                    }
                    2 => {
                        if let Ok(right) = commit.parent_id(1) {
                            right_commits.insert(right);
                            debug!("FOUND RIGHT =========== {:?}", right);
                        }
                        return Some((None, right_commits.clone()));
                    }
                    _ => {
                        panic!("got nor 1 nor 2 parents !!!!!!!!!!!! {:?}", commit);
                    }                        
                }
            }
        }
        Some((None, right_commits.clone()))
    }).filter_map(|(commit, right_commits)| {
        if let Some(commit) = commit {
            // search by oid and author
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
                    return None;
                }
            }
            let from = {
                if right_commits.contains(&commit.id()) {
                    "right"
                } else {
                    ""
                }
            };
            return Some(CommitLog::from_log(commit, from.to_string()));
        }
        None
    }).collect::<Vec<CommitLog>>();
    Ok(commits)
}
