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
    revwalk.simplify_first_parent()?;
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
                        if let Ok(commit) = commit.parent(0) {
                            if commit.parent_count() == 1 {
                                result.replace(commit);
                            }
                        }
                        if let Ok(commit) = commit.parent(1) {
                            if commit.parent_count() == 1 {
                                if let Some(found) = result {
                                    panic!("FOUND------------------> {:?} vs {:?}", found, commit)
                                }
                                result.replace(commit);
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
