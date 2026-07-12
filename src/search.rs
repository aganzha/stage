// SPDX-FileCopyrightText: 2026 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Event, Hunk, StatusRenderContext};
use async_channel::Sender;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, SearchBar, SearchEntry};
// use glib::signal::SignalHandlerId;
// use std::sync::{OnceLock, RwLock};
use regex::Regex;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub fn make_search(sender: Sender<Event>) -> (SearchBar, impl Fn(i32, &mut StatusRenderContext)) {
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

    let found_lines: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let current_search_line: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
    forward.connect_clicked({
        let found_lines = found_lines.clone();
        let current_search_line = current_search_line.clone();
        let sender = sender.clone();
        move |_btn| {
            let current_line = current_search_line.get().unwrap_or(0);
            println!(
                "🏈 FORWARD curren search line {:?} found lines {:?}",
                current_line, found_lines
            );

            if let Some(scroll_to) = found_lines
                .borrow()
                .iter()
                .filter(|l| l > &&current_line)
                .min()
                .copied()
            {
                println!("🏈 FORWARD SCROLL_TO {}", scroll_to);
                current_search_line.replace(Some(scroll_to));
                sender
                    .send_blocking(Event::GoToLine(scroll_to))
                    .expect("cant send throu channel");
            }
        }
    });
    backward.connect_clicked({
        let found_lines = found_lines.clone();
        let current_search_line = current_search_line.clone();
        let sender = sender.clone();
        move |_btn| {
            let current_line = current_search_line.get().unwrap_or(0);
            println!(
                "🦴 GO BACKWARD curren search line {:?} found lines {:?}",
                current_line, found_lines
            );
            if let Some(scroll_to) = found_lines
                .borrow()
                .iter()
                .filter(|l| l < &&current_line)
                .max()
                .copied()
            {
                println!("🦴 backeard SCROLL_TO {}", scroll_to);
                current_search_line.replace(Some(scroll_to));
                sender
                    .send_blocking(Event::GoToLine(scroll_to))
                    .expect("cant send through channel");
            }
        }
    });
    let search_bar = SearchBar::builder()
        .child(&search_box)
        .search_mode_enabled(true)
        .visible(true)
        .show_close_button(true)
        .build();

    let updater = {
        let current_search_line = current_search_line.clone();
        let found_lines = found_lines.clone();
        move |current_line: i32, context: &mut StatusRenderContext| {
            println!(
                "🧶 update search ..... {} {:?}",
                current_line, context.search_matched_lines
            );
            current_search_line.replace(Some(current_line));
            found_lines.replace(context.search_matched_lines.clone());
            // let found_lines = context.search_matched_lines.clone();
            // let handlers = search_bar_handler().write().unwrap();
            // for handler in handlers.iter() {
            //     search_bar.disconnect(handler);
            // }
        }
    };
    (search_bar, updater)
}

impl Hunk {
    fn mark_dirty_by_search(&self) -> bool {
        if !self.search_ranges.is_empty() {
            self.view.dirty(true);
            // ⚠️ ATTENTION this affect structure during expand/collapse
            //self.view.child_dirty(true);
            for line in &self.lines {
                line.view.dirty(true);
            }
            return true;
        }
        false
    }
    pub fn reset_search(&mut self) -> bool {
        let was_searched = self.mark_dirty_by_search();
        self.search_ranges.clear();
        was_searched
    }
    pub fn perform_search(&mut self, term: &Regex) -> bool {
        // cleanup prev search
        self.reset_search();
        self.search_ranges = term
            .find_iter(&self.buf)
            .map(|m| (m.start() + 1, m.end()))
            .collect();
        if !self.search_ranges.is_empty() {
            println!("🧶 FOUND!");
        }
        self.mark_dirty_by_search()
    }
}
