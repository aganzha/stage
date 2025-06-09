// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod external;
mod status_view;
mod syntax;
use status_view::{
    context::StatusRenderContext,
    headerbar::factory as headerbar_factory,
    headerbar::{HbUpdateData, Scheme, SCHEME_TOKEN},
    remotes::auth,
    stage_view::factory as stage_factory,
    Status,
};

mod branches_view;
use branches_view::show_branches_window;

mod log_view;
use log_view::show_log_window;

mod tags_view;
use tags_view::show_tags_window;

mod stashes_view;
use stashes_view::factory as stashes_view_factory;

mod commit_view;
use commit_view::show_commit_window;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
mod git;
use git::{
    branch, commit, get_current_repo_status, get_directories, reset_hard, stage_untracked,
    stage_via_apply,
    stash::{StashNum, Stashes},
    Diff, DiffKind, File, Head, Hunk, HunkLineNo, Line, LineKind, State, MARKER_OURS,
    MARKER_THEIRS,
};
use git2::Oid;
mod dialogs;
use dialogs::alert;

mod tests;
use gdk::Display;
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, style_context_add_provider_for_display,
    style_context_remove_provider_for_display, Box as Gtk4Box, CssProvider, Orientation,
    ScrolledWindow, STYLE_PROVIDER_PRIORITY_USER,
};
use libadwaita::prelude::*;
use libadwaita::{
    Application, ApplicationWindow, Banner, OverlaySplitView, StyleManager, Toast, ToastOverlay,
    ToolbarStyle, ToolbarView, Window,
};

use log::{info, trace};

const APP_ID: &str = "io.github.aganzha.Stage";

pub const DARK_CLASS: &str = "dark";
pub const LIGHT_CLASS: &str = "light";

#[derive(Clone)]
enum CurrentWindow {
    Window(Window),
    ApplicationWindow(ApplicationWindow),
}

#[derive(Debug, Clone)]
pub struct LoginPassword {
    login: String,
    password: String,
    cancel: bool,
    pending: bool,
}

