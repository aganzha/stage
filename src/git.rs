use std::env;
use git2::{Repository, StatusOptions};
use crate::glib::{Sender};
use crate::gio;

fn get_current_repo() -> Result<Repository, String> {
    let mut path_buff = env::current_exe()
        .map_err(|e| format!("can't get repo from executable {:?}", e))?;
    loop {        
        let path = path_buff.as_path();
        let repo_r = Repository::open(path);
        if let Ok(repo) =repo_r {
            return Ok(repo)
        } else {
            if !path_buff.pop() {
                break
            }            
        }        
    }
    Err("no repoitory found".to_string())
}

pub fn get_current_repo_status(sender: Sender<crate::Event>) {
    if let Ok(repo) = get_current_repo() {
        sender.send(crate::Event::CurrentRepo(repo.path())).expect("Could not send through channel");
        let statuses = repo.statuses(None);
    }
}

pub fn stage_changes(repo: Repository, _sender: Sender<crate::Event>) {
    // gio::spawn_blocking(|| {
        println!("oooooooooooooooooo {:?}", repo.is_empty());
    // });
}
