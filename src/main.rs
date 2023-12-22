mod text_view;
use text_view::{text_view_factory, render};
mod git;
use git::{get_current_repo_status, stage_changes, Diff, LineKind, View};
use core::num::NonZeroU32;

use gtk::prelude::*;
use adw::prelude::*;
use glib::{MainContext, Priority, subclass::Signal, subclass::signal::SignalId};
use adw::{Application, HeaderBar, ApplicationWindow};
use gtk::{glib, gdk, gio, Box, Label, Orientation, CssProvider, ScrolledWindow};// TextIter
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
    Status(Diff),
    Expand(i32),
    Stage(String)
}

fn build_ui(app: &adw::Application) {

    let window = ApplicationWindow::new(app);
    window.set_default_size(1280, 960);

    let scroll = ScrolledWindow::new();

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
                Event::Status(d) => {
                    println!("git diff in status {:p}", &d);
                    diff.replace(d);
                    let mut d = diff.as_mut().unwrap();
                    // let cl = d.clone();
                    // println!("replaced it with clone! {:p}", &cl);
                    // diff.replace(cl);
                    // let cl = d.clone();
                    // println!("one more clone to render! {:p}", &cl);
                    render(&txt, d, sender.clone());// , txt_signal
                },
                Event::Expand(line_no) => {
                    println!("Expand {:?}", line_no);
                    let mut d = diff.as_mut().unwrap();
                    // let mut d = diff.clone().unwrap();
                    println!("thats d in expand {:p}, was before expand {:?}", &d, d.tmp);
                    println!("line number in first file {:?}", d.files[1].view.as_ref().unwrap().line_no);
                    d.set_expand(line_no);                    
                    // 
                    // let cl = d.clone();
                    // println!("re render it again with clone {:p}", &cl); 
                    // render(&txt, cl, sender.clone());
                }
                Event::Stage(text) => {
                    println!("STAGE THIS TEXT {:?} in {:?}", text, diff);
                }
            };
            glib::ControlFlow::Continue
        }
    );
    window.present();

}
