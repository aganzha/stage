// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::git::{branch::BranchData, get_upstream, merge, DeferRefresh};
use anyhow::{anyhow, Result};
use async_channel::Sender;
use git2;
use log::{debug, error, trace};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

const PLAIN_PASSWORD: &str = "plain text password required";

#[derive(Debug, Default)]
pub struct RemoteResponse {
    pub body: Option<Vec<String>>,
    pub error: Option<String>,
}

impl fmt::Display for RemoteResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?} {:?}", self.error, self.body)
    }
}
impl From<git2::Error> for RemoteResponse {
    fn from(err: git2::Error) -> RemoteResponse {
        RemoteResponse {
            body: Some(vec![err.message().to_string()]),
            error: Some(format!(
                "Git2 error class {:?} code {:?}",
                err.class(),
                err.code()
            )),
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

#[derive(Debug, Clone, Default)]
pub struct Authorizer {
    login_pass: Option<crate::LoginPassword>,
}

impl Authorizer {
    pub fn new(login_pass: crate::LoginPassword) -> Self {
        Self {
            login_pass: Some(login_pass),
        }
    }

    pub fn callbacks(&self) -> git2::RemoteCallbacks {
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials({
            move |_url, username_from_url, allowed_types| {
                if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                    return git2::Cred::ssh_key_from_agent(username_from_url.unwrap());
                }
                if allowed_types == git2::CredentialType::USER_PASS_PLAINTEXT {
                    let login_pass = self.login_pass.clone().unwrap();
                    return git2::Cred::userpass_plaintext(&login_pass.login, &login_pass.password);
                }
                todo!("implement other types");
            }
        });
        callbacks
    }
}

pub fn make_authorized_remote<'a>(
    repo: &'a git2::Repository,
    remote_name: &'a str,
    direction: git2::Direction,
    received_from_user: Authorizer,
    sender: Sender<crate::Event>,
) -> Result<(git2::Remote<'a>, Authorizer), RemoteResponse> {
    let mut callbacks = git2::RemoteCallbacks::new();

    let stored: Rc<RefCell<Authorizer>> = Rc::new(RefCell::new(received_from_user.clone()));

    callbacks.credentials({
        let sender = sender.clone();
        let stored = stored.clone();
        move |_url, username_from_url, allowed_types| {
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                let result = git2::Cred::ssh_key_from_agent(username_from_url.unwrap());
                return result;
            }
            if allowed_types == git2::CredentialType::USER_PASS_PLAINTEXT {
                if let Some(login_pass) = &received_from_user.login_pass {
                    let plain_result =
                        git2::Cred::userpass_plaintext(&login_pass.login, &login_pass.password);
                    return plain_result;
                } else {
                    let auth_request =
                        Arc::new((Mutex::new(crate::LoginPassword::default()), Condvar::new()));
                    let ui_auth_request = auth_request.clone();
                    sender
                        .send_blocking(crate::Event::UserInputRequired(ui_auth_request))
                        .expect("cant send through channel");

                    let mut login_pass = auth_request.0.lock().unwrap();

                    while login_pass.pending {
                        login_pass = auth_request.1.wait(login_pass).unwrap();
                    }

                    if login_pass.cancel {
                        return Err(git2::Error::from_str(PLAIN_PASSWORD));
                    }
                    stored.replace(Authorizer::new(login_pass.clone()));
                    let plain_result =
                        git2::Cred::userpass_plaintext(&login_pass.login, &login_pass.password);
                    return plain_result;
                }
            }
            todo!("implement other types");
        }
    });

    let mut remote = repo.find_remote(remote_name).unwrap();
    if let Err(error) = remote.connect_auth(direction, Some(callbacks), None) {
        if error.message() != PLAIN_PASSWORD {
            // "unknown ssl error" and valid login_pass was already provided
            // by user. looks like it happens when some time pass while user
            // enters credentials. THis error does not occur if provide credentials
            // immidiatelly, so thats annother attempt to authorize with same
            // credentials
            if stored.borrow().login_pass.is_some() {
                return make_authorized_remote(
                    repo,
                    remote_name,
                    direction,
                    stored.borrow().clone(),
                    sender,
                );
            }
        }
        return Err(error.into());
    }
    let authorizer = (*stored.borrow()).clone();
    Ok((remote, authorizer))
}

