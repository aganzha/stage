use std::{env, str};
use git2::{Repository, StatusOptions, ObjectType, Oid};
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
        let path = repo.path();
        sender.send(crate::Event::CurrentRepo(std::ffi::OsString::from(path)))
            .expect("Could not send through channel");
        if let Ok(statuses) = repo.statuses(None) {
            for status_entry in statuses.iter() {
                println!("Status path {:?}", status_entry.path());
                if status_entry.path().is_none() {
                    continue
                }
                let path = status_entry.path().unwrap();
                // if path == "target/" {
                //     // TODO! check .gitignore flags : Status(IGNORED)
                //     continue
                // }
                // println!("flags : {:?}", status_entry.status());
                // println!("head to index {:?}", status_entry.head_to_index());
                // println!("index to workdir {:?}", status_entry.index_to_workdir());
                // println!("");
                // println!("->");
                if let Some(workdir_delta) = status_entry.index_to_workdir() {
                    let id = workdir_delta.old_file().id();
                    if let Some(text) = blob_text_from_id(&repo, id) {
                        println!("old_file");
                        print!("{}", text);
                    }
                    // println!("id of diff file, which must be blob {:?}, zero? {:?}", id, id.is_zero());
                    // if !id.is_zero() {
                    //     let ob = repo.find_object(id, Some(ObjectType::Blob));
                    //     println!("Ooooooooooooooob {:?}", ob);
                    //     if let Ok(blob) = ob {
                    //         let bl = blob.as_blob();
                    //         if let Some(the_blob) = bl {
                    //             let bytes = the_blob.content();
                    //             //println!("blob? {:?}", str::from_utf8(bytes));
                    //             let s = str::from_utf8(bytes).unwrap();
                    //             print!("{}", s);
                    //             println!("");
                    //             println!("");
                    //         }
                    //     }                        
                    // }
                }
            }
        }        
    }
}

pub fn blob_text_from_id(repo: &Repository, id: Oid) -> Option<String> {
    if !id.is_zero() {
        let ob = repo.find_object(id, Some(ObjectType::Blob));
        println!("Ooooooooooooooob {:?}", ob);
        if let Ok(blob) = ob {
            let bl = blob.as_blob();
            if let Some(the_blob) = bl {
                let bytes = the_blob.content();
                let s = str::from_utf8(bytes).unwrap();
                return Some(String::from(s))
            }
        }                        
    }
    None
}



pub fn stage_changes(repo: Repository, _sender: Sender<crate::Event>) {
    // gio::spawn_blocking(|| {
        println!("oooooooooooooooooo {:?}", repo.is_empty());
    // });
}
