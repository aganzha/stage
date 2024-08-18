// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::{get_directories, Event, Status};
use core::time::Duration;
use gio::{
    Cancellable, File, FileMonitor, FileMonitorEvent, FileMonitorFlags,
};
use gtk4::prelude::*;
use gtk4::{gio, glib};
use log::trace;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

impl Status {
    pub fn lock_monitors(&mut self, lock: bool) {
        self.monitor_global_lock.replace(lock);
    }

    pub fn setup_monitors(
        &mut self,
        monitors: Rc<RefCell<Vec<FileMonitor>>>,
        path: PathBuf,
    ) {
        glib::spawn_future_local({
            let sender = self.sender.clone();
            let lock = self.monitor_lock.clone();
            let global_lock = self.monitor_global_lock.clone();
            async move {
                let mut directories = gio::spawn_blocking({
                    let path = path.clone();
                    move || get_directories(path)
                })
                .await
                .expect("cant get direcories");
                let root = path
                    .to_str()
                    .expect("cant get string from path")
                    .replace(".git/", "");
                directories.insert(root.clone());
                for dir in directories {
                    trace!("dirname {:?}", dir);
                    let dir_name = match dir {
                        name if name == root => name,
                        name => {
                            format!("{}{}", root, name)
                        }
                    };
                    trace!("setup monitor {:?}", dir_name);
                    let file = File::for_path(dir_name);
                    let flags = FileMonitorFlags::empty();

                    let monitor = file
                        .monitor_directory(
                            flags,
                            Cancellable::current().as_ref(),
                        )
                        .expect("cant get monitor");
                    monitor.connect_changed({
                        let path = path.clone();
                        let sender = sender.clone();
                        let lock = lock.clone();
                        let global_lock = global_lock.clone();
                        move |_monitor, file, _other_file, event| {
                            // TODO get from SELF.settings
                            if *global_lock.borrow() {
                                trace!("no way, global lock on monitor");
                                return;
                            }
                            let patterns_to_exclude: Vec<&str> =
                                vec!["/.#", "/mout", "flycheck_", "/sed", ".goutputstream"];
                            match event {
                                FileMonitorEvent::Changed | FileMonitorEvent::Created | FileMonitorEvent::Deleted => {
                                    // ChangesDoneHint is not fired for small changes :(
                                    let fp =
                                        file.path().expect("no file path");
                                    let mut str_file_path = fp.to_str().expect("wrong path");
                                    str_file_path = str_file_path.strip_prefix(
                                        &path
                                            .to_str()
                                            .unwrap()
                                            .replace("./git/", "")
                                    ).expect("wrong path in strip");
                                    trace!("file path in monitor! {:?}", str_file_path);
                                    for pat in patterns_to_exclude {
                                        if str_file_path.contains(pat) {
                                            return;
                                        }
                                    }
                                    let file_path: PathBuf = str_file_path.into();
                                    if lock.borrow().contains(&file_path) {
                                        trace!("NO WAY: monitor locked");
                                        return;
                                    }
                                    lock.borrow_mut().insert(file_path.clone());
                                    trace!("set monitor lock for file {:?}", &file_path);
                                    glib::source::timeout_add_local(
                                        Duration::from_millis(300),
                                        {
                                            let lock = lock.clone();
                                            let sender = sender.clone();
                                            move || {
                                                lock.borrow_mut().remove(&file_path);
                                                sender.send_blocking(Event::TrackChanges(file_path.clone()))
                                                    .expect("cant send through channel");
                                                glib::ControlFlow::Break
                                            }
                                        },
                                    );
                                }
                                _ => {
                                    trace!(
                                        "file event in monitor {:?} {:?}",
                                        event,
                                        file.path()
                                    );
                                }
                            }
                        }
                    });
                    monitors.borrow_mut().push(monitor);
                }
                trace!("my monitors a set {:?}", monitors.borrow().len());
            }
        });
    }
}
