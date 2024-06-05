use crate::git::{
    branch::BranchName, get_conflicted_v1, get_current_repo_status, make_diff,
    make_diff_options, BranchData, DiffKind, Hunk, Line, MARKER_HUNK, MARKER_OURS, MARKER_THEIRS, MARKER_VS, MINUS,
    SPACE, NEW_LINE
};
use async_channel::Sender;
use git2;
use gtk4::gio;
use log::{debug, info, trace};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::from_utf8,
};

pub const STAGE_FLAG: u16 = 0x3000;

pub fn final_commit(
    path: PathBuf,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {

    let repo = git2::Repository::open(path.clone())?;
    let me = repo.signature()?;

    let my_oid = repo.revparse_single("HEAD^{commit}")?.id();
    
    let my_commit = repo.find_commit(my_oid)?;

    let message = repo.message()?;

    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());

    let tree_oid = repo.index()?.write_tree()?;

    let tree = repo.find_tree(tree_oid)?;

    repo.commit(
        Some("HEAD"),
        &me,
        &me,
        &message,
        &tree,
        &[&my_commit],
    )?;

    repo.cleanup_state()?;
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    Ok(())
}

pub fn final_merge_commit(
    path: PathBuf,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let mut repo = git2::Repository::open(path.clone())?;
    let me = repo.signature()?;

    let my_oid = repo.revparse_single("HEAD^{commit}")?.id();

    let mut their_oid: Option<git2::Oid> = None;
    repo.mergehead_foreach(|oid_ref| -> bool {
        their_oid.replace(*oid_ref);
        true
    })?;

    let their_oid = their_oid.unwrap();
    info!("creating merge commit for {:?} {:?}", my_oid, their_oid);

    let my_commit = repo.find_commit(my_oid)?;
    let their_commit = repo.find_commit(their_oid)?;

    // let message = message.unwrap_or(repo.message().expect("cant get merge message"));

    let mut their_branch: Option<git2::Branch> = None;
    let refs = repo.references()?;
    for r in refs.into_iter().flatten() {
        if let Some(oid) = r.target() {
            if oid == their_oid {
                their_branch.replace(git2::Branch::wrap(r));
            }
        }
    }
    let their_branch = their_branch.unwrap();

    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());
    let my_branch = git2::Branch::wrap(head_ref);
    let message = format!(
        "merge branch {} into {}",
        their_branch.branch_name(),
        my_branch.branch_name()
    );

    let tree_oid = repo.index()?.write_tree()?;

    let tree = repo.find_tree(tree_oid)?;

    repo.commit(
        Some("HEAD"),
        &me,
        &me,
        &message,
        &tree,
        &[&my_commit, &their_commit],
    )?;

    repo.cleanup_state()?;
    gio::spawn_blocking({
        move || {
            get_current_repo_status(Some(path), sender);
        }
    });
    Ok(())
}

pub fn branch(
    path: PathBuf,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<Option<BranchData>, git2::Error> {
    info!("merging {:?}", branch_data.name);
    let repo = git2::Repository::open(path.clone())?;
    let annotated_commit = repo.find_annotated_commit(branch_data.oid)?;

    match repo.merge_analysis(&[&annotated_commit]) {
        Ok((analysis, _)) if analysis.is_up_to_date() => {
            info!("merge.uptodate");
        }

        Ok((analysis, preference))
            if analysis.is_fast_forward()
                && !preference.is_no_fast_forward() =>
        {
            info!("merge.fastforward");
            let ob = repo.find_object(
                branch_data.oid,
                Some(git2::ObjectType::Commit),
            )?;
            repo.checkout_tree(
                &ob,
                Some(git2::build::CheckoutBuilder::new().safe()),
            )?;
            repo.reset(&ob, git2::ResetType::Soft, None)?;
        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {
            info!("merge.normal");
            repo.merge(&[&annotated_commit], None, None)?;

            let index = repo.index()?;
            if index.has_conflicts() {
                gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || {
                        get_current_repo_status(Some(path), sender.clone());
                    }
                });
                return Ok(None);
            }
            final_merge_commit(path, sender.clone())?;
        }
        Ok((analysis, preference)) => {
            todo!("not implemented case {:?} {:?}", analysis, preference);
        }
        Err(err) => {
            panic!("error in merge_analysis {:?}", err.message());
        }
    }

    // let state = repo.state();
    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());
    // let ob = head_ref.peel(git2::ObjectType::Commit)?;
    // let commit = ob.peel_to_commit()?;
    let branch = git2::Branch::wrap(head_ref);
    // let new_head = Head::new(&branch, &commit);
    // sender
    //     .send_blocking(crate::Event::State(State::new(state, branch.branch_name())))
    //     .expect("Could not send through channel");
    // sender
    //     .send_blocking(crate::Event::Head(new_head))
    //     .expect("Could not send through channel");
    BranchData::from_branch(branch, git2::BranchType::Local)
}

