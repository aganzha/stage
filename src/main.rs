use gtk::prelude::*;
use adw::prelude::*;
use adw::{Application, HeaderBar, ApplicationWindow};
use gtk::{glib, gdk, Box, Label, Orientation, TextView, TextBuffer, CssProvider, TextTag};// TextIter
use gdk::Display;


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


    let txt = TextView::builder()
        .build();

    let event_controller = gtk::EventControllerKey::new();
    event_controller.connect_key_pressed(|_, key, _, _| {
        println!("==========================> {:?}", key);
        match key {
            gdk::Key::Tab => {
                println!("taaaaaaaaaaaaaaaaaaaaaaaaaaaaaab!")
            },
            gdk::Key::s => {
                println!("ssssssssssssssssssssssssssssssssssss!")
            },
            _ => (),
        }
        glib::Propagation::Proceed
    });
    let gesture = gtk::GestureClick::new();
    gesture.connect_released(|gesture, _, _, _| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        println!("Box pressed!");
    });
    txt.add_controller(event_controller);
    txt.add_controller(gesture);

    let tag = TextTag::new(Some("highlight"));
    let tc = tag.clone();
    tag.set_background(Some("#f6fecd"));

    txt.connect_move_cursor(move |view, step, count, _selection| {
        let buffer = view.buffer();
        let pos = buffer.cursor_position();
        let mut iter = buffer.iter_at_offset(pos);
        let mut clear_highlight = false;
        match step {
            gtk::MovementStep::LogicalPositions |
            gtk::MovementStep::VisualPositions => {
                iter.forward_chars(count);
                clear_highlight = !iter.has_tag(&tc);
            },
            gtk::MovementStep::Words => {
                iter.forward_word_end();
                clear_highlight = !iter.has_tag(&tc);
            },
            gtk::MovementStep::DisplayLines |
            gtk::MovementStep::DisplayLineEnds |
            gtk::MovementStep::Paragraphs |
            gtk::MovementStep::ParagraphEnds => {
                iter.forward_lines(count);
                clear_highlight = !iter.has_tag(&tc);
            },
            gtk::MovementStep::Pages |
            gtk::MovementStep::BufferEnds |
            gtk::MovementStep::HorizontalPages => {
                clear_highlight = true;
            },
            _ => todo!()
        }
        let start_mark = buffer.mark("start_highlight").unwrap();
        let end_mark = buffer.mark("end_highlight").unwrap();
        println!("Soooooooooooooooooooo ? {:?}", clear_highlight);
        if clear_highlight {
            buffer.remove_tag(
                &tc,
                &buffer.iter_at_mark(&start_mark),
                &buffer.iter_at_mark(&end_mark)
            );
            //let end_iter = buffer.iter_at_offset(iter)
            println!("REEEEMOOOOVE TAG");
        }
    });
    let lorem: &str = "Untracked files (1)
src/style.css

Recent commits
a2959bf master origin/master begin textview
7601dc7 added adwaita
c622f7f init";

    let buffer = txt.buffer();
    buffer.set_text(&lorem);

    buffer.tag_table().add(&tag);
    txt.set_monospace(true);
    txt.set_editable(false);

    buffer.place_cursor(&buffer.iter_at_offset(0));


    let start_iter = buffer.iter_at_offset(0);
    buffer.create_mark(Some("start_highlight"), &start_iter, false);
        
    let mut end_iter = buffer.iter_at_offset(0);
    end_iter.forward_to_line_end();
    buffer.create_mark(Some("end_highlight"), &end_iter, false);    
    
    buffer.apply_tag(&tag, &start_iter, &end_iter);
    println!("ADDED TAG {:?} : {:?} end offset", start_iter.offset(), end_iter.offset());
    stage.append(&txt);

    window.set_content(Some(&stage));

    window.present();
}
