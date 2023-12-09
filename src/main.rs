use gtk::prelude::*;
use adw::prelude::*;
use adw::{Application, HeaderBar, ApplicationWindow};
use gtk::{glib, Box, Label, Orientation, TextView};

const APP_ID: &str = "io.github.aganzha.Stage";

fn main() -> glib::ExitCode {

    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(build_ui);

    app.run()
}

fn build_ui(app: &adw::Application) {

    let window = ApplicationWindow::new(app);
    window.set_default_size(640, 480);

    let stage = Box::builder()
        .build();
    stage.set_orientation(Orientation::Vertical);
    let hb = HeaderBar::new();
    let lbl = Label::builder()
        .label("stage")
        .single_line_mode(true)
        .width_chars(5)
        .build();
    hb.set_title_widget(Some(&lbl));
    stage.append(&hb);


    let lbl = Label::builder()
        .label("ou")
        .selectable(true)
        .single_line_mode(true)
        .width_chars(5)
        .build();
    stage.append(&lbl);

    let txt = TextView::builder()
        .build();
    let s = String::from("Hello. How is it going? \n");
    txt.buffer().set_text(&s);
    stage.append(&txt);

    let lbl = Label::builder()
        .label("bou")
        .selectable(true)
        .single_line_mode(true)
        .width_chars(5)
        .build();
    stage.append(&lbl);

    window.set_content(Some(&stage));  

    window.present();
}
