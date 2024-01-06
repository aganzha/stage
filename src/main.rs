mod text_view;
use text_view::{cursor, debug, expand, render_status, stage, text_view_factory, Status};
mod common_tests;
mod git;
use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar};
use gdk::Display;
use git::{
    get_current_repo_status, stage_via_apply, ApplyFilter, Diff, File, Hunk, Line, LineKind, View,
};
use glib::{clone, MainContext, Priority};
use gtk::prelude::*;
use gtk::{gdk, gio, glib, Box, CssProvider, Label, Orientation, ScrolledWindow}; // TextIter

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
}

fn build_ui(app: &adw::Application) {
    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);
    //window.set_default_size(640, 480);
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
            get_current_repo_status(sender);
        }
    });

    receiver.attach(None, move |event: Event| {
        match event {
            Event::CurrentRepo(path) => {
                current_repo_path.replace(path);
            }
            Event::Debug => {
                println!("main. FAKE");
                debug(&txt, &mut status);
            }
            Event::Staged(d) => {
                println!("main. staged {:p}", &d);
                status.staged.replace(d);
                if status.staged.is_some() && status.unstaged.is_some() {
                    render_status(&txt, &mut status, sender.clone());
                }
            }
            Event::Unstaged(d) => {
                println!("main. unstaged {:p}", &d);
                status.unstaged.replace(d);
                if status.staged.is_some() && status.unstaged.is_some() {
                    render_status(&txt, &mut status, sender.clone());
                }
            }
            Event::Expand(offset, line_no) => {
                expand(&txt, &mut status, offset, line_no, sender.clone());
            }
            Event::Cursor(offset, line_no) => {
                cursor(&txt, &mut status, offset, line_no, sender.clone());
            }
            Event::Stage(offset, line_no) => {
                stage(
                    &txt,
                    &mut status,
                    offset,
                    line_no,
                    current_repo_path.as_ref().unwrap(),
                    true,
                    sender.clone(),
                );
                println!("STAGE THIS TEXT {:?} in {:?}", offset, line_no);
            }
            Event::UnStage(offset, line_no) => {
                stage(
                    &txt,
                    &mut status,
                    offset,
                    line_no,
                    current_repo_path.as_ref().unwrap(),
                    false,
                    sender.clone(),
                );
                println!("STAGE THIS TEXT {:?} in {:?}", offset, line_no);
            }
        };
        glib::ControlFlow::Continue
    });
    window.present();
}
