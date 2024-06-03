use crate::git::{
    get_conflicted_v1, get_current_repo_status, make_diff, make_diff_options,
    BranchData, DiffKind, Head, Hunk, Line, LineKind, State, MARKER_HUNK,
    MARKER_OURS, MARKER_THEIRS, MARKER_VS,
};
use async_channel::Sender;
use git2;
use gtk4::gio;
use log::{debug, info, trace};
use std::{
    collections::{HashSet},
    path::{Path, PathBuf},
    str::from_utf8,
};

pub const STAGE_FLAG: u16 = 0x3000;


pub fn commit(path: PathBuf) {
    let mut repo =
        git2::Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");

    let my_oid = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse")
        .id();

    let mut their_oid: Option<git2::Oid> = None;
    repo.mergehead_foreach(|oid_ref| -> bool {
        their_oid.replace(*oid_ref);
        true
    })
    .expect("cant get merge heads");

    let their_oid = their_oid.unwrap();
    info!("creating merge commit for {:?} {:?}", my_oid, their_oid);

    let my_commit = repo.find_commit(my_oid).expect("cant get commit");
    let their_commit = repo.find_commit(their_oid).expect("cant get commit");

    // let message = message.unwrap_or(repo.message().expect("cant get merge message"));

    let mut their_branch: Option<git2::Branch> = None;
    let refs = repo.references().expect("no refs");
    for r in refs.into_iter().flatten() {
        if let Some(oid) = r.target() {
            if oid == their_oid {
                their_branch.replace(git2::Branch::wrap(r));
            }
        }
    }
    let their_branch = their_branch.unwrap();

    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let my_branch = git2::Branch::wrap(head_ref);
    let message = format!(
        "merge branch {} into {}",
        their_branch.name().unwrap().unwrap(),
        my_branch.name().unwrap().unwrap()
    );

    let tree_oid = repo
        .index()
        .expect("can't get index")
        .write_tree()
        .expect("can't write tree");
    let tree = repo.find_tree(tree_oid).expect("can't find tree");

    repo.commit(
        Some("HEAD"),
        &me,
        &me,
        &message,
        &tree,
        &[&my_commit, &their_commit],
    )
    .expect("cant create merge commit");
    repo.cleanup_state().expect("cant cleanup state");
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
            commit(path);
        }
        Ok((analysis, preference)) => {
            todo!("not implemented case {:?} {:?}", analysis, preference);
        }
        Err(err) => {
            panic!("error in merge_analysis {:?}", err.message());
        }
    }

    let state = repo.state();
    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());
    let ob = head_ref.peel(git2::ObjectType::Commit)?;
    let commit = ob.peel_to_commit()?;
    let branch = git2::Branch::wrap(head_ref);
    let new_head = Head::new(&branch, &commit);
    sender
        .send_blocking(crate::Event::State(State::new(state)))
        .expect("Could not send through channel");
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");
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
            if h.has_conflicts {
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

pub fn choose_conflict_side_of_hunk(
    path: PathBuf,
    file_path: PathBuf,
    hunk: Hunk,
    line: Line,
    sender: Sender<crate::Event>,
) {
    info!(
        "choose_conflict_side_of_hunk {:?} Line: {:?}",
        hunk.header, line.content
    );
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");

    // let mut current_conflict: git2::IndexConflict;

    let chosen_path = PathBuf::from(&file_path);

    let current_conflict = conflicts
        .filter(|c| c.as_ref().unwrap().get_path() == chosen_path)
        .next()
        .unwrap()
        .unwrap();
    // let current_conflict = conflicts.find(
    //     |c| c.as_ref().unwrap().get_path() == chosen_path
    // ).unwrap().unwrap();

    index
        .remove_path(chosen_path.as_path())
        .expect("cant remove path");

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");

    let mut opts = make_diff_options();
    let mut opts = opts.pathspec(&file_path).reverse(true);

    let mut git_diff = repo
        .diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
        .expect("cant get diff");

    let mut reversed_header = Hunk::reverse_header(hunk.header);

    let mut options = git2::ApplyOptions::new();

    let file_path_clone = file_path.clone();
    // so, the problem is: there could be multiple conflicts inside
    // 1 hunk. Both sides must be affected

    let mut patch = git2::Patch::from_diff(&git_diff, 0)
        .expect("cant get patch")
        .unwrap();
    let buff = patch.to_buf().expect("cant get buff");

    let raw = buff.as_str().unwrap();
    trace!("*************************************");
    for line in raw.lines() {
        trace!("{}", line);
    }

    let mut acc = Vec::new();

    let mut lines = raw.lines();
    let _first = true;
    let kind = &line.kind;
    let mut hunk_deltas: Vec<(&str, i32)> = Vec::new();

    let mut conflict_offset_inside_hunk: i32 = 0;
    for (i, l) in hunk.lines.iter().enumerate() {
        if l.content.starts_with(MARKER_OURS) {
            conflict_offset_inside_hunk = i as i32;
        }
        if l == &line {
            break;
        }
    }
    let mut line_offset_inside_hunk: i32 = -1; // first line in hunk will be 0

    // this handles all hunks, not just selected one
    while let Some(line) = lines.next() {
        if !line.is_empty() && line[1..].starts_with(MARKER_OURS) {
            // is it marker that we need?
            line_offset_inside_hunk += 1;
            let mut this_is_current_conflict = false;
            if conflict_offset_inside_hunk == line_offset_inside_hunk
                && hunk_deltas.last().unwrap().0 == reversed_header
            {
                trace!(
                    "look for offset {:?}, this offset {:?} for line {:?}",
                    conflict_offset_inside_hunk,
                    line_offset_inside_hunk,
                    line
                );
                this_is_current_conflict = true;
            }
            if this_is_current_conflict {
                // this marker will be deleted
                acc.push(line);
                acc.push("\n");
            } else {
                // do not delete it for now
                acc.push(" ");
                acc.push(&line[1..]);
                acc.push("\n");
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
                        acc.push("\n");
                    } else {
                        // do not delete it for now
                        acc.push(" ");
                        acc.push(&line[1..]);
                        acc.push("\n");
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
                                acc.push("\n");
                            } else {
                                // do not delete it for now
                                acc.push(" ");
                                acc.push(&line[1..]);
                                acc.push("\n");
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
                                if *kind == LineKind::Ours {
                                    // theirs will be deleted
                                    acc.push(line);
                                    acc.push("\n");
                                } else {
                                    // do not delete theirs!
                                    acc.push(" ");
                                    acc.push(&line[1..]);
                                    acc.push("\n");
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
                                acc.push(" ");
                                acc.push(&line[1..]);
                                acc.push("\n");
                                // delta += 1;
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
                        if *kind == LineKind::Ours {
                            // remain our lines
                            acc.push(line);
                            acc.push("\n");
                        } else {
                            // delete our lines!
                            acc.push("-");
                            acc.push(&line[1..]);
                            acc.push("\n");
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
                        acc.push("\n");
                    }
                }
            }
        } else {
            // line not belonging to conflict
            if !line.is_empty() && line[1..].contains(MARKER_HUNK) {
                hunk_deltas.push((line, 0));
                trace!(
                    "----------->reset oggset for hunk {:?} {:?}",
                    line_offset_inside_hunk,
                    line
                );
                line_offset_inside_hunk = -1;
            } else {
                trace!(
                    "increment iffset for line {:?} {:?}",
                    line_offset_inside_hunk,
                    line
                );
                line_offset_inside_hunk += 1;
            }
            acc.push(line);
            acc.push("\n");
        }
    }

    let mut new_body = acc.iter().fold("".to_string(), |cur, nxt| cur + nxt);

    trace!("xxxxxxxxxxxxxxxx deltas {:?}", &hunk_deltas);

    // so. not only new lines are changed. new_start are changed also!!!!!!
    // it need to add delta of prev hunk int new start of next hunk!!!!!!!!
    let mut prev_delta = 0;
    for (hh, delta) in hunk_deltas {
        let new_header =
            Hunk::replace_new_start_and_lines(hh, delta, prev_delta);
        new_body = new_body.replace(hh, &new_header);
        if hh == reversed_header {
            reversed_header = new_header;
        }
        prev_delta = delta;
    }
    // let new_header = Hunk::replace_new_lines(&reversed_header, delta);
    // new_body = new_body.replace(&reversed_header, &new_header);
    // reversed_header = new_header;

    trace!("+++++++++++++++++++++++++++++++++++++++++++");
    for line in new_body.lines() {
        trace!("{}", line);
    }

    git_diff = git2::Diff::from_buffer(new_body.as_bytes())
        .expect("cant create diff");

    options.hunk_callback(|odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            return header == reversed_header;
        }
        false
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: PathBuf = dd.new_file().path().unwrap().into();
            return file_path == path;
        }
        todo!("diff without delta");
    });

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut options))
        .expect("cant apply");

    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // remove from index again to restore conflict
    // and also to clear from other side tree
    index
        .remove_path(Path::new(&file_path.clone()))
        .expect("cant remove path");

    if let Some(entry) = &current_conflict.ancestor {
        index.add(entry).expect("cant add ancestor");
        debug!("ancestor restored!");
    }
    if let Some(entry) = &current_conflict.our {
        debug!("our restored!");
        index.add(entry).expect("cant add our");
    }
    if let Some(entry) = &current_conflict.their {
        debug!("their restored!");
        index.add(entry).expect("cant add their");
    }
    index.write().expect("cant write index");

    cleanup_last_conflict_for_file(path, file_path_clone, sender);
}

pub fn cleanup_last_conflict_for_file(
    path: PathBuf,
    file_path: PathBuf,
    sender: Sender<crate::Event>,
) {
    let diff = get_conflicted_v1(path.clone());
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");

    let has_conflicts = diff
        .files
        .iter()
        .flat_map(|f| &f.hunks)
        .any(|h| h.has_conflicts);
    if !has_conflicts {
        // file is clear now
        // stage it!
        index
            .remove_path(Path::new(&file_path))
            .expect("cant remove path");
        index
            .add_path(Path::new(&file_path))
            .expect("cant add path");
        index.write().expect("cant write index");
        // perhaps it will be another files with conflicts
        // perhaps not
        // it need to rerender everything
        get_current_repo_status(Some(path), sender);
        return;
    }
    sender
        .send_blocking(crate::Event::Conflicted(diff))
        .expect("Could not send through channel");
}
