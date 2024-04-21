mod status_view;
use status_view::{factory::text_view_factory, Status, StatusRenderContext};

mod branches_view;
use branches_view::{show_branches_window, Event as BranchesEvent};

mod stashes_view;
use stashes_view::factory as stashes_view_factory;

mod commit_view;
use commit_view::show_commit_window;

use core::time::Duration;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::SystemTime;

mod git;
use git::{
    apply_stash, checkout, cherry_pick, commit, create_branch, drop_stash,
    get_current_repo_status, get_directories, get_refs, kill_branch, merge,
    pull, push, reset_hard, stage_untracked, stage_via_apply, stash_changes, get_commit_diff,
    track_changes, ApplyFilter, ApplySubject, BranchData, Diff, DiffKind, CommitDiff,
    File, Head, Hunk, Line, StashData, Stashes, State, Untracked,
    UntrackedFile, View,
};
use git2::Oid;
mod widgets;
use widgets::{display_error, make_confirm_dialog, make_header_bar};

use gdk::Display;
use glib::{clone, ControlFlow};
use libadwaita::prelude::*;
use libadwaita::{
    Application, ApplicationWindow, HeaderBar, OverlaySplitView, Toast,
    ToastOverlay, ToolbarStyle, ToolbarView,
};

use gtk4::{
    gdk, gio, glib, style_context_add_provider_for_display, Align, Button,
    CssProvider, ScrolledWindow, Settings, TextView, TextWindowType,
    STYLE_PROVIDER_PRIORITY_APPLICATION,
};

use log::{debug, info};
use regex::Regex;

const APP_ID: &str = "com.github.aganzha.stage";

