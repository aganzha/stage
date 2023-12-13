mod text_view;
use text_view::{text_view_factory, render};
mod git;
use git::{get_current_repo_status, stage_changes};


use gtk::prelude::*;
use adw::prelude::*;
use glib::{MainContext, Priority};
use adw::{Application, HeaderBar, ApplicationWindow};
use gtk::{glib, gdk, gio, Box, Label, Orientation, CssProvider};// TextIter
use gdk::Display;
use git2::Repository;


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
    Stage
        
}

fn build_ui(app: &adw::Application) {

    let window = ApplicationWindow::new(app);
    window.set_default_size(640, 480);

    let stage = Box::builder()
        .build();
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

    let txt = text_view_factory();

    stage.append(&txt);

    window.set_content(Some(&stage));

    window.present();

    let mut repo: Option<std::ffi::OsString> = None;
    let (sender, receiver) = MainContext::channel(Priority::default());

    gio::spawn_blocking({
        let sender = sender.clone();
        move || {
            get_current_repo_status(sender);
        }
    });

    receiver.attach(
        None,
        move |event: Event| {
            match event {
                Event::CurrentRepo(path) => {
                    if repo.is_none() {
                        // need cleanup everything
                    }
                    repo.replace(path);
                },
                Event::Stage => {                    
                }
            };
            glib::ControlFlow::Continue
        }
    );
//     let lorem: &str = "
// Untracked files (1)
// src/style.css

// Recent commits
// a2959bf master origin/master begin textview
// 7601dc7 added adwaita
// c622f7f init";

//     render(txt, lorem);
//     let mut path_buff = env::current_exe().unwrap();


//     println!("--------------------> {:?}", path_buff);
//     let path = path_buff.as_path();
//     let repo = Repository::open(path).unwrap();
//     println!("UUUUUUUUUUUUUUUUUU-> {:?} {:?}", repo.is_empty(), path);

}
