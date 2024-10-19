use crate::dialogs::{alert, ConfirmWithOptions, YES};
use crate::git::commit;
use async_channel::Sender;
use git2::Oid;
use gtk4::{
    gio, glib, ListBox,
    SelectionMode, Widget,
};
use libadwaita::prelude::*;
use libadwaita::SwitchRow;
use std::path::PathBuf;

pub fn cherry_pick(
    repo_path: PathBuf,
    window: &impl IsA<Widget>,
    oid: Oid,
    sender: Sender<crate::Event>,
) {
    glib::spawn_future_local({
        let sender = sender.clone();
        let path = repo_path.clone();
        let window = window.clone();
        async move {
            let list_box = ListBox::builder()
                .selection_mode(SelectionMode::None)
                .css_classes(vec![String::from("boxed-list")])
                .build();
            let no_commit = SwitchRow::builder()
                .title("Only apply changes without commit")
                .css_classes(vec!["input_field"])
                .active(false)
                .build();

            list_box.append(&no_commit);

            let response = alert(ConfirmWithOptions(
                "Cherry pick commit?".to_string(),
                format!("{}", oid),
                list_box.into(),
            ))
            .choose_future(&window)
            .await;
            if response != YES {
                return;
            }
            gio::spawn_blocking({
                let sender = sender.clone();
                let path = path.clone();
                let is_active = no_commit.is_active();
                move || commit::cherry_pick(path, oid, None, None, is_active, sender)
            })
            .await
            .unwrap_or_else(|e| {
                alert(format!("{:?}", e)).present(&window);
                Ok(())
            })
            .unwrap_or_else(|e| {
                alert(e).present(&window);
            });
        }
    });
}