impl Default for LoginPassword {
    fn default() -> Self {
        Self {
            login: String::from(""),
            password: String::from(""),
            cancel: false,
            pending: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOp {
    Stage,
    Unstage,
    Kill,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplyOp {
    CherryPick(Oid, Option<PathBuf>, Option<String>),
    Revert(Oid, Option<PathBuf>, Option<String>),
    Stash(Oid, StashNum, Option<PathBuf>, Option<String>),
}

#[derive(Debug)]
pub enum Event {
    Debug,
    Dump,
    OpenRepo(PathBuf),
    CurrentRepo(PathBuf),
    Conflicted(Option<Diff>, Option<State>),
    Unstaged(Option<Diff>),
    Untracked(Option<Diff>),
    Staged(Option<Diff>),
    Head(Option<Head>),
    Upstream(Option<Head>),
    UpstreamProgress,
    State(State),
    OpenFileDialog,
    RepoPopup,
    Expand(i32, i32),
    Cursor(i32, i32),
    Stage(StageOp),
    Commit,
    Push,
    Pull,
    ShowBranches,
    Branches(Vec<branch::BranchData>),
    Log(Option<Oid>, Option<String>),
    ShowOid(Oid, Option<StashNum>, Option<HunkLineNo>),
    ShowTextOid(String),
    TextViewResize(i32),
    TextCharVisibleWidth(i32),
    Toast(String),
    StashesPanel,
    Stashes(Stashes),
    Refresh,
    RemotesDialog,
    Zoom(bool),
    ResetHard(Option<Oid>),
    CommitDiff(commit::CommitDiff),
    LockMonitors(bool),
    StoreSettings(String, String),
    OpenEditor,
    Tags(Option<Oid>),
    Apply(ApplyOp),
    Focus,
    UserInputRequired(Arc<(Mutex<LoginPassword>, Condvar)>),
    Blame,
}

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    app.connect_startup(|_| {
        let display = Display::default().expect("cant get dispay");
        let provider = CssProvider::new();
        provider.load_from_string(include_str!("style.css"));
        style_context_add_provider_for_display(&display, &provider, STYLE_PROVIDER_PRIORITY_USER);
    });
    app.connect_startup(|_| {
        gio::resources_register_include!("gresources.compiled")
            .expect("Failed to register resources.");
    });

    let initial_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    app.connect_open({
        let initial_path = initial_path.clone();
        move |opened_app: &Application, files: &[gio::File], _: &str| {
            if !files.is_empty() {
                if let Some(path) = files[0].path() {
                    initial_path.replace(Some(path));
                }
            }
            opened_app.activate();
        }
    });
    app.connect_activate({
        let initial_path = initial_path.clone();
        move |running_app| {
            let windows = running_app.windows();
            if windows.is_empty() {
                run_app(running_app, &initial_path.borrow());
            } else {
                windows[0].present();
            }
        }
    });
    app.run()
}

pub fn get_settings() -> gio::Settings {
    if let Some(system_schema_source) = gio::SettingsSchemaSource::default() {
        if let Some(schema) = system_schema_source.lookup(APP_ID, false) {
            return gio::Settings::new(&schema.id());
        }
    }
    let mut exe_path = std::env::current_exe().expect("cant get exe path");
    exe_path.pop();
    let exe_path = exe_path.as_path();
    let schema_source =
        gio::SettingsSchemaSource::from_directory(exe_path, None, true).expect("no source");
    let schema = schema_source.lookup(APP_ID, false).expect("no schema");
    gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None)
}

fn run_app(app: &Application, initial_path: &Option<PathBuf>) {
    env_logger::builder().format_timestamp(None).init();

    let (sender, receiver) = async_channel::unbounded();
    let monitors = Rc::new(RefCell::<Vec<gio::FileMonitor>>::new(Vec::new()));

    let settings = get_settings();

    let font_size = settings.get::<i32>("zoom");
    let provider = CssProvider::new();
    provider.load_from_string(&format!(
        "#status_view, #commit_view {{font-size: {}px;}}",
        font_size
    ));
    let display = Display::default().expect("cant get dispay");
    style_context_add_provider_for_display(&display, &provider, STYLE_PROVIDER_PRIORITY_USER);
    let font_size_provider = RefCell::new(provider);

    let scheme = settings.get::<String>(SCHEME_TOKEN);
    if !scheme.is_empty() {
        StyleManager::default().set_color_scheme(Scheme::new(scheme).scheme_name());
    }

    let mut status = Status::new(
        initial_path.clone().or_else(|| {
            let last_path = settings.get::<String>("lastpath");
            if !last_path.is_empty() {
                Some(last_path.into())
            } else {
                None
            }
        }),
        sender.clone(),
    );

    let application_window = ApplicationWindow::builder().application(app).build();

    settings
        .bind("width", &application_window, "default-width")
        .build();
    settings
        .bind("height", &application_window, "default-height")
        .build();
    settings
        .bind("is-maximized", &application_window, "maximized")
        .build();
    settings
        .bind("is-fullscreen", &application_window, "fullscreened")
        .build();

    let action_close = gio::SimpleAction::new("close", None);
    action_close.connect_activate({
        let window = application_window.clone();
        move |_, _| {
            window.close();
        }
    });
    application_window.add_action(&action_close);

    let action_open = gio::SimpleAction::new("open", Some(glib::VariantTy::STRING));
    action_open.connect_activate({
        let sender = sender.clone();
        move |_, chosen_path| {
            if let Some(path) = chosen_path {
                let path: String = path.get().expect("cant get path from gvariant");
                sender
                    .send_blocking(Event::OpenRepo(path.into()))
                    .expect("Could not send through channel");
            }
        }
    });

    application_window.add_action(&action_open);

    app.set_accels_for_action("win.close", &["<Ctrl>W"]);

    let (hb, hb_updater) = headerbar_factory(
        sender.clone(),
        settings.clone(),
        &application_window.clone(),
    );

    let txt = stage_factory(sender.clone(), "status_view");

    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .vexpand_set(true)
        .hexpand(true)
        .hexpand_set(true)
        .build();

    scroll.set_child(Some(&status.get_empty_view()));

    let bx = Gtk4Box::builder()
        .hexpand(true)
        .vexpand(true)
        .vexpand_set(true)
        .hexpand_set(true)
        .orientation(Orientation::Vertical)
        .build();
    let banner = Banner::builder().revealed(false).build();
    let revealer = banner.last_child().unwrap();
    let gizmo = revealer.last_child().unwrap();
    let banner_button = gizmo.last_child().unwrap();
    let banner_button_handler_id = banner.connect_button_clicked(|_| {});
    let banner_button_clicked = Rc::new(RefCell::new(Some(banner_button_handler_id)));
    bx.append(&banner);
    bx.append(&scroll);

    let toast_lock: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    let toast_overlay = ToastOverlay::new();
    toast_overlay.set_child(Some(&bx));

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

    application_window.set_content(Some(&tb));

    let mut stage_set = false;
    status.get_status();
    application_window.present();

    let window_stack: Rc<RefCell<Vec<Window>>> = Rc::new(RefCell::new(Vec::new()));

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            let mut ctx = StatusRenderContext::new(&txt);

            match event {
                Event::OpenRepo(path) => {
                    info!("info.open repo {:?}", path);
                    // here could come path selected by the user
                    // this is 'dirty' one. The right path will
                    // came from git with /.git/ suffix
                    // but the 'dirty' path will be used first
                    // for querying repo status and investigate real one
                    // see CurrentRepo event
                    if split.shows_sidebar() {
                        split.set_show_sidebar(false);
                    }
                    status.update_path(path, monitors.clone(), true, &settings);
                    txt.grab_focus();
                    status.get_status();
                }
                Event::Focus => {
                    info!("focus");
                    txt.grab_focus();
                }
                Event::OpenFileDialog => {
                    hb_updater(HbUpdateData::RepoOpen);
                }
                Event::RepoPopup => {
                    hb_updater(HbUpdateData::RepoPopup);
                }
                Event::CurrentRepo(path) => {
                    info!("info.CurrentRepo {:?}", path);
                    if !stage_set {
                        scroll.set_child(Some(&txt));
                        txt.grab_focus();
                        stage_set = true;
                    }
                    hb_updater(HbUpdateData::Path(path.clone()));
                    status.update_path(path, monitors.clone(), false, &settings);
                }
                Event::State(state) => {
                    info!("main. state");
                    status.update_state(state, &txt, &mut ctx);
                }
                Event::OpenEditor => {
                    let args = status.editor_args_at_cursor(&txt);
                    info!("OpenEditor {:?}", args);
                    if let Some((path, line_no, col_no)) = args {
                        external::try_open_editor(path, line_no, col_no);
                    }
                }
                Event::Dump => {
                    info!("Dump");
                }
                Event::Debug => {
                    info!("Debug");
                    status.debug(&txt, &mut ctx);
                }
                Event::Commit => {
                    info!("main.commit");
                    if !status.has_staged() {
                        alert(String::from("No changes were staged. Stage by hitting 's'"))
                            .present(Some(&txt));
                    } else {
                        status.commit(&application_window);
                    }
                }
                Event::Untracked(untracked) => {
                    info!("main. untracked");
                    status.update_untracked(untracked, &txt, &settings, &mut ctx);
                }
                Event::Push => {
                    info!("main.push");
                    hb_updater(HbUpdateData::Push);
                    status.push(&application_window);
                }
                Event::Pull => {
                    info!("main.pull");
                    hb_updater(HbUpdateData::Pull);
                    status.pull(&application_window);
                }
                Event::Branches(branches) => {
                    info!("main. branches");
                    status.update_branches(branches);
                }
                Event::ShowBranches => {
                    info!("main.braches");
                    let path = status.path.clone().unwrap();
                    let w = show_branches_window(
                        path,
                        status.branches.take(),
                        &application_window,
                        sender.clone(),
                    );
                    w.connect_close_request({
                        let window_stack = window_stack.clone();
                        move |_| {
                            info!(
                                "popping stack while close branches {:?}",
                                window_stack.borrow_mut().pop()
                            );
                            glib::signal::Propagation::Proceed
                        }
                    });
                    window_stack.borrow_mut().push(w);
                }
                // Event::TrackChanges(file_path) => {
                //     info!("track file changes {:?}", &file_path);
                //     status.track_changes(file_path, sender.clone());
                // }
                Event::Tags(ooid) => {
                    let oid = ooid.unwrap_or(status.head_oid());
                    let mut remote_name: Option<String> = None;
                    if let Some((o_remote_name, _)) = status.choose_remote_branch_name() {
                        remote_name = o_remote_name;
                    }
                    let current_window = if let Some(stacked_window) = window_stack.borrow().last()
                    {
                        CurrentWindow::Window(stacked_window.clone())
                    } else {
                        CurrentWindow::ApplicationWindow(application_window.clone())
                    };
                    let tags_window = show_tags_window(
                        status.path.clone().expect("no path"),
                        current_window,
                        oid,
                        remote_name,
                        sender.clone(),
                    );
                    tags_window.connect_close_request({
                        let window_stack = window_stack.clone();
                        move |_| {
                            info!(
                                "popping stack while close log {:?}",
                                window_stack.borrow_mut().pop()
                            );
                            glib::signal::Propagation::Proceed
                        }
                    });
                    window_stack.borrow_mut().push(tags_window);
                }
                Event::Log(ooid, obranch_name) => {
                    info!("main.log");
                    let current_window = if let Some(stacked_window) = window_stack.borrow().last()
                    {
                        CurrentWindow::Window(stacked_window.clone())
                    } else {
                        CurrentWindow::ApplicationWindow(application_window.clone())
                    };
                    let log_window = show_log_window(
                        status.path.clone().expect("no path"),
                        current_window,
                        obranch_name.unwrap_or("unknown branch".to_string()),
                        sender.clone(),
                        ooid,
                    );
                    log_window.connect_close_request({
                        let window_stack = window_stack.clone();
                        move |_| {
                            info!(
                                "popping stack while close log {:?}",
                                window_stack.borrow_mut().pop()
                            );
                            glib::signal::Propagation::Proceed
                        }
                    });
                    window_stack.borrow_mut().push(log_window);
                }
                Event::Head(h) => {
                    info!("main. head");
                    if let Some(upstream) = &status.upstream {
                        if let Some(head) = &h {
                            hb_updater(HbUpdateData::Unsynced(head.oid != upstream.oid));
                        }
                    } else {
                        hb_updater(HbUpdateData::Unsynced(true));
                    }
                    status.update_head(h, &txt, &mut ctx);
                }
                Event::UpstreamProgress => {
                    info!("main. UpstreamProgress");
                    hb_updater(HbUpdateData::Upstream);
                }
                Event::Upstream(h) => {
                    info!("main. upstream");
                    hb_updater(HbUpdateData::Upstream);
                    if let (Some(head), Some(upstream)) = (&status.head, &h) {
                        hb_updater(HbUpdateData::Unsynced(head.oid != upstream.oid));
                    }
                    status.update_upstream(h, &txt, &mut ctx);
                }
                Event::Conflicted(odiff, ostate) => {
                    info!("Conflicted");
                    status.update_conflicted(
                        odiff,
                        ostate,
                        &txt,
                        &application_window,
                        sender.clone(),
                        &banner,
                        &banner_button,
                        banner_button_clicked.clone(),
                        &mut ctx,
                    );
                }
                Event::Staged(odiff) => {
                    info!("Staged");
                    hb_updater(HbUpdateData::Staged(odiff.is_some()));
                    status.update_staged(odiff, &txt, &mut ctx);
                }
                Event::Unstaged(odiff) => {
                    info!("Unstaged");
                    status.update_unstaged(odiff, &txt, &mut ctx);
                }
                Event::Expand(offset, line_no) => {
                    trace!("Expand");
                    status.expand(&txt, line_no, offset, &mut ctx);
                }
                Event::Cursor(offset, line_no) => {
                    trace!("Cursor");
                    status.cursor(&txt, line_no, offset, &mut ctx);
                }
                Event::Stage(stage_op) => {
                    info!("Stage {:?}", stage_op);
                    status.stage_op(stage_op, &application_window, &settings);
                }
                Event::TextViewResize(w) => {
                    info!("TextViewResize {}", w);
                }
                Event::TextCharVisibleWidth(w) => {
                    info!("TextCharVisibleWidth {}", w);
                }
                Event::Toast(title) => {
                    info!("Toast {:?}", toast_lock);
                    if !toast_lock.get() {
                        toast_lock.replace(true);
                        let toast = Toast::builder().title(title).timeout(2).build();
                        toast.connect_dismissed({
                            let toast_lock = toast_lock.clone();
                            move |_t| {
                                toast_lock.replace(false);
                            }
                        });
                        toast_overlay.add_toast(toast);
                    }
                }
                Event::Zoom(dir) => {
                    info!("Zoom");
                    let settings = get_settings();
                    let font_size = settings.get::<i32>("zoom") + if dir { 1 } else { -1 };
                    let provider = CssProvider::new();
                    provider.load_from_string(&format!(
                        "#status_view, #commit_view {{font-size: {}px;}}",
                        font_size
                    ));
                    let display = Display::default().expect("cant get dispay");
                    style_context_add_provider_for_display(
                        &display,
                        &provider,
                        STYLE_PROVIDER_PRIORITY_USER,
                    );
                    let old_provider = font_size_provider.replace(provider);
                    style_context_remove_provider_for_display(&display, &old_provider);
                    settings.set("zoom", font_size).expect("cant set settings");
                    // when zoom, TextView become offset from scroll
                    // on some step. this is a hack to force rerender
                    // this pair to allow TextView accomodate whole
                    // width of ScrollView
                    // scroll.set_halign(Align::Start);
                    // glib::source::timeout_add_local(Duration::from_millis(30), {
                    //     let scroll = scroll.clone();
                    //     move || {
                    //         scroll.set_halign(Align::Fill);
                    //         ControlFlow::Break
                    //     }
                    // });
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
                        let (view, focus) = stashes_view_factory(&application_window, &status);
                        split.set_sidebar(Some(&view));
                        split.set_show_sidebar(true);
                        focus();
                    }
                }
                Event::Blame => {
                    info!("blame");
                    let current_window = if let Some(stacked_window) = window_stack.borrow().last()
                    {
                        CurrentWindow::Window(stacked_window.clone())
                    } else {
                        CurrentWindow::ApplicationWindow(application_window.clone())
                    };
                    status.blame(current_window);
                }
                Event::ShowTextOid(short_sha) => {
                    info!("main.show text oid {:?}", txt);
                    glib::spawn_future_local({
                        let path = status.path.clone().unwrap();
                        let current_window =
                            if let Some(stacked_window) = window_stack.borrow().last() {
                                CurrentWindow::Window(stacked_window.clone())
                            } else {
                                CurrentWindow::ApplicationWindow(application_window.clone())
                            };
                        let sender = sender.clone();
                        let window_stack = window_stack.clone();
                        async move {
                            let o_commit_window = match gio::spawn_blocking({
                                let path = path.clone();
                                move || commit::from_short_sha(path, short_sha)
                            })
                            .await
                            .unwrap()
                            {
                                Ok(oid) => {
                                    let commit_window = show_commit_window(
                                        path,
                                        oid,
                                        None,
                                        current_window,
                                        sender.clone(),
                                    );
                                    Some(commit_window)
                                }
                                Err(e) => {
                                    let dialog = alert(format!("{:?}", e));
                                    match current_window {
                                        CurrentWindow::Window(w) => dialog.present(Some(&w)),
                                        CurrentWindow::ApplicationWindow(w) => {
                                            dialog.present(Some(&w))
                                        }
                                    };
                                    None
                                }
                            };
                            if let Some(commit_window) = o_commit_window {
                                commit_window.connect_close_request({
                                    let window_stack = window_stack.clone();
                                    move |_| {
                                        info!(
                                            "popping stack while close commit {:?}",
                                            window_stack.borrow_mut().pop()
                                        );
                                        glib::signal::Propagation::Proceed
                                    }
                                });
                                window_stack.borrow_mut().push(commit_window);
                            }
                        }
                    });
                }
                Event::ShowOid(oid, onum, olineno) => {
                    info!("main.show oid {:?}", oid);
                    let current_window = if let Some(stacked_window) = window_stack.borrow().last()
                    {
                        CurrentWindow::Window(stacked_window.clone())
                    } else {
                        CurrentWindow::ApplicationWindow(application_window.clone())
                    };
                    let commit_window = show_commit_window(
                        status.path.clone().expect("no path"),
                        oid,
                        onum,
                        current_window,
                        sender.clone(),
                    );
                    commit_window.connect_close_request({
                        let window_stack = window_stack.clone();
                        move |_| {
                            info!(
                                "popping stack while close commit {:?}",
                                window_stack.borrow_mut().pop()
                            );
                            glib::signal::Propagation::Proceed
                        }
                    });
                    window_stack.borrow_mut().push(commit_window);
                }
                Event::ResetHard(ooid) => {
                    info!("main. reset hard");
                    status.reset_hard(ooid, &application_window);
                }
                Event::Refresh => {
                    info!("main. refresh");
                    status.get_status();
                }
                Event::CommitDiff(_d) => {
                    panic!("got oid diff in another receiver");
                }
                Event::RemotesDialog => {
                    info!("main. remotes dialog");
                    status.show_remotes_dialog(&application_window);
                }
                Event::LockMonitors(lock) => {
                    info!("main. lock monitors {}", lock);
                    status.lock_monitors(lock);
                }
                Event::StoreSettings(name, value) => {
                    info!("StoreSettings {} {}", name, value);
                    settings.set(&name, value).expect("cant set settings");
                    if name == SCHEME_TOKEN {
                        txt.set_background();
                    }
                }
                Event::Apply(apply_op) => {
                    info!("Apply op: {:?}", apply_op);
                    if let Some(window) = window_stack.borrow().last() {
                        status.apply_op(apply_op, window)
                    } else {
                        status.apply_op(apply_op, &application_window)
                    }
                }
                Event::UserInputRequired(auth_request) => {
                    info!("main. UserInputRequired");
                    if let Some(stack) = window_stack.borrow().last() {
                        auth(auth_request, stack);
                    } else {
                        auth(auth_request, &application_window);
                    }
                }
            };
            hb_updater(HbUpdateData::Context(ctx));
        }
    });
}
