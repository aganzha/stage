// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later

mod external;
mod status_view;
use status_view::{
    context::StatusRenderContext,
    headerbar::factory as headerbar_factory,
    headerbar::{HbUpdateData, Scheme, SCHEME_TOKEN},
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

use core::time::Duration;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

mod git;
use git::{
    branch, commit, get_current_repo_status, get_directories, reset_hard,
    stage_untracked, stage_via_apply, stash::Stashes, track_changes, Diff,
    DiffKind, File, Head, Hunk, HunkLineNo, Line, LineKind, State,
};
use git2::Oid;
mod dialogs;
use dialogs::{alert, confirm_dialog_factory};

mod tests;
use gdk::Display;
use glib::{clone, ControlFlow};
use libadwaita::prelude::*;
use libadwaita::{
    Application, ApplicationWindow, Banner, OverlaySplitView, StyleManager,
    Toast, ToastOverlay, ToolbarStyle, ToolbarView, Window,
};

use gtk4::{
    gdk, gio, glib, style_context_add_provider_for_display, Align, Box,
    CssProvider, Orientation, ScrolledWindow, Settings,
    STYLE_PROVIDER_PRIORITY_USER,
};

use log::{info, trace};
use regex::Regex;

const APP_ID: &str = "com.github.aganzha.stage";

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|_| load_css());

    let initial_path: Rc<RefCell<Option<PathBuf>>> =
        Rc::new(RefCell::new(None));

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

fn load_css() {
    let display = Display::default().expect("Could not connect to a display.");
    let settings = Settings::for_display(&display);
    let stored_settings = get_settings();
    let stored_font_size = stored_settings.get::<i32>("zoom");
    settings
        .set_gtk_font_name(Some(&format!("Cantarell {}", stored_font_size)));

    let provider = CssProvider::new();

    provider.load_from_string(include_str!("style.css"));

    style_context_add_provider_for_display(
        &display,
        &provider,
        STYLE_PROVIDER_PRIORITY_USER,
    );
}

#[derive(Debug, Clone)]
pub enum StageOp {
    Stage(i32),
    Unstage(i32),
    Kill(i32),
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
    TrackedFile(PathBuf, Diff),
    Staged(Option<Diff>),
    Head(Head),
    Upstream(Option<Head>),
    State(State),
    OpenFileDialog,
    RepoPopup,
    Expand(i32, i32),
    Cursor(i32, i32),
    CopyToClipboard(i32, i32),
    Stage(StageOp),
    // Stage(i32, i32),
    // UnStage(i32, i32),
    // Kill(i32, i32),
    Ignore(i32, i32),
    Commit,
    Push,
    Pull,
    ShowBranches,
    Branches(Vec<branch::BranchData>),
    Log(Option<Oid>, Option<String>),
    ShowOid(Oid, Option<usize>),
    TextViewResize(i32),
    TextCharVisibleWidth(i32),
    Toast(String),
    StashesPanel,
    Stashes(Stashes),
    Refresh,
    Zoom(bool),
    ResetHard(Option<Oid>),
    CommitDiff(commit::CommitDiff),
    PushUserPass(String, bool),
    PullUserPass,
    LockMonitors(bool),
    StoreSettings(String, String),
    OpenEditor,
    Tags(Option<Oid>),
    TrackChanges(PathBuf),
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
        } else if int_size > 1 {
            int_size -= 1;
        }
        settings.set_gtk_font_name(Some(&format!("Cantarell {}", int_size)));
        let settings = get_settings();
        settings.set("zoom", int_size).expect("cant set settings")
    };
}

pub fn get_settings() -> gio::Settings {
    let mut exe_path = std::env::current_exe().expect("cant get exe path");
    exe_path.pop();
    let exe_path = exe_path.as_path();
    let schema_source =
        gio::SettingsSchemaSource::from_directory(exe_path, None, true)
            .expect("no source");
    let schema = schema_source.lookup(APP_ID, false).expect("no schema");
    gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None)
}

