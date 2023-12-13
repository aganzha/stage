use std::env;
use git2::{Repository, StatusOptions};
use crate::glib::{Sender};
use crate::gio;

fn get_current_repo() -> Result<Repository, String> {
    let mut path_buff = env::current_exe()
        .map_err(|e| format!("can't get repo from executable {:?}", e))?;
    loop {        
        let path = path_buff.as_path();
        println!("tryyyyyyyyyyyyyyyy {:?}", path);
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
    println!("111111111111111111111111111111");
    if let Ok(repo) = get_current_repo() {
        println!("22222222222222222222222222222");
        let path = repo.path();
        sender.send(crate::Event::CurrentRepo(std::ffi::OsString::from(path))).expect("Could not send through channel");
        if let Ok(statuses) = repo.statuses(None) {
            for status_entry in statuses.iter() {
                println!("Status path {:?}", status_entry.path());
                if status_entry.path().is_none() {
                    continue
                }
                let path = status_entry.path().unwrap();
                if path == "target/" {
                    // TODO! check .gitignore flags : Status(IGNORED)
                    continue
                }
                println!("flags : {:?}", status_entry.status());
                println!("head to index {:?}", status_entry.head_to_index());
                println!("index to workdir {:?}", status_entry.index_to_workdir());
            }
        }        
    }
}

pub fn stage_changes(repo: Repository, _sender: Sender<crate::Event>) {
    // gio::spawn_blocking(|| {
        println!("oooooooooooooooooo {:?}", repo.is_empty());
    // });
}
