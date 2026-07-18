// SPDX-FileCopyrightText: 2026 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Diff, Event, Hunk, ViewContainer};
use async_channel::Sender;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, SearchBar, SearchEntry};

use regex::Regex;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Search {
    pub current_lineno: Rc<Cell<Option<i32>>>,
    pub matched_lines: Rc<RefCell<Vec<i32>>>,
    sender: Sender<Event>,
    label: Label,
    search_entry: SearchEntry,
    pub search_bar: SearchBar,
}

impl Search {
    fn new(
        search_bar: SearchBar,
        search_entry: SearchEntry,
        label: Label,
        sender: Sender<Event>,
    ) -> Self {
        search_bar.connect_search_mode_enabled_notify({
            let label = label.clone();
            let search_entry = search_entry.clone();
            let sender = sender.clone();
            move |search_bar| {
                if !search_bar.is_search_mode() {
                    label.set_label("");
                    search_entry.set_text("");
                    sender
                        .send_blocking(Event::Focus)
                        .expect("cant send through channel")
                }
            }
        });

        Search {
            current_lineno: Rc::new(Cell::new(None)),
            matched_lines: Rc::new(RefCell::new(Vec::new())),
            sender,
            label,
            search_entry,
            search_bar,
        }
    }
    pub fn update(&self) {
        if let Some(line) = self.current_lineno.get() {
            let lines = self.matched_lines.borrow();
            if let Some(idx) = lines.iter().position(|l| l == &line) {
                self.label
                    .set_label(&format!("{}({})", idx + 1, lines.len()));
            }
        } else {
            self.label.set_label("");
        }
    }

    pub fn toggle(&self, value: bool) {
        self.search_bar.set_search_mode(value);
        if value {
            self.search_entry.grab_focus();
        }
    }

    pub fn forward(&self) {
        let current_line = self.current_lineno.get().unwrap_or(0);
        if let Some(scroll_to) = self
            .matched_lines
            .borrow()
            .iter()
            .filter(|l| l > &&current_line)
            .min()
            .copied()
        {
            self.current_lineno.replace(Some(scroll_to));
            self.sender
                .send_blocking(Event::GoToLine(scroll_to))
                .expect("cant send through channel");
        }
        self.update();
    }

    fn backward(&self) {
        let current_line = self.current_lineno.get().unwrap_or(0);
        if let Some(scroll_to) = self
            .matched_lines
            .borrow()
            .iter()
            .filter(|l| l < &&current_line)
            .max()
            .copied()
        {
            self.current_lineno.replace(Some(scroll_to));
            self.sender
                .send_blocking(Event::GoToLine(scroll_to))
                .expect("cant send through channel");
        }
        self.update();
    }
}

pub fn make_search(sender: Sender<Event>) -> Search {
    let search_entry = SearchEntry::builder()
        .search_delay(800)
        .hexpand(true)
        .placeholder_text("search in changes")
        .build();
    search_entry.connect_search_changed({
        let sender = sender.clone();
        move |entry| {
            let term: String = entry.text().into();
            if term.is_empty() {
                sender
                    .send_blocking(Event::ResetSearch)
                    .expect("cant send throug channel");
                return;
            }
            if let Ok(regex) = Regex::new(&term) {
                sender
                    .send_blocking(Event::Search(regex))
                    .expect("cant send throug channel");
            }
        }
    });
    let search_box = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .build();
    let lbl = Label::builder()
        .width_chars(6)
        .max_width_chars(6)
        .xalign(0.5)
        .build();

    let backward = Button::builder()
        .sensitive(true)
        .icon_name("go-up-symbolic")
        .build();
    let forward = Button::builder()
        .sensitive(true)
        .icon_name("go-down-symbolic")
        .build();
    search_box.append(&search_entry);
    search_box.append(&lbl);
    search_box.append(&backward);
    search_box.append(&forward);

    let search_bar = SearchBar::builder()
        .child(&search_box)
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(true)
        .build();
    search_bar.set_search_mode(false);

    let search = Search::new(search_bar, search_entry, lbl, sender.clone());

    forward.connect_clicked({
        let search = search.clone();
        move |_btn| {
            search.forward();
        }
    });
    backward.connect_clicked({
        let search = search.clone();
        move |_btn| {
            search.backward();
        }
    });

    search
}

impl Hunk {
    pub fn reset_search(&mut self) -> bool {
        let had_ranges = !self.search_ranges.is_empty();
        self.search_ranges.clear();
        had_ranges
    }

    pub fn perform_search(&mut self, term: &Regex) {
        self.search_ranges = term
            .find_iter(&self.buf)
            .map(|m| (m.start() + 1, m.end()))
            .collect();
    }
}

impl Diff {
    // TODO! search
    // pub fn perform_search(&mut self, term: &Regex) {
    //     for file in self.files.iter_mut() {
    //         let mut found_in_file = false;
    //         let file_is_expanded = file.is_expanded();
    //         for hunk in file.hunks.iter_mut() {
    //             let has_prev_search = !hunk.search_ranges.is_empty();
    //             hunk.perform_search(term);
    //             let found_in_hunk = !hunk.search_ranges.is_empty();
    //             if has_prev_search || found_in_hunk {
    //                 if file_is_expanded {
    //                     if hunk.is_expanded() {
    //                         println!("📻 case 2 {}", &hunk);
    //                         for line in &hunk.lines {
    //                             // TODO mark only certain lines
    //                             line.update_tags();
    //                         }
    //                     } else {
    //                         println!("📻 case 1 {}", &hunk);
    //                         if let Some(line_no) = hunk.get_line_no() {
    //                             println!("📻 case 1 EXPAND {}", &hunk);
    //                             hunk.expand(line_no, context);
    //                         }
    //                     }
    //                 } else if !hunk.is_expanded() {
    //                     println!("📻 case 3/4");
    //                     hunk.set_switch(true);
    //                 }
    //             }
    //             found_in_file = found_in_file || found_in_hunk;
    //         }
    //         if found_in_file && !file.is_expanded() {
    //             if let Some(line_no) = file.get_line_no() {
    //                 println!("📻 case 3/4 FILE");
    //                 file.expand(line_no);
    //             }
    //         }
    //     }
    // }

    pub fn reset_search(&mut self) {
        for file in self.files.iter_mut() {
            for hunk in file.hunks.iter_mut() {
                if hunk.reset_search() && hunk.is_expanded() {
                    for line in &hunk.lines {
                        // TODO mark only certain lines
                        line.update_tags();
                    }
                }
            }
        }
    }
}