pub fn abort(path: PathBuf, sender: Sender<crate::Event>) {
    info!("git.abort merge");

    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut checkout_builder = git2::build::CheckoutBuilder::new();

    let index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut has_conflicts = false;
    for conflict in conflicts.flatten() {
        if let Some(our) = conflict.our {
            checkout_builder.path(our.path);
            has_conflicts = true;
        }
    }
    if !has_conflicts {
        panic!("no way to abort merge without conflicts");
    }

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo
        .diff_tree_to_index(Some(&current_tree), None, None)
        .expect("can't get diff tree to index");
    git_diff
        .foreach(
            &mut |d: git2::DiffDelta, _| {
                let path = d.new_file().path().expect("cant get path");
                checkout_builder.path(path);
                true
            },
            None,
            None,
            None,
        )
        .expect("cant foreach on diff");

    let head_ref = repo.head().expect("can't get head");

    let ob = head_ref
        .peel(git2::ObjectType::Commit)
        .expect("can't get commit from ref!");

    repo.reset(&ob, git2::ResetType::Hard, Some(&mut checkout_builder))
        .expect("cant reset hard");

    get_current_repo_status(Some(path), sender);
}

pub fn choose_conflict_side(
    path: PathBuf,
    ours: bool,
    sender: Sender<crate::Event>,
) {
    info!("git.choose side");
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut entries: Vec<git2::IndexEntry> = Vec::new();
    for conflict in conflicts.flatten() {
        if ours {
            if let Some(our) = conflict.our {
                entries.push(our);
            }
        } else if let Some(their) = conflict.their {
            entries.push(their);
        }
    }
    if entries.is_empty() {
        panic!("nothing to resolve in choose_conflict_side");
    }

    let mut diff_opts = make_diff_options();
    diff_opts.reverse(true);

    for entry in &mut entries {
        let pth =
            String::from_utf8(entry.path.clone()).expect("cant get path");
        diff_opts.pathspec(pth.clone());
        index
            .remove_path(Path::new(&pth))
            .expect("cant remove path");
        entry.flags &= !STAGE_FLAG;
        index.add(entry).expect("cant add to index");
    }
    index.write().expect("cant write index");
    let git_diff = repo
        .diff_index_to_workdir(Some(&index), Some(&mut diff_opts))
        .expect("cant get diff");

    let mut apply_opts = git2::ApplyOptions::new();

    let mut conflicted_headers = HashSet::new();
    let diff = make_diff(&git_diff, DiffKind::Conflicted);

    for f in diff.files {
        for h in f.hunks {
            if h.conflicts_count > 0 {
                conflicted_headers.insert(h.header);
            }
        }
    }

    apply_opts.hunk_callback(move |odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            let matched = conflicted_headers.contains(&header);
            debug!("header in callback  {:?} ??= {:?}", header, matched);
            return matched;
        }
        false
    });

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.apply(
        &git_diff,
        git2::ApplyLocation::WorkDir,
        Some(&mut apply_opts),
    )
    .expect("can't apply patch");

    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // if their side is choosen, it need to stage all conflicted paths
    // because resolved conflicts will go to staged area, but other changes
    // will be on other side of stage (will be +- same hunks on both sides)
    for entry in &entries {
        let pth =
            String::from_utf8(entry.path.clone()).expect("cant get path");
        index.add_path(Path::new(&pth)).expect("cant add path");
    }
    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender);
}

trait PathHolder {
    fn get_path(&self) -> PathBuf;
}

impl PathHolder for git2::IndexConflict {
    fn get_path(&self) -> PathBuf {
        PathBuf::from(
            from_utf8(match (&self.our, &self.their) {
                (Some(o), _) => &o.path[..],
                (_, Some(t)) => &t.path[..],
                _ => panic!("no path"),
            })
            .unwrap(),
        )
    }
}


