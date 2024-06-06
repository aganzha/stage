use libadwaita::prelude::*;
use gtk4::prelude::*;
use crate::{Event, git::{commit as git_commit}};
use libadwaita::{
    ApplicationWindow, Banner, EntryRow, PasswordEntryRow, SwitchRow,
};
use gtk4::{
    gio, glib, Box, Label as GtkLabel, ListBox, Orientation, SelectionMode,
    TextBuffer, TextView, Widget, ScrolledWindow, WrapMode, EventControllerKey,
    gdk
};
use crate::dialogs::{alert, DangerDialog, YES};
use async_channel::Sender;
use std::path::PathBuf;

pub fn commit(path: Option<PathBuf>, ammend_allowed: bool, window: &ApplicationWindow, sender: Sender<Event>) {
    glib::spawn_future_local({
        let window = window.clone();
        let sender = sender.clone();
        let path = path.clone();
        async move {

            let txt = TextView::builder()
                .margin_start(12)
                .margin_end(12)
                .margin_top(12)
                .margin_bottom(12)
                .wrap_mode(WrapMode::Word)
                .build();
            let scroll = ScrolledWindow::builder()
                .vexpand(true)
                .vexpand_set(true)
                .hexpand(true)
                .hexpand_set(true)
                .min_content_width(480)
                .min_content_height(320)
                .build();

            scroll.set_child(Some(&txt));

            let bx = Box::builder()
                .hexpand(true)
                .vexpand(true)
                .vexpand_set(true)
                .hexpand_set(true)
                .orientation(Orientation::Vertical)
                .build();

            bx.append(&scroll);

            let dialog = crate::confirm_dialog_factory(
                &window,
                Some(&bx),
                "Commit",
                "Commit",
            );
            
            let switch = SwitchRow::builder()
                .title("Amend")
                .css_classes(vec!["input_field"])
                .active(false)
                .build();
            
            if ammend_allowed {
                let lb = ListBox::builder()
                    .selection_mode(SelectionMode::None)
                    .css_classes(vec![String::from("boxed-list")])
                    .build();
                lb.append(&switch);
                bx.append(&lb);
            }
            

            let label = GtkLabel::builder()
                .label("Ctrl-c or Ctrl-Enter to commit. Esc to exit")
                .build();

            bx.append(&label);
            
            let key_controller = EventControllerKey::new();
            key_controller.connect_key_pressed({
                let dialog = dialog.clone();
                move |_, key, _, modifier| {
                    match (key, modifier) {
                        (gdk::Key::Return, gdk::ModifierType::CONTROL_MASK) => {
                            dialog.response("confirm");
                            dialog.close();

                        }
                        (gdk::Key::c, gdk::ModifierType::CONTROL_MASK) => {
                            dialog.response("confirm");
                            dialog.close();
                        }
                        (_, _) => {}
                    }
                    glib::Propagation::Proceed
                }
            });
            txt.add_controller(key_controller);
            let response = dialog.choose_future().await;
            if "confirm" != response {
                return;
            }

            gio::spawn_blocking({
                // let message = format!("{}", input.text());
                let buffer = txt.buffer();
                let start_iter = buffer.iter_at_offset(0);
                let eof_iter = buffer.end_iter();
                let message = buffer.text(&start_iter, &eof_iter, true).to_string();
                let amend = switch.is_active();
                move || {                            
                    git_commit::create_commit(
                        path.expect("no path"),
                        message,
                        amend,
                        sender,
                    )
                }
            }).await
                .unwrap_or_else(|e| {
                    alert(format!("{:?}", e))
                        .present(&window);
                    Ok(())
                })
                .unwrap_or_else(|e| {
                    alert(e).present(&window);
                    
                });
        }
    });
}