fn main() -> glib::ExitCode {
    let app: Application;
    if let Some(_path) = std::env::args().nth(1) {
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
    let display = Display::default().expect("Could not connect to a display.");
    let settings = Settings::for_display(&display);
    settings.set_gtk_font_name(Some("Cantarell 18")); // "Cantarell 21"
    let provider = CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    style_context_add_provider_for_display(
        &display,
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
    Upstream(Option<Head>),
    State(State),
    Expand(i32, i32),
    Cursor(i32, i32),
    // does not used for now
    Stage(i32, i32),
    UnStage(i32, i32),
    Kill(i32, i32),
    Commit,
    Push,
    Pull,
    Branches,
    ShowOid(Oid),
    TextViewResize,
    Toast(String),
    StashesPanel,
    Stashes(Stashes),
    Refresh,
    Zoom(bool),
    Untracked(Untracked),
    ResetHard,
    CommitDiff(CommitDiff)
    // Monitors(Vec<gio::FileMonitor>)
}

fn zoom(dir: bool) {
    let display = Display::default().expect("Could not connect to a display.");
    let settings = Settings::for_display(&display);
    let font = settings.gtk_font_name().expect("cant get font");
    // "Cantarell 21"
    let re = Regex::new(r".+ ([0-9]+)").expect("fail in regexp");
    if let Some((_whole, [size])) =
        re.captures_iter(&font).map(|c| c.extract()).next()
    {
        let mut int_size = size.parse::<i32>().expect("cant parse size");
        if dir {
            if int_size < 64 {
                int_size += 1;
            }
        } else {
            if int_size > 1 {
                int_size -= 1;
            }
        }
        settings.set_gtk_font_name(Some(&format!("Cantarell {}", int_size)));
    };
}

fn run_with_args(app: &Application, files: &[gio::File], _blah: &str) {
    let le = files.len();
    if le > 0 {
        if let Some(path) = files[0].path() {
            run_app(app, Some(path.into_os_string()));
            return;
        }
    }
    run_app(app, None)
}

fn run_without_args(app: &Application) {
    run_app(app, None)
}

fn run_app(app: &Application, initial_path: Option<std::ffi::OsString>) {
    env_logger::builder().format_timestamp(None).init();

    let (sender, receiver) = async_channel::unbounded();
    let monitors = Rc::new(RefCell::<Vec<gio::FileMonitor>>::new(Vec::new()));

    let mut status = Status::new(initial_path, sender.clone());
    status.setup_monitor(monitors.clone());
    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);

    let action_close = gio::SimpleAction::new("close", None);
    action_close.connect_activate(clone!(@weak window => move |_, _| {
        window.close();
    }));
    window.add_action(&action_close);
    app.set_accels_for_action("win.close", &["<Ctrl>W"]);

    let hb = make_header_bar(sender.clone());

    let text_view_width = Rc::new(RefCell::<(i32, i32)>::new((0, 0)));
    let txt = text_view_factory(sender.clone(), text_view_width.clone());

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&txt));

    let toast_overlay = ToastOverlay::new();
    toast_overlay.set_child(Some(&scroll));

    let split = OverlaySplitView::builder()
        .content(&toast_overlay)
        .show_sidebar(false)
        .min_sidebar_width(400.0)
        .build();

    let tb = ToolbarView::builder()
        .top_bar_style(ToolbarStyle::Raised)
        .content(&split)
        .build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    status.get_status();
    window.present();

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            // context is updated on every render
            status.make_context(text_view_width.clone());
            // debug!("main looooop {:?} {:p}", monitors, &monitors);
            match event {
                Event::CurrentRepo(path) => {
                    info!("info.path {:?}", path);
                    status.update_path(path, monitors.clone());
                }
                Event::State(state) => {
                    info!("main. state {:?}", &state);
                    status.update_state(state, &txt);
                }
                Event::Debug => {
                    info!("main. debug");
                    status.debug(&txt);
                }
                Event::Commit => {
                    info!("main.commit");
                    if !status.has_staged() {
                        display_error(
                            &window,
                            "No changes were staged. Stage by hitting 's'",
                        );
                    } else {
                        status.commit(&txt, &window);
                    }
                }
                Event::Untracked(untracked) => {
                    info!("main. untracked");
                    status.update_untracked(untracked, &txt);
                }
                Event::Push => {
                    info!("main.push");
                    status.push(&window);
                }
                Event::Pull => {
                    info!("main.pull");
                    status.pull();
                }
                Event::Branches => {
                    info!("main.braches");
                    show_branches_window(
                        status.path.clone().expect("no path"),
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
                    status.stage(&txt, line_no, ApplySubject::Stage);
                }
                Event::UnStage(_offset, line_no) => {
                    status.stage(&txt, line_no, ApplySubject::Unstage);
                }
                Event::Kill(_offset, line_no) => {
                    info!("main.kill {:?}", SystemTime::now());
                    status.stage(&txt, line_no, ApplySubject::Kill);
                    info!("main.completed kill {:?}", SystemTime::now());
                }
                Event::TextViewResize => {
                    status.resize(&txt);
                }
                Event::Toast(title) => {
                    info!("toast");
                    let toast =
                        Toast::builder().title(title).timeout(2).build();
                    toast_overlay.add_toast(toast);
                }
                Event::Zoom(dir) => {
                    zoom(dir);
                    // when zoom, TextView become offset from scroll
                    // on some step. this is a hack to force rerender
                    // this pair to allow TextView accomodate whole
                    // width of ScrollView
                    scroll.set_halign(Align::Start);
                    glib::source::timeout_add_local(
                        Duration::from_millis(30),
                        {
                            let scroll = scroll.clone();
                            move || {
                                scroll.set_halign(Align::Fill);
                                ControlFlow::Break
                            }
                        },
                    );
                    status.resize(&txt);
                }
                Event::Stashes(stashes) => {
                    info!("stashes data");
                    status.update_stashes(stashes)
                }
                Event::StashesPanel => {
                    info!("stashes panel");
                    if split.shows_sidebar() {
                        split.set_show_sidebar(false);
                        txt.grab_focus();
                    } else {
                        // stashes_filler(&status);
                        let (view, focus) =
                            stashes_view_factory(&window, &status);
                        split.set_sidebar(Some(&view));
                        split.set_show_sidebar(true);
                        focus();
                    }
                }
                Event::ShowOid(oid) => {
                    info!("main.show oid");
                    show_commit_window(
                        status.path.clone().expect("no path"),
                        oid,
                        &window,
                        sender.clone(),
                    );
                }
                Event::ResetHard => {
                    info!("main. reset hard");
                    status.reset_hard(sender.clone());
                }
                Event::Refresh => {
                    status.get_status();
                }
                Event::CommitDiff(_d) => {
                    panic!("got oid diff in another receiver");
                }
            };
        }
    });
}