pub fn choose_conflict_side_of_blob<'a, F>(raw: &'a str,
                                           hunk_deltas: &mut Vec<(&'a str, i32)>,
                                           predicate: F,
                                           ours_choosed: bool) -> String
    where F: Fn(i32, &str) -> bool
{

    let mut acc = Vec::new();

    let mut lines = raw.lines();

    let mut line_offset_inside_hunk: i32 = -1; // first line in hunk will be 0

    while let Some(line) = lines.next() {
        if !line.is_empty() && line[1..].starts_with(MARKER_OURS) {
            // is it marker that we need?
            line_offset_inside_hunk += 1;
            let mut this_is_current_conflict = false;

            if predicate(line_offset_inside_hunk, hunk_deltas.last().unwrap().0)
            {
                this_is_current_conflict = true;
            }
            if this_is_current_conflict {
                // this marker will be deleted
                acc.push(line);
                acc.push(NEW_LINE);
            } else {
                // do not delete it for now
                acc.push(SPACE);
                acc.push(&line[1..]);
                acc.push(NEW_LINE);
                // delta += 1;
                let hd = hunk_deltas.last().unwrap();
                let le = hunk_deltas.len();
                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                trace!(
                    "......remain marker ours when not found {:?}",
                    hunk_deltas
                );
            }
            // go deeper inside OURS
            'ours: while let Some(line) = lines.next() {
                line_offset_inside_hunk += 1;
                if !line.is_empty() && line[1..].starts_with(MARKER_VS) {
                    if this_is_current_conflict {
                        // this marker will be deleted
                        acc.push(line);
                        acc.push(NEW_LINE);
                    } else {
                        // do not delete it for now
                        acc.push(SPACE);
                        acc.push(&line[1..]);
                        acc.push(NEW_LINE);
                        // delta += 1;
                        let hd = hunk_deltas.last().unwrap();
                        let le = hunk_deltas.len();
                        hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                        trace!(
                            "......remain marker vs when not found {:?}",
                            hunk_deltas
                        );
                    }
                    // go deeper inside THEIRS
                    for line in lines.by_ref() {
                        line_offset_inside_hunk += 1;
                        if !line.is_empty()
                            && line[1..].starts_with(MARKER_THEIRS)
                        {
                            if this_is_current_conflict {
                                // this marker will be deleted
                                acc.push(line);
                                acc.push(NEW_LINE);
                            } else {
                                // do not delete it for now
                                acc.push(" ");
                                acc.push(&line[1..]);
                                acc.push(NEW_LINE);
                                // delta += 1;
                                let hd = hunk_deltas.last().unwrap();
                                let le = hunk_deltas.len();
                                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                                trace!("......remain marker theirs when not found {:?}", hunk_deltas);
                            }
                            // conflict is over
                            // go out to next conflict
                            break 'ours;
                        } else {
                            // THEIR lines between === and >>>>
                            // this lines are deleted in this diff
                            // lets adjust it
                            if this_is_current_conflict {
      
                                if ours_choosed {
                                    // theirs will be deleted
                                    acc.push(line);
                                    acc.push(NEW_LINE);
                                } else {
                                    // do not delete theirs!
                                    acc.push(SPACE);
                                    acc.push(&line[1..]);
                                    acc.push(NEW_LINE);
                                    // delta += 1;
                                    let hd = hunk_deltas.last().unwrap();
                                    let le = hunk_deltas.len();
                                    hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                                    trace!(
                                        "......remain theirs in found {:?}",
                                        hunk_deltas
                                    );
                                }
                            } else {
                                // do not delete for now
                                acc.push(SPACE);
                                acc.push(&line[1..]);
                                acc.push(NEW_LINE);             
                                let hd = hunk_deltas.last().unwrap();
                                let le = hunk_deltas.len();
                                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);
                                trace!("......remain theirs when not in found {:?}", hunk_deltas);
                            }
                        }
                    }
                } else {
                    // OUR lines between <<< and ====
                    // in this diff they are not deleted
                    if this_is_current_conflict {
                        if ours_choosed {
                            // remain our lines
                            acc.push(line);
                            acc.push("\n");
                        } else {
                            // delete our lines!
                            acc.push(MINUS);
                            acc.push(&line[1..]);
                            acc.push(NEW_LINE);
                            let hd = hunk_deltas.last().unwrap();
                            let le = hunk_deltas.len();
                            hunk_deltas[le - 1] = (hd.0, hd.1 - 1);
                            trace!(
                                "......delete ours in found {:?}",
                                hunk_deltas
                            );
                        }
                    } else {
                        // remain our lines
                        acc.push(line);
                        acc.push(NEW_LINE);
                    }
                }
            }
        } else {
            // line not belonging to conflict
            if !line.is_empty() && line[1..].contains(MARKER_HUNK) {
                hunk_deltas.push((line, 0));
                trace!(
                    "----------->reset offset for hunk {:?} {:?}",
                    line_offset_inside_hunk,
                    line
                );
                line_offset_inside_hunk = -1;
            } else {
                trace!(
                    "increment offset for line {:?} {:?}",
                    line_offset_inside_hunk,
                    line
                );
                line_offset_inside_hunk += 1;
            }
            if !hunk_deltas.is_empty() && line.starts_with(MINUS) {
                // when 1 hunk have multiple conflicts
                // perhaps here will be conflicts resolved
                // in previous turn. They already stripped off
                // conflicts markers, but their choosen lines
                // will be marked for deletion (there are no such lines
                // in tree and the diff is reversed
                acc.push(SPACE);
                acc.push(&line[1..]);
                acc.push(NEW_LINE);
                let hd = hunk_deltas.last().unwrap();
                let le = hunk_deltas.len();
                hunk_deltas[le - 1] = (hd.0, hd.1 + 1);                
            } else {
                acc.push(line);
                acc.push(NEW_LINE);
            }
        }
    }
    acc.iter().fold("".to_string(), |cur, nxt| cur + nxt)
}


