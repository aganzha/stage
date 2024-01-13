mod text_view;
use text_view::{debug, text_view_factory, RenderSource, Status};

mod common_tests;

mod git;
use git::{
    commit_staged, get_current_repo_status, stage_via_apply, ApplyFilter, Diff, File, Hunk, Line,
    View,
};

mod widgets;
use widgets::{display_error, show_commit_message};

use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, MessageDialog, ResponseAppearance};
use gdk::Display;

use glib::{clone, MainContext, Priority, Sender};
use gtk::prelude::*;
use gtk::{gdk, gio, glib, Box, CssProvider, Entry, Label, Orientation, ScrolledWindow, TextView};

const APP_ID: &str = "io.github.aganzha.Stage";

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_startup(|_| load_css());
    app.connect_activate(build_ui);

    app.run()
}

fn load_css() {
    // Load the CSS file and add it to the provider
    let provider = CssProvider::new();
    provider.load_from_data(include_str!("style.css"));

    // Add the provider to the default screen
    gtk::style_context_add_provider_for_display(
        &Display::default().expect("Could not connect to a display."),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

pub enum Event {
    Debug,
    CurrentRepo(std::ffi::OsString),
    Unstaged(Diff),
    Staged(Diff),
    Expand(i32, i32),
    Cursor(i32, i32),
    // does not used for now
    Stage(i32, i32),
    UnStage(i32, i32),
    CommitRequest,
    Commit(String),
}

fn build_ui(app: &adw::Application) {
    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);
    // window.set_default_size(640, 480);
    let scroll = ScrolledWindow::new();

    let action_close = gio::SimpleAction::new("close", None);
    action_close.connect_activate(clone!(@weak window => move |_, _| {
        window.close();
    }));
    window.add_action(&action_close);
    app.set_accels_for_action("win.close", &["<Ctrl>W"]);

    let container = Box::builder().build();
    container.set_orientation(Orientation::Vertical);
    container.add_css_class("stage");
    let hb = HeaderBar::new();
    let lbl = Label::builder()
        .label("stage")
        .single_line_mode(true)
        .width_chars(5)
        .build();
    hb.set_title_widget(Some(&lbl));
    container.append(&hb);

    let (sender, receiver) = MainContext::channel(Priority::default());

    let txt = text_view_factory(sender.clone());

    scroll.set_min_content_height(960);
    scroll.set_max_content_height(960);
    scroll.set_child(Some(&txt));

    container.append(&scroll);

    window.set_content(Some(&container));

    let mut current_repo_path: Option<std::ffi::OsString> = None;
    let mut status = Status::new();

    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            get_current_repo_status(None, sender);
        }
    });
    window.present();

    receiver.attach(None, move |event: Event| {
        match event {
            Event::CurrentRepo(path) => {
                current_repo_path.replace(path);
            }
            Event::Debug => {
                println!("main. FAKE");
                debug(&txt, &mut status);
            }
            Event::CommitRequest => {
                println!("commit request");
                if !status.has_staged() {
                    display_error(&window, "No changes were staged. Stage by hitting 's'");
                } else {
                    show_commit_message(&window, sender.clone());
                }
            }
            Event::Commit(message) => {
                println!("do commit! {:?}", message);
                commit_staged(current_repo_path.as_ref().unwrap(), message, sender.clone());
                // gio::spawn_blocking({
                //     let sender = sender.clone();
                //     move || {
                //         commit_staged(path, message, sender);
                //     }
                // });
            }
            Event::Staged(d) => {
                println!("main. staged {:p}", &d);
                status.staged.replace(d);
                if status.staged.is_some() && status.unstaged.is_some() {
                    status.render(&txt, RenderSource::Git);
                }
            }
            Event::Unstaged(d) => {
                println!("main. unstaged {:p}", &d);
                status.unstaged.replace(d);
                if status.staged.is_some() && status.unstaged.is_some() {
                    status.render(&txt, RenderSource::Git);
                }
            }
            Event::Expand(offset, line_no) => {
                status.expand(&txt, line_no, offset);
            }
            Event::Cursor(offset, line_no) => {
                status.cursor(&txt, line_no, offset);
            }
            Event::Stage(_offset, line_no) => {
                status.stage(
                    &txt,
                    line_no,
                    current_repo_path.as_ref().unwrap(),
                    true,
                    sender.clone(),
                );
            }
            Event::UnStage(_offset, line_no) => {
                status.stage(
                    &txt,
                    line_no,
                    current_repo_path.as_ref().unwrap(),
                    false,
                    sender.clone(),
                );
            }
        };
        glib::ControlFlow::Continue
    });
}
