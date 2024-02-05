mod text_view;
use text_view::{text_view_factory, Status};

mod common_tests;

mod git;
use git::{
    commit_staged, get_current_repo_status, push,
    stage_via_apply, ApplyFilter, Diff, File, Head,
    Hunk, Line, View,
};

mod widgets;
use widgets::{
    display_error, show_commit_message,
    show_push_message,
};

use adw::prelude::*;
use adw::{
    Application, ApplicationWindow, HeaderBar,
    MessageDialog, ResponseAppearance,
};
use gdk::Display;

use glib::{clone, MainContext, Priority, Sender};
use gtk::prelude::*;
use gtk::{
    gdk, gio, glib, Box, CssProvider, Entry, Label,
    Orientation, ScrolledWindow, TextView,
};

use log::{debug, error, info, log_enabled, trace};

const APP_ID: &str = "com.github.aganzha.stage";

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_startup(|_| load_css());
    app.connect_activate(build_ui);

    app.run()
}

fn load_css() {
    // Load the CSS file and add it to the provider
    let provider = CssProvider::new();
    provider
        .load_from_data(include_str!("style.css"));

    // Add the provider to the default screen
    gtk::style_context_add_provider_for_display(
        &Display::default().expect(
            "Could not connect to a display.",
        ),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

pub enum Event {
    Debug,
    CurrentRepo(std::ffi::OsString),
    Unstaged(Diff),
    Staged(Diff),
    Head(Head),
    Upstream(Head),
    Expand(i32, i32),
    Cursor(i32, i32),
    // does not used for now
    Stage(i32, i32),
    UnStage(i32, i32),
    CommitRequest,
    PushRequest,
    Commit(String),
    Push,
}

fn build_ui(app: &adw::Application) {
    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);
    // window.set_default_size(640, 480);
    let scroll = ScrolledWindow::new();

    let action_close =
        gio::SimpleAction::new("close", None);
    action_close.connect_activate(
        clone!(@weak window => move |_, _| {
            window.close();
        }),
    );
    window.add_action(&action_close);
    app.set_accels_for_action(
        "win.close",
        &["<Ctrl>W"],
    );

    let container = Box::builder().build();
    container
        .set_orientation(Orientation::Vertical);
    container.add_css_class("stage");
    let hb = HeaderBar::new();
    let lbl = Label::builder()
        .label("stage")
        .single_line_mode(true)
        .width_chars(5)
        .build();
    hb.set_title_widget(Some(&lbl));
    container.append(&hb);

    let (sender, receiver) =
        MainContext::channel(Priority::default());
    let txt = text_view_factory(sender.clone());

    scroll.set_min_content_height(960);
    scroll.set_max_content_height(960);
    scroll.set_child(Some(&txt));

    container.append(&scroll);

    window.set_content(Some(&container));

    env_logger::builder()
        .format_timestamp(None)
        .init();

    let mut current_repo_path: Option<
        std::ffi::OsString,
    > = None;
    let mut status = Status::new();
    status.get_status(sender.clone());
    window.present();

    receiver.attach(None, move |event: Event| {
        // let sett = txt.settings();
        // debug!("cursor settings {} {} {} {}", sett.is_gtk_cursor_blink(), sett.gtk_cursor_aspect_ratio(), sett.gtk_cursor_blink_time(), sett.gtk_cursor_blink_timeout());
        match event {
            Event::CurrentRepo(path) => {
                current_repo_path.replace(path);
            }
            Event::Debug => {
                info!("main. debug");
                status.debug(&txt);
            }
            Event::CommitRequest => {
                info!("commit request");
                if !status.has_staged() {
                    display_error(&window, "No changes were staged. Stage by hitting 's'");
                } else {
                    show_commit_message(&window, sender.clone());
                }
            }
            Event::PushRequest => {
                info!("main.push request");
                // todo - check that there is something to push
                show_push_message(&window, sender.clone());
            }
            Event::Commit(message) => {
                info!("main.commit");
                status.commit_staged(
                    current_repo_path.as_ref().unwrap(),
                    message,
                    &txt,
                    sender.clone(),
                );
            }
            Event::Push => {
                info!("main.push");
                status.push(
                    current_repo_path.as_ref().unwrap(),
                    &txt,
                    sender.clone(),
                );
            }
            Event::Head(h) => {
                info!("main. head");
                status.update_head(h, &txt);
            }
            Event::Upstream(h) => {
                info!("main. upstream");
                status.update_upstream(h, &txt);
            }
            Event::Staged(d) => {
                info!("main. staged {:p}", &d);
                status.update_staged(d, &txt);
            }
            Event::Unstaged(d) => {
                info!("main. unstaged {:p}", &d);
                status.update_unstaged(d, &txt);
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
