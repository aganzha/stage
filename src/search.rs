// SPDX-FileCopyrightText: 2026 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Event, Hunk};
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
        } //  else {
          //     self.label.set_label("");
          //     self.search_entry.set_text("");
          // }
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
            let current_line = search.current_lineno.get().unwrap_or(0);
            if let Some(scroll_to) = search
                .matched_lines
                .borrow()
                .iter()
                .filter(|l| l > &&current_line)
                .min()
                .copied()
            {
                search.current_lineno.replace(Some(scroll_to));
                search
                    .sender
                    .send_blocking(Event::GoToLine(scroll_to))
                    .expect("cant send through channel");
            }
            search.update();
        }
    });
    backward.connect_clicked({
        let search = search.clone();
        move |_btn| {
            let current_line = search.current_lineno.get().unwrap_or(0);
            if let Some(scroll_to) = search
                .matched_lines
                .borrow()
                .iter()
                .filter(|l| l < &&current_line)
                .max()
                .copied()
            {
                search.current_lineno.replace(Some(scroll_to));
                search
                    .sender
                    .send_blocking(Event::GoToLine(scroll_to))
                    .expect("cant send through channel");
            }
            search.update();
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