fn run_app(app: &Application, initial_path: &Option<PathBuf>) {
    env_logger::builder().format_timestamp(None).init();

    let (sender, receiver) = async_channel::unbounded();
    let monitors = Rc::new(RefCell::<Vec<gio::FileMonitor>>::new(Vec::new()));

    let settings = get_settings();

    let scheme = settings.get::<String>(SCHEME_TOKEN);
    if !scheme.is_empty() {
        StyleManager::default()
            .set_color_scheme(Scheme::new(scheme).scheme_name());
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
        settings.clone(),
        sender.clone(),
    );

    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(1280)
        .default_height(480)
        //.css_classes(vec!["devel"])
        .build();

    let action_close = gio::SimpleAction::new("close", None);
    action_close.connect_activate(clone!(@weak window => move |_, _| {
        window.close();
    }));
    window.add_action(&action_close);

    let action_open =
        gio::SimpleAction::new("open", Some(glib::VariantTy::STRING)); //
    action_open.connect_activate(clone!(@strong sender => move |_, chosen_path| {
        if let Some(path) = chosen_path {
            let path:String = path.get().expect("cant get path from gvariant");
            sender.send_blocking(Event::OpenRepo(path.into()))
                                .expect("Could not send through channel");
        }
    }));
    window.add_action(&action_open);

    app.set_accels_for_action("win.close", &["<Ctrl>W"]);

    let (hb, hb_updater) = headerbar_factory(sender.clone(), settings.clone()); // TODO! remove/

    // what about changing color_scheme from gnome settings?
    let txt = stage_factory(sender.clone(), "status_view");

    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .vexpand_set(true)
        .hexpand(true)
        .hexpand_set(true)
        .build();

    scroll.set_child(Some(&status.get_empty_view()));

    let bx = Box::builder()
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
    let banner_button_clicked =
        Rc::new(RefCell::new(Some(banner_button_handler_id)));
    bx.append(&banner);
    bx.append(&scroll);

    let toast_lock: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    let toast_overlay = ToastOverlay::new();
    toast_overlay.set_child(Some(&bx)); // scroll bs bx

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

    let mut stage_set = false;
    status.get_status();
    window.present();

    let window_stack: Rc<RefCell<Vec<Window>>> =
        Rc::new(RefCell::new(Vec::new()));

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            // context is updated on every render
            let mut ctx = StatusRenderContext::new();

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
                    status.update_path(path, monitors.clone(), true);
                    txt.grab_focus();
                    status.get_status();
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
                    status.update_path(path, monitors.clone(), false);
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
                    status.dump(&txt, &mut ctx);
                }
                Event::Debug => {
                    info!("Debug");
                    status.debug(&txt, &mut ctx);
                    // let new_snapshot = Snapshot::new();
                    // new_snapshot.append_color(
                    //     &gdk::RGBA::new(0.0, 0.0, 0.0, 1.0),
                    //     &graphene::Rect::new(0.0, 0.0, 100.0, 100.0)
                    // );
                    // new_snapshot.pop();
                    // scroll.snapshot_child(&txt, &new_snapshot);
                    // txt.snapshot_layer();
                    // info!("meeeeeeeeeeeeeeeeeeeeeeeeeeee");
                }
                Event::Commit => {
                    info!("main.commit");
                    if !status.has_staged() {
                        alert(String::from(
                            "No changes were staged. Stage by hitting 's'",
                        ))
                        .present(&txt);
                    } else {
                        status.commit(&window);
                    }
                }
                Event::Untracked(untracked) => {
                    info!("main. untracked");
                    status.update_untracked(untracked, &txt, &mut ctx);
                }
                Event::Push => {
                    info!("main.push");
                    status.push(&window, None);
                }
                Event::Pull => {
                    info!("main.pull");
                    status.pull(&window, None);
                }
                Event::Branches(branches) => {
                    info!("main. branches");
                    status.update_branches(branches);
                }
                Event::ShowBranches => {
                    info!("main.braches");
                    let w = show_branches_window(
                        status.path.clone().expect("no path"),
                        status.branches.take(),
                        &window,
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
                Event::TrackChanges(file_path) => {
                    info!("track file changes {:?}", &file_path);
                    status.track_changes(file_path, sender.clone());
                }
                Event::Tags(ooid) => {
                    let oid = ooid.unwrap_or(status.head_oid());
                    let w = {
                        if let Some(stack) = window_stack.borrow().last() {
                            show_tags_window(
                                status.path.clone().expect("no path"),
                                stack,
                                oid,
                                sender.clone(),
                            )
                        } else {
                            show_tags_window(
                                status.path.clone().expect("no path"),
                                &window,
                                oid,
                                sender.clone(),
                            )
                        }
                    };
                    w.connect_close_request({
                        let window_stack = window_stack.clone();
                        move |_| {
                            info!(
                                "popping stack while close log {:?}",
                                window_stack.borrow_mut().pop()
                            );
                            glib::signal::Propagation::Proceed
                        }
                    });
                    window_stack.borrow_mut().push(w);
                }
                Event::Log(ooid, obranch_name) => {
                    info!("main.log");
                    let w = {
                        if let Some(stack) = window_stack.borrow().last() {
                            show_log_window(
                                status.path.clone().expect("no path"),
                                stack,
                                obranch_name
                                    .unwrap_or("unknown branch".to_string()),
                                sender.clone(),
                                ooid,
                            )
                        } else {
                            show_log_window(
                                status.path.clone().expect("no path"),
                                &window,
                                status.head_name(),
                                sender.clone(),
                                ooid,
                            )
                        }
                    };
                    w.connect_close_request({
                        let window_stack = window_stack.clone();
                        move |_| {
                            info!(
                                "popping stack while close log {:?}",
                                window_stack.borrow_mut().pop()
                            );
                            glib::signal::Propagation::Proceed
                        }
                    });
                    window_stack.borrow_mut().push(w);
                }
                Event::Head(h) => {
                    info!("main. head");
                    if let Some(upstream) = &status.upstream {
                        hb_updater(HbUpdateData::Unsynced(
                            h.oid != upstream.oid,
                        ));
                    } else {
                        hb_updater(HbUpdateData::Unsynced(true));
                    }
                    status.update_head(h, &txt, &mut ctx);
                }
                Event::Upstream(h) => {
                    info!("main. upstream");
                    if let (Some(head), Some(upstream)) = (&status.head, &h) {
                        hb_updater(HbUpdateData::Unsynced(
                            head.oid != upstream.oid,
                        ));
                    }
                    status.update_upstream(h, &txt, &mut ctx);
                }
                Event::Conflicted(odiff, ostate) => {
                    info!("Conflicted");
                    // hb_updater(HbUpdateData::Staged(!d.files.is_empty()));
                    // if let Some(state) = ostate {
                    //     status.update_state(state, &txt, &mut ctx);
                    // }
                    status.update_conflicted(
                        odiff,
                        ostate,
                        &txt,
                        &window,
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
                Event::TrackedFile(file_path, diff) => {
                    info!("Unstaged");
                    status
                        .update_tracked_file(file_path, diff, &txt, &mut ctx);
                }
                Event::Expand(offset, line_no) => {
                    info!("Expand");
                    status.expand(&txt, line_no, offset, &mut ctx);
                }
                Event::Cursor(offset, line_no) => {
                    trace!("Cursor");
                    status.cursor(&txt, line_no, offset, &mut ctx);
                }
                Event::CopyToClipboard(start_offset, end_offset) => {
                    info!("CopyToClipboard");
                    status.copy_to_clipboard(
                        &txt,
                        start_offset,
                        end_offset,
                        &mut ctx,
                    );
                }
                Event::Stage(stage_op) => {
                    info!("Stage {:?}", stage_op);
                    status.stage(stage_op, &window);
                }
                Event::Ignore(offset, line_no) => {
                    info!("main.ignore");
                    status.ignore(&txt, line_no, offset, &mut ctx);
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
                        let toast =
                            Toast::builder().title(title).timeout(2).build();
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
                    zoom(dir);
                    // when zoom, TextView become offset from scroll
                    // on some step. this is a hack to force rerender
                    // this pair to allow TextView accomodate whole
                    // width of ScrollView
                    status.resize_highlights(&txt, &mut ctx);
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
                        let (view, focus) =
                            stashes_view_factory(&window, &status);
                        split.set_sidebar(Some(&view));
                        split.set_show_sidebar(true);
                        focus();
                    }
                }
                Event::ShowOid(oid, num) => {
                    info!("main.show oid {:?}", oid);
                    if let Some(stack) = window_stack.borrow().last() {
                        show_commit_window(
                            status.path.clone().expect("no path"),
                            oid,
                            num,
                            stack,
                            sender.clone(),
                        );
                    } else {
                        show_commit_window(
                            status.path.clone().expect("no path"),
                            oid,
                            num,
                            &window,
                            sender.clone(),
                        );
                    }
                }
                Event::ResetHard(ooid) => {
                    info!("main. reset hard");
                    status.reset_hard(ooid, &window);
                }
                Event::Refresh => {
                    info!("main. refresh");
                    status.get_status();
                }
                Event::CommitDiff(_d) => {
                    panic!("got oid diff in another receiver");
                }
                Event::PushUserPass(remote, tracking) => {
                    status.push(&window, Some((remote, tracking, true)))
                }
                Event::PullUserPass => {
                    // does it used? yes.
                    // just pulling via ssh.
                    info!("main. userpass");
                    status.pull(&window, Some(true))
                }
                Event::LockMonitors(lock) => {
                    info!("main. lock monitors {}", lock);
                    status.lock_monitors(lock);
                }
                Event::StoreSettings(name, value) => {
                    info!("StoreSettings {} {}", name, value);
                    settings.set(&name, value).expect("cant set settings");
                    if name == SCHEME_TOKEN {
                        txt.set_is_dark(
                            StyleManager::default().is_dark(),
                            true,
                        );
                    }
                }
            };
            // no way. it need to choose in each place...
            // status.choose_cursor_position(&txt, &mut ctx);
        }
    });
}