pub fn choose_conflict_side_of_hunk(
    path: PathBuf,
    file_path: PathBuf,
    hunk: Hunk,
    line: Line,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    info!(
        "choose_conflict_side_of_hunk {:?} Line: {:?}",
        hunk.header, line.content
    );
    let repo = git2::Repository::open(path.clone())?;
    let mut index = repo.index()?;
    let conflicts = index.conflicts()?;

    let chosen_path = PathBuf::from(&file_path);

    let current_conflict = conflicts
        .filter(|c| c.as_ref().unwrap().get_path() == chosen_path)
        .next()
        .unwrap()
        .unwrap();

    index
        .remove_path(chosen_path.as_path())?;

    let ob = repo.revparse_single("HEAD^{tree}")?;
    let current_tree = repo.find_tree(ob.id())?;

    let mut opts = make_diff_options();
    let mut opts = opts.pathspec(&file_path).reverse(true);

    let mut git_diff = repo
        .diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))?;

    let mut reversed_header = Hunk::reverse_header(&hunk.header);
    
    let mut apply_options = git2::ApplyOptions::new();

    let file_path_clone = file_path.clone();

    let mut patch = git2::Patch::from_diff(&git_diff, 0)?.unwrap();

    let buff = patch.to_buf()?;
    let raw = buff.as_str().unwrap();

    let ours_choosed = line.is_our_side_of_conflict();
    let mut hunk_deltas: Vec<(&str, i32)> = Vec::new();

    let conflict_offset_inside_hunk = hunk.get_conflict_offset_by_line(&line);

    let mut new_body = choose_conflict_side_of_blob(
        raw,
        &mut hunk_deltas,
        |line_offset_inside_hunk, hunk_header| {
            line_offset_inside_hunk == conflict_offset_inside_hunk
                &&
                hunk_header == reversed_header
        },
        ours_choosed
    );

    // so. not only new lines are changed. new_start are changed also!!!!!!
    // it need to add delta of prev hunk int new start of next hunk!!!!!!!!
    let mut prev_delta = 0;
    for (hh, delta) in hunk_deltas {
        let new_header =
            Hunk::shift_new_start_and_lines(hh, prev_delta, delta);
        new_body = new_body.replace(hh, &new_header);
        if hh == reversed_header {
            reversed_header = new_header;
        }
        prev_delta = delta;
    }

    
    git_diff = git2::Diff::from_buffer(new_body.as_bytes())?;
    
    apply_options.hunk_callback(|odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            return header == reversed_header;
        }
        false
    });
    apply_options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: PathBuf = dd.new_file().path().unwrap().into();
            return file_path == path;
        }
        todo!("diff without delta");
    });
    
    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    let apply_error = repo.apply(
        &git_diff,
        git2::ApplyLocation::WorkDir,
        Some(&mut apply_options)
    ).err();
    
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // remove from index again to restore conflict
    // and also to clear from other side tree
    index
        .remove_path(Path::new(&file_path.clone()))?;

    if let Some(entry) = &current_conflict.ancestor {
        index.add(entry)?;
    }
    if let Some(entry) = &current_conflict.our {
        index.add(entry)?;
    }
    if let Some(entry) = &current_conflict.their {
        index.add(entry)?;
    }
    index.write()?;

    cleanup_last_conflict_for_file(path, file_path_clone, sender)?;
    if let Some(error) = apply_error {
        return Err(error);
    }
    Ok(())
}

pub fn cleanup_last_conflict_for_file(
    path: PathBuf,
    file_path: PathBuf,
    sender: Sender<crate::Event>,
) -> Result<(), git2::Error> {
    let diff = get_conflicted_v1(path.clone());
    let repo = git2::Repository::open(path.clone())?;
    let mut index = repo.index()?;

    let has_conflicts = diff
        .files
        .iter()
        .flat_map(|f| &f.hunks)
        .any(|h| h.conflicts_count > 0);
    if !has_conflicts {
        // file is clear now
        // stage it!
        index
            .remove_path(Path::new(&file_path))?;
        index
            .add_path(Path::new(&file_path))?;
        index.write()?;
        // perhaps it will be another files with conflicts
        // perhaps not
        // it need to rerender everything
        gio::spawn_blocking({
            move || {
                get_current_repo_status(Some(path), sender);
            }
        });
        return Ok(());
    }
    sender
        .send_blocking(crate::Event::Conflicted(diff))
        .expect("Could not send through channel");
    Ok(())
}
