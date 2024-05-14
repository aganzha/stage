use gtk4::{gio};
use std::{ffi::OsString, collections::HashSet, str::from_utf8, path::{PathBuf, Path}};
use crate::git::{BranchData, Head, State, get_current_repo_status, STAGE_FLAG, make_diff, Hunk, DiffKind, get_conflicted_v1, Line, LineKind};
use async_channel::Sender;
use log::{debug, info, trace};
use git2;

#[derive(Debug, Clone)]
pub enum MergeError {
    Conflicts,
    General(String)
}

pub fn commit(path: OsString) {
    let mut repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let me = repo.signature().expect("can't get signature");

    let my_oid = repo
        .revparse_single("HEAD^{commit}")
        .expect("fail revparse")
        .id();

    let mut their_oid: Option<git2::Oid> = None;
    repo.mergehead_foreach(|oid_ref| -> bool {
        their_oid.replace(*oid_ref);
        true
    }).expect("cant get merge heads");

    let their_oid = their_oid.unwrap();
    info!("creating merge commit for {:?} {:?}", my_oid, their_oid);

    let my_commit = repo.find_commit(my_oid).expect("cant get commit");
    let their_commit = repo.find_commit(their_oid)
        .expect("cant get commit");

    // let message = message.unwrap_or(repo.message().expect("cant get merge message"));

    let mut their_branch: Option<git2::Branch> = None;
    let refs = repo.references().expect("no refs");
    for r in refs.into_iter() {
        if let Ok(r) = r {
            if let Some(oid) = r.target() {
                if oid == their_oid {
                    their_branch.replace(git2::Branch::wrap(r));
                }
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
        &[&my_commit, &their_commit]
    ).expect("cant create merge commit");
    repo.cleanup_state().expect("cant cleanup state");
}

pub fn branch(
    path: OsString,
    branch_data: BranchData,
    sender: Sender<crate::Event>,
) -> Result<BranchData, MergeError> {
    info!("merging {:?}", branch_data.name);
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let annotated_commit = repo
        .find_annotated_commit(branch_data.oid)
        .expect("cant find commit");

    match repo.merge_analysis(&[&annotated_commit]) {
        Ok((analysis, _)) if analysis.is_up_to_date() => {
            info!("merge.uptodate");
        }

        Ok((analysis, preference))
            if analysis.is_fast_forward()
                && !preference.is_no_fast_forward() =>
        {
            info!("merge.fastforward");
            let ob = repo.find_object(branch_data.oid, Some(git2::ObjectType::Commit))
                .expect("cant find ob for oid");
            repo.checkout_tree(&ob, Some(git2::build::CheckoutBuilder::new().safe())).expect("cant checkout tree");
            repo.reset(&ob, git2::ResetType::Soft, None)
                .expect("cant reset to commit");
        }
        Ok((analysis, preference))
            if analysis.is_normal() && !preference.is_fastforward_only() =>
        {

            info!("merge.normal");
            if let Err(error) = repo.merge(&[&annotated_commit], None, None) {
                return Err(MergeError::General(String::from(error.message())));
            }
            let index = repo.index().expect("cant get index");
            if index.has_conflicts() {
                gio::spawn_blocking({
                    let sender = sender.clone();
                    let path = path.clone();
                    move || {
                        get_current_repo_status(Some(path), sender.clone());
                    }
                });
                return Err(MergeError::Conflicts);
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
    let head_ref = repo.head().expect("can't get head");
    assert!(head_ref.is_branch());
    let ob = head_ref
        .peel(git2::ObjectType::Commit)
        .expect("can't get commit from ref!");
    let commit = ob.peel_to_commit().expect("can't get commit from ob!");
    let branch = git2::Branch::wrap(head_ref);
    let new_head = Head::new(&branch, &commit);
    sender
        .send_blocking(crate::Event::State(State::new(state)))
        .expect("Could not send through channel");
    sender
        .send_blocking(crate::Event::Head(new_head))
        .expect("Could not send through channel");

    Ok(BranchData::from_branch(branch, git2::BranchType::Local)
        .expect("cant get branch"))
}


pub fn abort(path: OsString, sender: Sender<crate::Event>) {
    info!("git.abort merge");

    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut checkout_builder = git2::build::CheckoutBuilder::new();

    let index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut has_conflicts = false;
    for conflict in conflicts {
        if let Ok(conflict) = conflict {
            if let Some(our) = conflict.our {
                checkout_builder.path(our.path);
                has_conflicts = true;
            }
        }
    }
    if !has_conflicts {
        panic!("no way to abort merge without conflicts");
    }
    let head_ref = repo.head().expect("can't get head");

    let ob = head_ref
        .peel(git2::ObjectType::Commit)
        .expect("can't get commit from ref!");

    repo.reset(&ob, git2::ResetType::Hard, Some(&mut checkout_builder))
        .expect("cant reset hard");

    get_current_repo_status(Some(path), sender);
}

pub fn choose_conflict_side(path: OsString, ours: bool, sender: Sender<crate::Event>) {
    info!("git.choose side");
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut entries: Vec<git2::IndexEntry> = Vec::new();
    for conflict in conflicts {
        if let Ok(conflict) = conflict {
            if ours {
                if let Some(our) = conflict.our {
                    entries.push(our);
                }
            } else {
                if let Some(their) = conflict.their {
                    entries.push(their);
                }
            }
        }
    }
    if entries.is_empty() {
        panic!("nothing to resolve in choose_conflict_side");
    }

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.reverse(true);


    for entry in &mut entries {
        let pth = String::from_utf8(entry.path.clone()).expect("cant get path");
        diff_opts.pathspec(pth.clone());
        index.remove_path(Path::new(&pth)).expect("cant remove path");
        entry.flags = entry.flags & !STAGE_FLAG;
        index.add(&entry).expect("cant add to index");
    }
    index.write().expect("cant write index");
    let git_diff = repo.diff_index_to_workdir(Some(&index), Some(&mut diff_opts))
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

    sender.send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");


    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut apply_opts))
        .expect("can't apply patch");

    sender.send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // if their side is choosen, it need to stage all conflicted paths
    // because resolved conflicts will go to staged area, but other changes
    // will be on other side of stage (will be +- same hunks on both sides)
    for entry in &entries {
        let pth = String::from_utf8(entry.path.clone()).expect("cant get path");
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
        PathBuf::from(from_utf8(match (&self.our, &self.their) {
            (Some(o), _) => {
                &o.path[..]
            }
            (_, Some(t)) => {
                &t.path[..]
            }
            _ => panic!("no path")
        }).unwrap())
    }
}

pub fn choose_conflict_side_of_hunk(
    path: OsString,
    file_path: OsString,
    hunk: Hunk,
    line: Line,
    sender: Sender<crate::Event>,
) {
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");

    // let mut current_conflict: git2::IndexConflict;

    let chosen_path = PathBuf::from(&file_path);

    let mut current_conflict = conflicts.filter(|c| c.as_ref().unwrap().get_path() == chosen_path)
        .next()
        .unwrap()
        .unwrap();

    index.remove_path(chosen_path.as_path()).expect("cant remove path");

    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");

    let mut opts = git2::DiffOptions::new();
    let mut opts = opts.pathspec(&file_path).reverse(true);
    let mut git_diff = repo.diff_tree_to_workdir(
        Some(&current_tree),
        Some(&mut opts)
    ).expect("cant get diff");


    let reversed_header = Hunk::reverse_header(hunk.header);    

    let mut options = git2::ApplyOptions::new();
    
    if line.kind == LineKind::Ours {
        // just kill all hunk from diff.
        // our tree is all that required
        options.hunk_callback(|odh| -> bool {
            if let Some(dh) = odh {
                let header = Hunk::get_header_from(&dh);
                return header == reversed_header
            }
            false
        });
        options.delta_callback(|odd| -> bool {
            if let Some(dd) = odd {
                let path: OsString = dd.new_file().path().unwrap().into();
                return file_path == path;
            }
            todo!("diff without delta");
        });

    } else {
        let their_entry = current_conflict.their.as_mut().unwrap();
        let their_original_flags = their_entry.flags;

        their_entry.flags = their_entry.flags & !STAGE_FLAG;

        index.add(their_entry).expect("cant add entry");

        let mut opts = git2::DiffOptions::new();
        opts.pathspec(file_path.clone());
        // reverse means index will be NEW side cause we are adding hunk to workdir
        opts.reverse(true);

        // ANOTHER DIFF!
        git_diff = repo.diff_index_to_workdir(Some(&index), Some(&mut opts))
            .expect("cant get diff");

        // restore stage flag to conflict again
        their_entry.flags = their_original_flags;


        // passed hunk is from diff_tree_to_workdir. workdir is NEW side
        // for this hunks NEW side is workdir
        // so it need to compare NEW side of passed hunk with OLD side of this diff
        // (cause new side is index side, where hunk headers will differ a lot)
        options.hunk_callback(|odh| -> bool {
            if let Some(dh) = odh {
                return hunk.new_start == dh.old_start();
            }
            false
        });
        options.delta_callback(|odd| -> bool {
            if let Some(dd) = odd {
                let path: OsString = dd.new_file().path().unwrap().into();
                return file_path == path;
            }
            todo!("diff without delta");
        });
    }


    sender.send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut options))
        .expect("can't apply patch");

    sender.send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // remove from index again to restore conflict
    // and also to clear from other side tree
    index.remove_path(Path::new(&file_path)).expect("cant remove path");

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
    
    let diff = get_conflicted_v1(path.clone());
    let has_conflicts = diff.files[0].hunks.iter().fold(false, |a, h| {
        a || h.has_conflicts
    });

    if has_conflicts {
        sender
            .send_blocking(crate::Event::Conflicted(diff))
            .expect("Could not send through channel");
        return;
    }

    // cleanup conflicts and show banner
    index.remove_path(Path::new(&file_path)).expect("cant remove path");
    if let Some(mut entry) = current_conflict.ancestor {
        debug!("ancestor replaced!");
        entry.flags = entry.flags & !STAGE_FLAG;
        index.add(&entry).expect("cant add ancestor");
    }
    if let Some(mut entry) = current_conflict.our {
        debug!("our replaced!");
        entry.flags = entry.flags & !STAGE_FLAG;
        index.add(&entry).expect("cant add our");
    }
    if let Some(mut entry) = current_conflict.their {
        debug!("their replaced!");
        entry.flags = entry.flags & !STAGE_FLAG;
        index.add(&entry).expect("cant add their");
    }
    index.write().expect("cant write index");
    get_current_repo_status(Some(path), sender);

}

pub fn choose_conflict_side_once(
    path: OsString,
    file_path: OsString,
    hunk_header: String,
    origin: git2::DiffLineType,
    sender: Sender<crate::Event>,
) {
    info!("choose_conflict_side_once");
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let index = repo.index().expect("cant get index");
    let conflicts = index.conflicts().expect("no conflicts");
    let mut opts = git2::DiffOptions::new();
    let mut current_conflict: Option<git2::IndexConflict> = None;
    for conflict in conflicts {
        if let Ok(conflict) = conflict {
            if let Some(ref our) = conflict.our {
                if file_path.to_str().unwrap() == String::from_utf8(our.path.clone()).unwrap() {
                    current_conflict.replace(conflict);
                }
            }
        }
    }
    let mut current_conflict = current_conflict.unwrap();
    let mut index = repo.index().expect("cant get index");

    // vv --------------------------------------------------------
    // delete whole conflict hunk and store lines which user
    // choosed to later apply them
    debug!(".........START removing conflicted file from index");
    index.remove_path(Path::new(&file_path)).expect("cant remove path");

    opts.pathspec(file_path.clone());
    opts.reverse(true);
    let ob = repo.revparse_single("HEAD^{tree}").expect("fail revparse");
    let current_tree = repo.find_tree(ob.id()).expect("no working tree");
    let git_diff = repo.diff_tree_to_workdir(Some(&current_tree), Some(&mut opts))
        .expect("cant get diff");

    let reversed_header = Hunk::reverse_header(hunk_header.clone());
    debug!(".........reverse header to apply to workdir to delete {:?}", &reversed_header);

    // ~~~~~~~~~~~~~~~~~~ store choosed lines ~~~~~~~~~~~~~~~~
    // lets store hunk lines, which will be removed from diff
    // TODO! replace it with new collecting procedure fro conflicting hunk!!!!!!!!!!!!
    let mut choosed_lines = String::from("");
    let mut collect: bool = false;
    git_diff.foreach(
        &mut |_delta: git2::DiffDelta, _num| { // file cb
            true
        },
        None, // binary cb
        None, // hunk cb
        Some(&mut |_delta: git2::DiffDelta, odh: Option<git2::DiffHunk>, dl: git2::DiffLine| {
            if let Some(dh) = odh {
                let header = Hunk::get_header_from(&dh);
                if header == reversed_header {
                    let content = String::from(
                        from_utf8(dl.content()).unwrap()
                    ).replace("\r\n", "").replace('\n', "");
                    debug!(".........collect {:?} and line in comparison: {:?}", collect, &content);
                    if content.len() >= 3 {
                        match &content[..3] {
                            "<<<" => {
                                debug!("..start collecting OUR SIDE");
                                // collect = true;
                            },
                            "===" => {
                                debug!("..stop collecting OR start collecting THEIR side");
                                // collect = false;
                                collect = true;
                            }
                            ">>>" => {
                                debug!("..stop collecting");
                                collect = false;
                            }
                            _ => {
                                if collect {
                                    debug!("collect this line!");
                                    choosed_lines.push_str(&content);
                                }
                            }
                        }
                    } else {
                        if collect {
                            debug!("collect this line!");
                            choosed_lines.push_str(&content);
                        }
                    }
                }
            }
            true
        })
    ).expect("cant iter on diff");
    debug!("~~~~~~~ choosed lines before delete: {}", choosed_lines);
    // ~~~~~~~~~~~~~~~~~~ store choosed lines ~~~~~~~~~~~~~~~~


    let mut options = git2::ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            debug!("--apply delete patch. hunk callback {:?} {:?} == {:?}", header, Hunk::reverse_header(hunk_header.clone()), header == Hunk::reverse_header(hunk_header.clone()));
            return header == reversed_header
        }
        false
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: OsString = dd.new_file().path().unwrap().into();
            debug!("--apply delete patch. delta callback {:?} {:?} {:?}", file_path, path, file_path == path);
            return file_path == path;
        }
        todo!("diff without delta");
    });


    sender.send_blocking(crate::Event::LockMonitors(true))
        .expect("Could not send through channel");

    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut options))
        .expect("can't apply patch");
    // ^^ -----------------------------------------------------
    debug!("..... conflict removed from workdir");


    // NOW. if user choosed our side, this means NOTHING
    // else todo with this current conflict. changes already were
    // reverted to our side! next part is valid only if chooser
    // used THEIR side! are you sure? yes. conflicts are removed
    // and workdir is restored according to the our tree (HEAD in this branch)!


    // vv --------------------------- apply hunk from choosed side

    // so. it is not possible to find tree from blob.
    // lets put this blob to index, maybe?
    let their_entry = current_conflict.their.as_mut().unwrap();
    let their_original_flags = their_entry.flags;

    debug!(">>>>> flags before {:?}", their_original_flags);
    their_entry.flags = their_entry.flags & !STAGE_FLAG;
    debug!(">>>>> flags after mask {:?}", their_entry.flags);

    index.add(their_entry).expect("cant add entry");
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path.clone());
    // reverse means index will be NEW side cause we are adding hunk to workdir
    opts.reverse(true);
    let git_diff = repo.diff_index_to_workdir(Some(&index), Some(&mut opts))
        .expect("cant get diff");

    // restore stage flag to conflict again
    their_entry.flags = their_original_flags;
    debug!(">>>>> flags after restore {:?}", their_entry.flags);

    // vv ~~~~~~~~~~~~~~~~ select hunk header for choosed lines
    let mut hunk_header_to_apply = String::from("");
    let mut current_hunk_header = String::from("");
    let mut found_lines = String::from("");

    debug!("..... choosing hunk to apply for choosed lines");

    let result = git_diff.foreach(
        &mut |_delta: git2::DiffDelta, _num| { // file cb
            true
        },
        None, // binary cb
        None, // hunk cb
        Some(&mut |_delta: git2::DiffDelta, odh: Option<git2::DiffHunk>, dl: git2::DiffLine| {
            if let Some(dh) = odh {
                if !hunk_header_to_apply.is_empty() {
                    // all done
                    debug!("+++ all good. return");
                    return false;
                }
                let header = Hunk::get_header_from(&dh);
                if header != current_hunk_header {
                    // handle next header (or first one)
                    debug!("++++ thats new hunk header and current one {:?} {:?}", header, current_hunk_header);
                    if found_lines == choosed_lines {
                        debug!("!!!!!!!!!!!!!!!!!! match!");
                        hunk_header_to_apply = current_hunk_header.clone();
                        // all done
                        debug!("allllllllllllll done");
                        return false;
                    }
                    debug!("+++ reset found lines");
                    current_hunk_header = header;
                    found_lines = String::from("");
                }
                if dl.origin_value() == origin {
                    let content = String::from(
                        from_utf8(dl.content()).unwrap()
                    ).replace("\r\n", "").replace('\n', "");
                    found_lines.push_str(&content);
                    debug!("++++ thats current line and total found lines {:?} {:?}", &content, &found_lines)
                }
            }
            true
        })
    );
    if result.is_ok() {
        // handle case when choosed hunk is last one
        assert!(hunk_header_to_apply.is_empty());
        debug!("++++ outside of loop. found lines {:?}", found_lines);
        if found_lines == choosed_lines {
            hunk_header_to_apply = current_hunk_header;
        }
    }
    if hunk_header_to_apply.is_empty() {
        panic!("cant find header for choosed_lines {:?}", choosed_lines);
    }
    // ^^ ~~~~~~~~~~~~~~~~ select hunk header for choosed lines

    let mut options = git2::ApplyOptions::new();

    options.hunk_callback(|odh| -> bool {
        if let Some(dh) = odh {
            let header = Hunk::get_header_from(&dh);
            debug!("**** apply hunk callback {:?} {:?} == {:?}", header, hunk_header_to_apply, header == hunk_header_to_apply);
            return header == hunk_header_to_apply
        }
        false
    });
    options.delta_callback(|odd| -> bool {
        if let Some(dd) = odd {
            let path: OsString = dd.new_file().path().unwrap().into();
            debug!("**** apply delta callback {:?} {:?} {:?}", file_path, path, file_path == path);
            return file_path == path;
        }
        todo!("diff without delta");
    });
    repo.apply(&git_diff, git2::ApplyLocation::WorkDir, Some(&mut options))
        .expect("can't apply patch");
    // ^^ ----------------------------


    sender.send_blocking(crate::Event::LockMonitors(false))
        .expect("Could not send through channel");

    // remove from index again to restore conflict
    index.remove_path(Path::new(&file_path)).expect("cant remove path");

    // ------------------------------------------
    // restore conflict file in index if not all conflicts were resolved
    if let Some(entry) = current_conflict.ancestor {
        index.add(&entry).expect("cant add ancestor");
        debug!("ancestor added!");
    }
    if let Some(entry) = current_conflict.our {
        debug!("our added!");
        index.add(&entry).expect("cant add our");
    }
    if let Some(entry) = current_conflict.their {
        debug!("their added!");
        index.add(&entry).expect("cant add their");
    }
    index.write().expect("cant write index");
    // ^^ -------------------------------------------    
    let diff = get_conflicted_v1(path);
    sender
        .send_blocking(crate::Event::Conflicted(diff))
        .expect("Could not send through channel");
}
