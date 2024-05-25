use log::{trace, debug};
use std::path::PathBuf;
use async_channel::Sender;
use std::collections::HashMap;
use git2;
use std::rc::Rc;
use std::cell::RefCell;
use crate::git::{get_head, get_upstream};

const PLAIN_PASSWORD: &str = "plain text password required";

#[derive(Debug, Default)]
pub struct RemoteResponse {
    pub body: Option<Vec<String>>,
    pub error: Option<String>
}

pub fn set_remote_callbacks(
    callbacks: &mut git2::RemoteCallbacks,
    user_pass: &Option<(String, String)>,
) -> Rc<RefCell<RemoteResponse>> {
    // const PLAIN_PASSWORD: &str = "plain text password required";
    callbacks.credentials({
        let user_pass = user_pass.clone();
        move |url, username_from_url, allowed_types| {
            debug!("auth credentials url {:?}", url);
            // "git@github.com:aganzha/stage.git"
            debug!(
                "auth credentials username_from_url {:?}",
                username_from_url
            );
            debug!("auth credentials allowed_types {:?}", allowed_types);
            if allowed_types.contains(git2::CredentialType::SSH_KEY) {
                let result =
                    git2::Cred::ssh_key_from_agent(username_from_url.unwrap());
                debug!(
                    "got auth memory result. is it ok? {:?}",
                    result.is_ok()
                );
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
            debug!(
                "push.sideband progress {:?}",
                str_resp
            );
            let mut rr = r.borrow_mut();
            if let Some(body) = &mut rr.body {
                body.push(str_resp);
            } else {
                let mut body = Vec::new();
                body.push(str_resp);
                rr.body.replace(body);
            }            
            true
        }});

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
    _sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>,
) -> Result<(), ()> {
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

pub fn push(
    path: PathBuf,
    remote_branch: String,
    tracking_remote: bool,
    sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>,
) -> Result<(), RemoteResponse> { 
    trace!("remote branch {:?}", remote_branch);
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let head_ref = repo.head().expect("can't get head");
    trace!("push.head ref name {:?}", head_ref.name());
    assert!(head_ref.is_branch());
    let refspec = format!(
        "{}:refs/heads/{}",
        head_ref.name().unwrap(),
        remote_branch.replace("origin/", "")
    );
    trace!("push. refspec {}", refspec);
    let mut branch = git2::Branch::wrap(head_ref);
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");

    let mut opts = git2::PushOptions::new();
    let mut callbacks = git2::RemoteCallbacks::new();

    callbacks.update_tips({
        let remote_branch = remote_branch.clone();
        let sender = sender.clone();
        move |updated_ref, oid1, oid2| {
            trace!(
                "updated local references {:?} {:?} {:?}",
                updated_ref, oid1, oid2
            );
            if tracking_remote {
                branch
                    .set_upstream(Some(&remote_branch))
                    .expect("cant set upstream");
            }
            sender
                .send_blocking(crate::Event::Toast(String::from(updated_ref)))
                .expect("cant send through channel");
            get_upstream(path.clone(), sender.clone());
            // todo what is this?
            true
        }
    });

    let response = set_remote_callbacks(&mut callbacks, &user_pass);
    opts.remote_callbacks(callbacks);

    let result = remote.push(&[refspec], Some(&mut opts));

    match &result {
        Ok(_) => {
            sender
                .send_blocking(crate::Event::Toast(String::from(
                    "Pushed to remote",
                )))
                .expect("cant send through channel");
        }
        Err(error) if error.message() == PLAIN_PASSWORD => {
            sender
                .send_blocking(crate::Event::PushUserPass(
                    remote_branch,
                    tracking_remote,
                ))
                .expect("cant send through channel");
            return Ok(());
        }
        _ => {}
    }

    let rr = response.borrow();
    if let Some(error) = &rr.error {
        let mut result = RemoteResponse::default();
        result.error.replace(error.clone());
        if let Some(body) = &rr.body {
            result.body.replace(body.clone());
        }
        return Err(result);
    }
    Ok(())
}

pub fn pull(
    path: PathBuf,
    sender: Sender<crate::Event>,
    user_pass: Option<(String, String)>,
) {
    let repo = git2::Repository::open(path.clone()).expect("can't open repo");
    let mut remote = repo
        .find_remote("origin") // TODO here is hardcode
        .expect("no remote");
    let head_ref = repo.head().expect("can't get head");

    let mut opts = git2::FetchOptions::new();
    let mut callbacks = git2::RemoteCallbacks::new();

    callbacks.update_tips({
        let path = path.clone();
        let sender = sender.clone();
        move |updated_ref, oid1, oid2| {
            debug!(
                "updated local references {:?} {:?} {:?}",
                updated_ref, oid1, oid2
            );
            sender
                .send_blocking(crate::Event::Toast(String::from(updated_ref)))
                .expect("cant send through channel");
            get_upstream(path.clone(), sender.clone());
            // todo what is this?
            true
        }
    });

    set_remote_callbacks(&mut callbacks, &user_pass);
    opts.remote_callbacks(callbacks);

    remote
        .fetch(&[head_ref.name().unwrap()], Some(&mut opts), None)
        .expect("cant fetch");

    assert!(head_ref.is_branch());
    let branch = git2::Branch::wrap(head_ref);
    let upstream = branch.upstream().unwrap();

    let u_oid = upstream.get().target().unwrap();
    let mut head_ref = repo.head().expect("can't get head");
    let log_message = format!(
        "(HEAD -> {}, {}) HEAD@{0}: pull: Fast-forward",
        branch.name().unwrap().unwrap(),
        upstream.name().unwrap().unwrap()
    );

    // think about it! perhaps it need to call merge analysys
    // during pull! if its fast formard - ok. if not - do merge, please.
    // see what git suggests:
    // Pulling without specifying how to reconcile divergent branches is
    // discouraged. You can squelch this message by running one of the following
    // commands sometime before your next pull:

    //   git config pull.rebase false  # merge (the default strategy)
    //   git config pull.rebase true   # rebase
    //   git config pull.ff only       # fast-forward only

    // You can replace "git config" with "git config --global" to set a default
    // preference for all repositories. You can also pass --rebase, --no-rebase,
    // or --ff-only on the command line to override the configured default per
    // invocation.
    let mut builder = git2::build::CheckoutBuilder::new();
    let opts = builder.safe();
    let commit = repo.find_commit(u_oid).expect("can't find commit");

    sender
        .send_blocking(crate::Event::LockMonitors(true))
        .expect("can send through channel");
    let result = repo.checkout_tree(commit.as_object(), Some(opts));
    sender
        .send_blocking(crate::Event::LockMonitors(false))
        .expect("can send through channel");

    match result {
        Ok(_) => {
            head_ref
                .set_target(u_oid, &log_message)
                .expect("cant set target");
            get_head(path.clone(), sender.clone());
        }
        Err(err) => {
            debug!(
                "errrrrrrrrrrror {:?} {:?} {:?}",
                err,
                err.code(),
                err.class()
            );
            match (err.code(), err.class()) {
                (git2::ErrorCode::Conflict, git2::ErrorClass::Checkout) => sender
                    .send_blocking(crate::Event::CheckoutError(
                        u_oid,
                        log_message,
                        String::from(err.message()),
                    ))
                    .expect("cant send through channel"),
                (code, class) => {
                    panic!("unknown checkout error {:?} {:?}", code, class)
                }
            };
        }
    }
}
