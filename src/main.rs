mod text_view;
use text_view::{text_view_factory, Status};

mod branches_view;
use branches_view::{show_branches_window, Event as BranchesEvent};

mod common_tests;

//use std::sync::mpsc::channel;
//use std::sync::mpsc::Sender;
use async_channel::Sender;

mod git;
use git::{
    checkout, commit_staged, create_branch, get_current_repo_status, get_refs,
    kill_branch, cherry_pick, push, stage_via_apply, ApplyFilter, BranchData,
    Diff, File, Head, Hunk, Line, Related, View, State
};
mod widgets;
use widgets::{
    display_error, get_new_branch_name, show_commit_message, show_push_message,
};

use libadwaita::prelude::*;
use libadwaita::{
    Application, ApplicationWindow, HeaderBar, ToolbarView, Window
};

use gdk::Display;

use glib::{clone, MainContext, Priority};
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, style_context_add_provider_for_display, Box, CssProvider,
    Label, Orientation, ScrolledWindow, STYLE_PROVIDER_PRIORITY_APPLICATION
};

use log::{debug, error, info, log_enabled, trace};

const APP_ID: &str = "com.github.aganzha.stage";

fn main() -> glib::ExitCode {

    // let pattern = std::env::args().nth(1).expect("no pattern given");
    // let path = std::env::args().nth(2).expect("no path given");
    // println!("-------- {:?} -----------------> {:?}", pattern, path);
    // let arg = std::env::args().nth(1);
    // let mut path_arg: Option<String> = None;
    // if let Some(path) = arg {
    //     path_arg.replace(format!("{}", path));
    //     println!("----------> {:?}", path);
    // }
    let mut app: Application;
    if let Some(path) = std::env::args().nth(1) {
        app = Application::builder()
            .application_id(APP_ID)
            .flags(gio::ApplicationFlags::HANDLES_OPEN)
            .build();
        app.connect_startup(|_| load_css());
        app.connect_open(run_with_args);
    } else {
        app = Application::builder()
            .application_id(APP_ID)
            .flags(gio::ApplicationFlags::HANDLES_OPEN)
            .build();
        app.connect_startup(|_| load_css());
        app.connect_activate(run_without_args);
    }

    app.run()    
}

fn load_css() {
    // Load the CSS file and add it to the provider
    let provider = CssProvider::new();
    provider.load_from_data(include_str!("style.css"));

    // Add the provider to the default screen
    style_context_add_provider_for_display(
        &Display::default().expect("Could not connect to a display."),
        &provider,
        STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[derive(Debug)]
pub enum Event {
    Debug,
    CurrentRepo(std::ffi::OsString),
    Unstaged(Diff),
    Staged(Diff),
    Head(Head),
    Upstream(Head),
    State(State),
    Expand(i32, i32),
    Cursor(i32, i32),
    // does not used for now
    Stage(i32, i32),
    UnStage(i32, i32),
    CommitRequest,
    // NewBranchRequest,
    PushRequest,
    Commit(String),
    Push,
    Branches,
}

fn run_with_args(app: &Application, files: &[gio::File], blah: &str) {
    let le = files.len();
    if le > 0 {
        if let Some(path) = files[0].path() {
            println!("................... {:?}", path);
            run_app(app, Some(path.into_os_string()));
            return;
        }
    }
    println!("heeeeeeeeeeeeeeeeeeeeeeeeeeeeeeey {:?} {:?}", files, blah);
    run_app(app, None)
}
fn run_without_args(app: &Application) {
    run_app(app, None)
}
// fn build_ui(app: &Application, files: &[gtk4::gio::File], blah: &str) {    

fn run_app(app: &Application, initial_path: Option<std::ffi::OsString>) {        

    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);

    let action_close = gio::SimpleAction::new("close", None);
    action_close.connect_activate(clone!(@weak window => move |_, _| {
        window.close();
    }));
    window.add_action(&action_close);
    app.set_accels_for_action("win.close", &["<Ctrl>W"]);

    let hb = HeaderBar::new();

    let (sender, receiver) = async_channel::unbounded();

    let txt = text_view_factory(sender.clone());

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&txt));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    env_logger::builder().format_timestamp(None).init();
    let mut current_repo_path = initial_path;
    let mut status = Status::new();
    status.get_status(current_repo_path.clone(), sender.clone());
    window.present();

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            match event {
                Event::CurrentRepo(path) => {
                    current_repo_path.replace(path);
                }
                Event::State(state) => {
                    status.update_state(state, &txt);
                }
                Event::Debug => {
                    info!("main. debug");
                    status.debug(&txt);
                }
                Event::CommitRequest => {
                    info!("commit request");
                    if !status.has_staged() {
                        display_error(
                            &window,
                            "No changes were staged. Stage by hitting 's'",
                        );
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
                Event::Branches => {
                    info!("main.braches");
                    show_branches_window(
                        current_repo_path.as_ref().unwrap().clone(),
                        &window,
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
        }
    });
}