pub fn set_remote_callbacks(callbacks: &mut git2::RemoteCallbacks) -> Rc<RefCell<RemoteResponse>> {
    callbacks.push_transfer_progress(|s1, s2, s3| {
        debug!("push_transfer_progress {:?} {:?} {:?}", s1, s2, s3);
    });

    let mut progress_counts: HashMap<usize, usize> = HashMap::new();
    callbacks.transfer_progress(move |progress| {
        let bytes = progress.received_bytes();
        if let Some(cnt) = progress_counts.get(&bytes) {
            progress_counts.insert(bytes, cnt + 1);
        } else {
            progress_counts.insert(bytes, 1);
        }
        // progress_counts[] = 1;
        trace!("transfer progress {:?}", bytes);
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
        debug!("certificate_check. error? {:?}", error);
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

pub fn update_remote(path: PathBuf, sender: Sender<crate::Event>) -> Result<(), git2::Error> {
    let _updater = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path)?;
    let mut errors: HashMap<&str, Vec<anyhow::Error>> = HashMap::new();

    let remotes = repo.remotes()?;
    for remote_name in &remotes {
        let remote_name = remote_name.unwrap();
        let sender = sender.clone();
        match make_authorized_remote(
            &repo,
            remote_name,
            git2::Direction::Fetch,
            Authorizer::default(),
            sender.clone(),
        ) {
            Ok((mut remote, authorizer)) => {
                let mut callbacks = authorizer.callbacks();
                set_remote_callbacks(&mut callbacks);
                if let Err(err) = remote.prune(Some(callbacks)) {
                    errors.entry(remote_name).or_default().push(err.into());
                    continue;
                }
                let mut opts = git2::FetchOptions::new();
                let mut callbacks = authorizer.callbacks();
                set_remote_callbacks(&mut callbacks);
                opts.remote_callbacks(callbacks);
                let refs: [String; 0] = [];
                if let Err(err) = remote.fetch(&refs, Some(&mut opts), None) {
                    errors.entry(remote_name).or_default().push(err.into());
                    continue;
                }
            }
            Err(err) => {
                errors
                    .entry(remote_name)
                    .or_default()
                    .push(anyhow!(err.to_string()));
                continue;
            }
        }
    }
    if !errors.is_empty() {
        let mut message = String::new();
        for (k, v) in &errors {
            message.push_str(&format!("Errors for remote {:}\n", k));
            for err in v {
                message.push_str(&format!("{}\n", err));
            }
            message.push('\n');
        }
        return Err(git2::Error::from_str(&message));
    }
    Ok(())
}

pub fn push(
    path: PathBuf,
    remote_name: String,
    remote_ref: String,
    tracking_remote: bool,
    is_tag: bool,
    sender: Sender<crate::Event>,
) -> Result<(), RemoteResponse> {
    let repo = git2::Repository::open(path.clone())?;

    let head_ref = repo.head()?;
    assert!(head_ref.is_branch());
    let head_ref_name = head_ref.name().ok_or("head ref has no name".to_string())?;

    let mut refspec = format!("{}:refs/heads/{}", head_ref_name, remote_ref);
    if is_tag {
        refspec = format!("refs/tags/{}:refs/tags/{}", remote_ref, remote_ref);
    }

    trace!("push. refspec {}", refspec);
    let mut branch = git2::Branch::wrap(head_ref);

    let (mut remote, authorizer) = make_authorized_remote(
        &repo,
        &remote_name,
        git2::Direction::Push,
        Authorizer::default(),
        sender.clone(),
    )?;

    let mut opts = git2::PushOptions::new();

    let mut callbacks = authorizer.callbacks();

    callbacks.update_tips({
        let remote_ref = remote_ref.clone();
        let sender = sender.clone();
        let remote_name = remote_name.clone();
        move |updated_ref, oid1, oid2| {
            debug!(
                "updated local references {:?} {:?} {:?}",
                updated_ref, oid1, oid2
            );
            if tracking_remote {
                let refspec = format!("{}/{}", remote_name, &remote_ref);
                branch
                    .set_upstream(Some(&refspec))
                    .expect("cant set upstream");
            }
            sender
                .send_blocking(crate::Event::Toast(String::from(updated_ref)))
                .expect("cant send through channel");
            match get_upstream(path.clone()) {
                Ok(head) => {
                    sender
                        .send_blocking(crate::Event::Upstream(Some(head)))
                        .expect("Could not send through channel");
                }
                Err(err) => {
                    error!("cant get Upstream {:?}", err);
                    sender
                        .send_blocking(crate::Event::Upstream(None))
                        .expect("Could not send through channel");
                }
            }
            // todo what is this?
            true
        }
    });

    let response = set_remote_callbacks(&mut callbacks);
    opts.remote_callbacks(callbacks);

    let result = remote.push(&[refspec], Some(&mut opts));
    let mut rr = response.borrow_mut();

    // there are possibly 2 errors
    // 1. - in result, when error happened before event response
    // 2. - error during response

    match &result {
        Ok(_) => {}
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

pub fn pull(path: PathBuf, sender: Sender<crate::Event>) -> Result<(), RemoteResponse> {
    let defer = DeferRefresh::new(path.clone(), sender.clone(), true, true);
    let repo = git2::Repository::open(path.clone())?;

    let head_ref = repo.head()?;
    let branch = git2::Branch::wrap(head_ref);
    let err = "No remote to pull from";
    let branch_data = BranchData::from_branch(&branch, git2::BranchType::Local)?
        .ok_or(git2::Error::from_str(err))?;

    let remote_name = branch_data
        .remote_name
        .clone()
        .ok_or(git2::Error::from_str(err))?;

    // let mut remote = repo.find_remote(&remote_name)?;
    let (mut remote, authorizer) = make_authorized_remote(
        &repo,
        &remote_name,
        git2::Direction::Fetch,
        Authorizer::default(),
        sender.clone(),
    )?;

    let mut opts = git2::FetchOptions::new();
    let mut callbacks = authorizer.callbacks();

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
            match get_upstream(path.clone()) {
                Ok(head) => {
                    sender
                        .send_blocking(crate::Event::Upstream(Some(head)))
                        .expect("Could not send through channel");
                }
                Err(err) => {
                    error!("cant get Upstream {:?}", err);
                    sender
                        .send_blocking(crate::Event::Upstream(None))
                        .expect("Could not send through channel");
                }
            }
            true
        }
    });

    set_remote_callbacks(&mut callbacks);
    opts.remote_callbacks(callbacks);

    remote.fetch(&[&branch_data.local_name()], Some(&mut opts), None)?;

    let upstream = branch.upstream()?;

    let branch_data = BranchData::from_branch(&upstream, git2::BranchType::Remote)
        .unwrap()
        .unwrap();
    merge::branch(
        path.clone(),
        branch_data,
        false,
        sender.clone(),
        Some(defer),
    )?;
    Ok(())
}

#[derive(Debug, Default, Clone)]
pub struct RemoteDetail {
    pub name: String,
    pub url: String,
    // pub push_url: String,
    pub refspecs: Vec<String>,
    // pub push_refspecs: Vec<String>,
}

impl From<git2::Remote<'_>> for RemoteDetail {
    fn from(remote: git2::Remote) -> RemoteDetail {
        let mut rd = RemoteDetail::default();
        if let Some(name) = remote.name() {
            rd.name = name.to_string();
        }
        if let Some(url) = remote.url() {
            rd.url = url.to_string();
        }
        // if let Some(url) = remote.pushurl() {
        //     rd.push_url = url.to_string();
        // }
        for r in remote.refspecs() {
            if let Some(refspec) = r.str() {
                rd.refspecs.push(refspec.to_string());
            }
        }
        // if let Ok(refspecs) = remote.push_refspecs() {
        //     for pr in &refspecs {
        //         if let Some(refspec) = pr {
        //             rd.push_refspecs.push(refspec.to_string());
        //         }
        //     }
        // }
        rd
    }
}

