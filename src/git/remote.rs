// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::git::{branch::BranchData, get_upstream, merge, DeferRefresh};
use async_channel::Sender;
use git2;
use log::{debug, trace};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

const PLAIN_PASSWORD: &str = "plain text password required";

#[derive(Debug, Default)]
pub struct RemoteResponse {
    pub body: Option<Vec<String>>,
    pub error: Option<String>,
}

impl From<git2::Error> for RemoteResponse {
    fn from(err: git2::Error) -> RemoteResponse {
        RemoteResponse {
            body: None,
            error: Some(err.message().to_string()),
        }
    }
}

impl From<String> for RemoteResponse {
    fn from(message: String) -> RemoteResponse {
        RemoteResponse {
            body: None,
            error: Some(message),
        }
    }
}
pub fn set_remote_callbacks(
    callbacks: &mut git2::RemoteCallbacks,
    user_pass: &Option<(String, String)>,
) -> Rc<RefCell<RemoteResponse>> {
    // const PLAIN_PASSWORD: &str = "plain text password required";
    callbacks.credentials({
        let user_pass = user_pass.clone();
        move |url, username_from_url, allowed_types| {
            trace!("auth credentials url {:?}", url);
            debug!("auth credentials username_from_url {:?}", username_from_url);
            trace!("auth credentials allowed_types {:?}", allowed_types);
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                let result = git2::Cred::ssh_key_from_agent(username_from_url.unwrap());
                trace!("got auth memory result. is it ok? {:?}", result.is_ok());
                return result;
            }
            if allowed_types == git2::CredentialType::USER_PASS_PLAINTEXT {
                if let Some((user_name, password)) = &user_pass {
                    return git2::Cred::userpass_plaintext(user_name, password);
                }
                return Err(git2::Error::from_str(PLAIN_PASSWORD));
            }
            todo!("implement other types");
        }
    });

    callbacks.push_transfer_progress(|s1, s2, s3| {
        debug!("push_transfer_progress {:?} {:?} {:?}", s1, s2, s3);
    });

    let mut progress_counts: HashMap<usize, usize> = HashMap::new();
    callbacks.transfer_progress(move |progress| {
        let bytes = progress.received_bytes();
        if let Some(cnt) = progress_counts.get(&bytes) {
            if cnt > &100000 {
                panic!("infinite loop in progress");
            }
            progress_counts.insert(bytes, cnt + 1);
        } else {
            progress_counts.insert(bytes, 1);
        }
        // progress_counts[] = 1;
        debug!("transfer progress {:?}", bytes);
        true
    });

    callbacks.pack_progress(|stage, s1, s2| {
        debug!("pack progress {:?} {:?} {:?}", stage, s1, s2);
    });

    let response = Rc::new(RefCell::new(RemoteResponse::default()));

    callbacks.sideband_progress({
        let r = response.clone();
        move |response| {
            let str_resp = String::from_utf8_lossy(response).into_owned();
            debug!("push.sideband progress {:?}", str_resp);
            let mut rr = r.borrow_mut();
            if let Some(body) = &mut rr.body {
                body.push(str_resp);
            } else {
                rr.body.replace(vec![str_resp]);
            }
            true
        }
    });

    callbacks.push_update_reference({
        let r = response.clone();
        move |ref_name, opt_status| {
            trace!("push update ref {:?}", ref_name);
            trace!("push status {:?}", opt_status);
            if let Some(status) = opt_status {
                let mut rr = r.borrow_mut();
                rr.error.replace(String::from(status));
                return Err(git2::Error::from_str(status));
            }
            Ok(())
        }
    });

    callbacks.certificate_check(|_cert, error| {
        debug!("cert error? {:?}", error);
        Ok(git2::CertificateCheckStatus::CertificateOk)
    });

    callbacks.push_negotiation(|update| {
        if !update.is_empty() {
            debug!(
                "push_negotiation {:?} {:?}",
                update[0].src_refname(),
                update[0].dst_refname()
            );
        }
        Ok(())
    });
    response
}

