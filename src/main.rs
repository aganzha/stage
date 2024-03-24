mod status_view;
use status_view::{factory::text_view_factory, Status, StatusRenderContext};

mod branches_view;
use branches_view::{show_branches_window, Event as BranchesEvent};

use std::cell::RefCell;
use std::rc::Rc;

mod git;
use git::{
    checkout, cherry_pick, commit, create_branch, get_current_repo_status,
    get_refs, kill_branch, push, stage_via_apply, ApplyFilter, ApplySubject,
    BranchData, Diff, DiffKind, File, Head, Hunk, Line, Related, State, View,
};
mod widgets;
use widgets::{display_error, get_new_branch_name, make_confirm_dialog};

use libadwaita::prelude::*;
use libadwaita::{Application, ApplicationWindow, HeaderBar, ToolbarView};

use gdk::Display;

use glib::{clone, ControlFlow};

use gtk4::{
    gdk, gio, glib, style_context_add_provider_for_display, Button,
    CssProvider, ScrolledWindow, STYLE_PROVIDER_PRIORITY_APPLICATION,
    TextWindowType
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
    // Load the CSS file and add it to the provider
    // let adw_theme = IconTheme::builder()
    //     .display()
    //     .theme_name("Adwaita")
    //     .build();
    let provider = CssProvider::new();
    let display = Display::default().expect("Could not connect to a display.");
    provider.load_from_string(include_str!("style.css"));

    // Add the provider to the default screen
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
    Commit,
    Push,
    Branches,
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
        .icon_name("view-refresh")
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

    let txt = text_view_factory(sender.clone());

    let text_view_width = Rc::new(RefCell::<(i32, i32)>::new((0, 0)));
    let txt_bounds =
        Rc::new(RefCell::<(i32, i32, i32, i32)>::new((0, 0, 0, 0)));
    txt.add_tick_callback({
        let text_view_width = text_view_width.clone();
        let txt_bounds = txt_bounds.clone();
        move |view, _clock| {
            // debug!("add tick callback -------------> {:?}", view.bounds());
            let width = view.width();
            if width > (*text_view_width.borrow()).0 {
                text_view_width.replace((width, 0));
                info!(".. {:?}", text_view_width);
                if let Some((mut iter, _over_text)) = view.iter_at_position(1, 1) {
                    let buff = iter.buffer();
                    iter.forward_to_line_end();
                    let mut pos = view.cursor_locations(Some(&iter)).0.x();
                    while pos < width {
                        info!("add chars one by one and pos is {:?}", pos);
                        buff.insert(&mut iter, " ");
                        pos = view.cursor_locations(Some(&iter)).0.x();
                    }
                    debug!("GOT MAX POS {:?} and iter offset {:?}", pos, iter.offset());
                    text_view_width.replace((width, iter.offset()));
                }
            }
            // let bounds = *txt_bounds.borrow();
            // // debug!("bbbbbbbbbbbbounds before match {:p} {:?}", &txt_bounds, txt_bounds);
            // match (bounds, view.bounds()) {
            //     ((0, 0, 0, 0), None) => (),
            //     ((0, 0, 0, 0), Some((x, y, width, height))) =>{
            //         debug!("got bounds first time ............{:?} {:?} {:?} {:?}", x, y, width, height);
            //         if x > 0 && y > 0 && width > 0 && height > 0 {
            //             txt_bounds.replace((x, y, width, height));
            //             if let Some((mut iter, over_text)) = view.iter_at_position(1, 1) {
            //                 let buff = iter.buffer();
            //                 iter.forward_to_line_end();
            //                 let mut pos = view.cursor_locations(Some(&iter)).0.x();
            //                 debug!("iter at 0,0  ------.--=>>>> {:?}. over over_text {:?}", iter.offset(), over_text);
            //                 debug!("cursor locations 1 {:}", view.cursor_locations(Some(&iter)).0.x());
            //                 buff.insert(&mut iter, "A");
            //                 debug!("cursor locations 2 {:}", view.cursor_locations(Some(&iter)).0.x());
            //                 buff.insert(&mut iter, "B");
            //                 debug!("cursor locations 3 {:}", view.cursor_locations(Some(&iter)).0.x());
            //                 buff.insert(&mut iter, "C");
            //                 debug!("cursor locations 4 {:}", view.cursor_locations(Some(&iter)).0.x());

            //             } else {
            //                 debug!("NOOOOOOOOOOOOOOOOOOOOO WAY!");
            //             }
            //             // i have a screen size here in txt_bounds. but cant got any iter
            //             // because iter Could be only inside rendered text, not whute space.
            //             // let (buff_x, buff_y) = view. window_to_buffer_coords(
            //             //     TextWindowType::Text,
            //             //     width,
            //             //     height
            //             // );
            //             // let iter = view.iter_at_location(1207, 140);// 1207, 157 works!
            //             // debug!("hey inside ---- {:p} {:?}. buff x and y {:?} {:?} iter======================={:?}",
            //             //        &txt_bounds,
            //             //        txt_bounds,
            //             //        buff_x,
            //             //        buff_y,
            //             //        iter);
            //         }
            //         // bounds.replace((x, y, width, height));
            //        // debug!("????????????????? bounds after ---- {:p} {:?}", &bounds, &bounds);
            //     }
            //     _ => {}
            // }
            ControlFlow::Continue
        }
    });

    let scroll = ScrolledWindow::new();
    scroll.set_child(Some(&txt));

    let tb = ToolbarView::builder().content(&scroll).build();
    tb.add_top_bar(&hb);

    window.set_content(Some(&tb));

    env_logger::builder().format_timestamp(None).init();

    status.get_status(current_repo_path.clone(), sender.clone());
    window.present();

    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            let mut ctx = StatusRenderContext::new();
            ctx.screen_width.replace(*text_view_width.borrow());
            ctx.screen_bounds.replace(*txt_bounds.borrow());
            status.context.replace(ctx);
            debug!(
                "main loop >>>>>>>>>>> {:?} {:?}",
                status.context, text_view_width
            );
            debug!("");
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
            };

            // debug!(
            //     "-----------------------outer match ------------------- {:?}",
            //     &status.context
            // );
            // status.context.take();
        }
    });
}
