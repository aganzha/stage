// SPDX-FileCopyrightText: 2025 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Event, Hunk};
use async_channel::Sender;
use gtk4::prelude::*;
use gtk4::{SearchBar, SearchEntry};
use regex::Regex;

pub fn make_search(sender: Sender<Event>) -> SearchBar {
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

    SearchBar::builder()
        .child(&search_entry)
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(true)
        .build()
}

impl Hunk {
    pub fn perform_search(&mut self, term: &Regex) -> bool {
        self.search_ranges = term
            .find_iter(&self.buf)
            .map(|m| (m.start(), m.end()))
            .collect();
        if !self.search_ranges.is_empty() {
            self.view.expand(true); // ??? how to do that? expand works on certain lines...
            self.view.dirty(true);
            self.view.child_dirty(true);
            // not all lines should be rendered. TODO! compare indices!
            for line in &self.lines {
                line.view.dirty(true);
            }
            return true;
        }
        false
    }
}