pub fn update_remote(
    path: PathBuf,
    sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>,
) -> Result<(), ()> {
    let _updater = DeferRefresh::new(path.clone(), sender, true, true);
    let repo = git2::Repository::open(path).expect("can't open repo");
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");

    let mut callbacks = git2::RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    remote
        .connect_auth(git2::Direction::Fetch, Some(callbacks), None)
        .expect("cant connect");
    let mut callbacks = git2::RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    remote.prune(Some(callbacks)).expect("cant prune");

    let mut callbacks = git2::RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);

    callbacks.update_tips({
        move |updated_ref, oid1, oid2| {
            debug!("updat tips {:?} {:?} {:?}", updated_ref, oid1, oid2);
            true
        }
    });

    let mut opts = git2::FetchOptions::new();
    opts.remote_callbacks(callbacks);
    let refs: [String; 0] = [];
    remote
        .fetch(&refs, Some(&mut opts), None)
        .expect("cant fetch");
    let mut callbacks = git2::RemoteCallbacks::new();
    set_remote_callbacks(&mut callbacks, &user_pass);
    remote
        .update_tips(Some(&mut callbacks), true, git2::AutotagOption::Auto, None)
        .expect("cant update");

    Ok(())
}

pub const REMOTE: &str = "origin";

pub fn push(
    path: PathBuf,
    remote_branch: String,
    tracking_remote: bool,
    sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>,
) -> Result<(), RemoteResponse> {
    debug!("remote branch {:?}", remote_branch);
    let repo = git2::Repository::open(path.clone())?;

    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());
    let head_ref_name = head_ref.name().ok_or("head ref has no name".to_string())?;

    let refspec = format!("{}:refs/heads/{}", head_ref_name, remote_branch);
    trace!("push. refspec {}", refspec);
    let mut branch = git2::Branch::wrap(head_ref);
    let mut remote = repo.find_remote(REMOTE)?; // TODO here is hardcode

    let mut opts = git2::PushOptions::new();
    let mut callbacks = git2::RemoteCallbacks::new();

    callbacks.update_tips({
        let remote_branch = remote_branch.clone();
        let sender = sender.clone();
        move |updated_ref, oid1, oid2| {
            debug!(
                "updated local references {:?} {:?} {:?}",
                updated_ref, oid1, oid2
            );
            if tracking_remote {
                branch
                    .set_upstream(Some(&format!("{}/{}", REMOTE, &remote_branch)))
                    .expect("cant set upstream");
            }
            sender
                .send_blocking(crate::Event::Toast(String::from(updated_ref)))
                .expect("cant send through channel");
            get_upstream(path.clone(), sender.clone()).expect("cant update tips");
            // todo what is this?
            true
        }
    });

    let response = set_remote_callbacks(&mut callbacks, &user_pass);
    opts.remote_callbacks(callbacks);

    let result = remote.push(&[refspec], Some(&mut opts));
    let mut rr = response.borrow_mut();

    // there are possibly 2 errors
    // 1. - in result, when error happened before event response
    // 2. - error during response

    match &result {
        Ok(_) => {}
        Err(error) if error.message() == PLAIN_PASSWORD => {
            // asks for password
            sender
                .send_blocking(crate::Event::PushUserPass(remote_branch, tracking_remote))
                .expect("cant send through channel");
            return Ok(());
        }
        Err(error) => {
            // error in rr - this is error from hooks.
            // it is more important
            if rr.error.is_none() {
                // push result is not ok
                rr.error.replace(error.message().to_string());
            }
        }
    }
    if let Some(error) = &rr.error {
        let mut response_result = RemoteResponse::default();
        response_result.error.replace(error.clone());
        if let Some(body) = &rr.body {
            // error containing response body
            response_result.body.replace(body.clone());
        }
        return Err(response_result);
    }
    Ok(())
}

pub fn pull(
    path: PathBuf,
    sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>,
) -> Result<(), git2::Error> {
    let defer = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path.clone())?;
    // TODO here is hardcode
    let mut remote = repo.find_remote("origin")?;
    let head_ref = repo.head()?;

    let mut opts = git2::FetchOptions::new();
    let mut callbacks = git2::RemoteCallbacks::new();

    callbacks.update_tips({
        let path = path.clone();
        let sender = sender.clone();
        move |updated_ref, oid1, oid2| {
            trace!(
                "updated local references {:?} {:?} {:?}",
                updated_ref,
                oid1,
                oid2
            );
            sender
                .send_blocking(crate::Event::Toast(String::from(updated_ref)))
                .expect("cant send through channel");
            get_upstream(path.clone(), sender.clone()).expect("cant update tips");
            true
        }
    });

    set_remote_callbacks(&mut callbacks, &user_pass);
    opts.remote_callbacks(callbacks);

    remote.fetch(&[head_ref.name().unwrap()], Some(&mut opts), None)?;

    assert!(head_ref.is_branch());
    let branch = git2::Branch::wrap(head_ref);
    let upstream = branch.upstream().unwrap();

    let branch_data = BranchData::from_branch(upstream, git2::BranchType::Remote)
        .unwrap()
        .unwrap();
    merge::branch(path.clone(), branch_data, sender.clone(), Some(defer))?;
    Ok(())
}
