mod status_view;
use status_view::{factory::text_view_factory, Status, StatusRenderContext};

mod branches_view;
use branches_view::{show_branches_window, Event as BranchesEvent};

use std::cell::RefCell;
use std::rc::Rc;
use std::time::SystemTime;

mod git;
use git::{
    checkout, cherry_pick, commit, create_branch, get_current_repo_status,
    get_refs, kill_branch, merge, push, stage_via_apply, ApplyFilter,
    ApplySubject, BranchData, Diff, DiffKind, File, Head, Hunk, Line, State,
    View,
};
mod widgets;
use widgets::{display_error, make_confirm_dialog};

use libadwaita::prelude::*;
use libadwaita::{
    Application, ApplicationWindow, HeaderBar, Toast, ToastOverlay,
    ToolbarView,
};

use gdk::Display;

use glib::{clone, ControlFlow};

use gtk4::{
    gdk, gio, glib, style_context_add_provider_for_display, Button,
    CssProvider, ScrolledWindow, Settings, TextWindowType,
    STYLE_PROVIDER_PRIORITY_APPLICATION,
};

use log::{debug, info};

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
    settings.set_gtk_font_name(Some("Cantarell 21"));
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
    Branches,
    TextViewResize,
    Toast(String),
}

fn run_with_args(app: &Application, files: &[gio::File], _blah: &str) {
    let le = files.len();
    if le > 0 {
        if let Some(path) = files[0].path() {
            println!("................... {:?}", path);
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
    let mut status = Status::new();
    let mut current_repo_path = initial_path;
    let (sender, receiver) = async_channel::unbounded();

    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);

    let action_close = gio::SimpleAction::new("close", None);
    action_close.connect_activate(clone!(@weak window => move |_, _| {
        window.close();
    }));
    window.add_action(&action_close);
    app.set_accels_for_action("win.close", &["<Ctrl>W"]);

    // works
    // media-playback-start
    // /usr/share/icons/Adwaita/symbolic/actions/media-playback-start-symbolic.svg
    let refresh_btn = Button::builder()
        .label("Refresh")
        .use_underline(true)
        .can_focus(false)
        .tooltip_text("Refresh")
        .icon_name("view-refresh-symbolic")
        .can_shrink(true)
        .build();
    refresh_btn.connect_clicked({
        let p = current_repo_path.clone();
        let s = sender.clone();
        move |_| {
            get_current_repo_status(p.clone(), s.clone());
        }
    });
    let hb = HeaderBar::new();
    hb.pack_start(&refresh_btn);

    let text_view_width = Rc::new(RefCell::<(i32, i32)>::new((0, 0)));
    let txt = text_view_factory(sender.clone(), text_view_width.clone());

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&txt));

    let toast_overlay = ToastOverlay::new();
    toast_overlay.set_child(Some(&scroll));

    let tb = ToolbarView::builder()
        // .content(&scroll)
        .content(&toast_overlay)
        .build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    env_logger::builder().format_timestamp(None).init();

    status.get_status(current_repo_path.clone(), sender.clone());
    window.present();

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            // context is updated on every render
            status.make_context(text_view_width.clone());

            match event {
                Event::CurrentRepo(path) => {
                    current_repo_path.replace(path);
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
                        status.commit(
                            current_repo_path.as_ref().unwrap(),
                            &txt,
                            &window,
                            sender.clone(),
                        );
                    }
                }
                Event::Push => {
                    info!("main.push");
                    status.push(
                        current_repo_path.as_ref().unwrap(),
                        &window,
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
                        ApplySubject::Stage,
                        sender.clone(),
                    );
                }
                Event::UnStage(_offset, line_no) => {
                    status.stage(
                        &txt,
                        line_no,
                        current_repo_path.as_ref().unwrap(),
                        ApplySubject::Unstage,
                        sender.clone(),
                    );
                }
                Event::Kill(_offset, line_no) => {
                    info!("main.kill {:?}", SystemTime::now());
                    status.stage(
                        &txt,
                        line_no,
                        current_repo_path.as_ref().unwrap(),
                        ApplySubject::Kill,
                        sender.clone(),
                    );
                    info!("main.completed kill {:?}", SystemTime::now());
                }
                Event::TextViewResize => {
                    // debug!(
                    //     "rrrrrresize window ----------------_______ {:?}",
                    //     status.context
                    // );
                    status.resize(&txt);
                }
                Event::Toast(title) => {
                    let toast =
                        Toast::builder().title(title).timeout(2).build();
                    toast_overlay.add_toast(toast);
                }
            };
        }
    });
}
