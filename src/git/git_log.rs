use std::path::PathBuf;
use log::debug;
use crate::git::{commit::CommitDiff};

const COMMIT_PAGE_SIZE: i32 = 500;

pub fn revwalk(
    path: PathBuf,
    start: Option<git2::Oid>,
    search_term: Option<String>,
) -> Result<Vec<CommitDiff>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let mut revwalk = repo.revwalk()?;
    // revwalk.simplify_first_parent()?;
    let mut i = 0;
    if let Some(oid) = start {
        revwalk.push(oid)?;
    } else {
        revwalk.push_head()?;
    }
    let mut result: Vec<CommitDiff> = Vec::new();
    for commit in revwalk.filter_map(|oid| {
        if let Ok(oid) = oid {
            if let Ok(commit) = repo.find_commit(oid) {
                match commit.parent_count() {
                    1 => {
                        return Some(commit);
                    }
                    2 => {
                        let mut result: Option<git2::Commit> = None;
                        if let Ok(left) = commit.parent(0) {
                            if left.parent_count() == 1 {
                                debug!("FOUND LEFT single parent commit parent  ------------------> PARENT {:?} LEFT {:?}", commit, left);
                                result.replace(left);
                            }
                        }
                        if let Ok(right) = commit.parent(1) {
                            if right.parent_count() == 1 {
                                if let Some(left) = &result {
                                    debug!("FOUND BOTH parents to be single-parent commit  ------------------> PARENT {:?} LEFT {:?} RIGHT {:?}", commit, left, right)
                                } else {
                                    debug!("FOUND RIGHT single parent commit parent  ------------------> PARENT {:?} RIGHT {:?}", commit, right);
                                    result.replace(right);                                    
                                }
                            }
                        }
                        return result;
                    }
                    _ => {
                        panic!("more then 2 commits");
                    }
                }
            }
        }        
        None
    }) {
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
    Ok(result)
}
