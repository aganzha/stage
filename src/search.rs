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

pub fn make_search(
    current_search_line: Rc<Cell<Option<i32>>>,
    search_matched_lines: Rc<RefCell<Vec<i32>>>,
    sender: Sender<Event>,
) -> (SearchBar, impl Fn()) {
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
    let updater = {
        let label = lbl.clone();
        let current_search_line = current_search_line.clone();
        let search_matched_lines = search_matched_lines.clone();
        move || {
            if let Some(line) = current_search_line.get() {
                let lines = search_matched_lines.borrow();
                if let Some(idx) = lines.iter().position(|l| l == &line) {
                    label.set_label(&format!("{}({})", idx + 1, lines.len()));
                }
            } else {
                label.set_label("");
            }
        }
    };
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

    forward.connect_clicked({
        let search_matched_lines = search_matched_lines.clone();
        let current_search_line = current_search_line.clone();
        let sender = sender.clone();
        let updater = updater.clone();
        move |_btn| {
            let current_line = current_search_line.get().unwrap_or(0);
            if let Some(scroll_to) = search_matched_lines
                .borrow()
                .iter()
                .filter(|l| l > &&current_line)
                .min()
                .copied()
            {
                current_search_line.replace(Some(scroll_to));
                sender
                    .send_blocking(Event::GoToLine(scroll_to))
                    .expect("cant send throu channel");
            }
            updater();
        }
    });
    backward.connect_clicked({
        let search_matched_lines = search_matched_lines.clone();
        let current_search_line = current_search_line.clone();
        let sender = sender.clone();
        let updater = updater.clone();
        move |_btn| {
            let current_line = current_search_line.get().unwrap_or(0);
            if let Some(scroll_to) = search_matched_lines
                .borrow()
                .iter()
                .filter(|l| l < &&current_line)
                .max()
                .copied()
            {
                current_search_line.replace(Some(scroll_to));
                sender
                    .send_blocking(Event::GoToLine(scroll_to))
                    .expect("cant send through channel");
            }
            updater();
        }
    });

    (
        SearchBar::builder()
            .child(&search_box)
            .search_mode_enabled(true)
            .visible(true)
            .show_close_button(true)
            .build(),
        updater,
    )
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
