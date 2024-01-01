mod text_view;
use text_view::{cursor, expand, render_head, render, text_view_factory};
mod git;
use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar};
use gdk::Display;
use git::{get_current_repo_status, Diff, File, Hunk, Line, LineKind, View, DiffView};
use glib::{MainContext, Priority};
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
    CurrentRepo(std::ffi::OsString),
    Status(Diff),
    Expand(i32, i32),
    Cursor(i32, i32),
    // does not used for now
    Stage(i32, i32),
}

fn build_ui(app: &adw::Application) {
    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);
    //window.set_default_size(640, 480);
    let scroll = ScrolledWindow::new();

    let stage = Box::builder().build();
    stage.set_orientation(Orientation::Vertical);
    stage.add_css_class("stage");
    let hb = HeaderBar::new();
    let lbl = Label::builder()
        .label("stage")
        .single_line_mode(true)
        .width_chars(5)
        .build();
    hb.set_title_widget(Some(&lbl));
    stage.append(&hb);

    let (sender, receiver) = MainContext::channel(Priority::default());

    let txt = text_view_factory(sender.clone());

    scroll.set_min_content_height(960);
    scroll.set_max_content_height(960);
    scroll.set_child(Some(&txt));

    stage.append(&scroll);

    window.set_content(Some(&stage));

    let mut repo: Option<std::ffi::OsString> = None;
    let mut diff: Option<Diff> = None;

    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            get_current_repo_status(sender);
        }
    });

    receiver.attach(None, move |event: Event| {
        match event {
            Event::CurrentRepo(path) => {
                if repo.is_none() {
                    // need cleanup everything
                }
                repo.replace(path);
            }
            Event::Status(d) => {
                println!("git diff in status {:p}", &d);
                diff.replace(d);
                let d = diff.as_mut().unwrap();
                render_head(&txt, d, sender.clone());
                render(&txt, d, sender.clone());
            }
            Event::Expand(offset, line_no) => {
                let d = diff.as_mut().unwrap();
                expand(&txt, d, offset, line_no, sender.clone());
            }
            Event::Cursor(offset, line_no) => {
                let d = diff.as_mut().unwrap();
                cursor(&txt, d, offset, line_no, sender.clone());
            }
            Event::Stage(offset, line_no) => {
                println!("STAGE THIS TEXT {:?} in {:?}", offset, line_no);
            }
        };
        glib::ControlFlow::Continue
    });
    window.present();
}