pub fn list(path: PathBuf) -> Result<Vec<RemoteDetail>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let mut remotes: Vec<RemoteDetail> = Vec::new();
    for remote_name in (&repo.remotes()?).into_iter().flatten() {
        let remote = repo.find_remote(remote_name)?;
        remotes.push(remote.into());
    }
    Ok(remotes)
}

pub fn add(path: PathBuf, name: String, url: String) -> Result<Option<RemoteDetail>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let remote = repo.remote(&name, &url)?;
    Ok(Some(remote.into()))
}

pub fn delete(path: PathBuf, name: String) -> Result<bool, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    repo.remote_delete(&name)?;
    Ok(true)
}

pub fn edit(
    path: PathBuf,
    name: String,
    remote: RemoteDetail,
) -> Result<Option<RemoteDetail>, git2::Error> {
    let repo = git2::Repository::open(path.clone())?;
    let git_remote = repo.find_remote(&name)?;
    if let Some(name) = git_remote.name() {
        if name != remote.name {
            repo.remote_rename(name, &remote.name)?;
            return Ok(Some(repo.find_remote(&remote.name)?.into()));
        }
        if let Some(url) = git_remote.url() {
            if url != remote.url {
                repo.remote_set_url(name, &remote.url)?;
                return Ok(Some(repo.find_remote(&remote.name)?.into()));
            }
        }
    }
    Ok(None)
}
