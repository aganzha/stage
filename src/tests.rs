// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(test)]
use crate::status_view::tags;
#[cfg(test)]
use gtk4::prelude::*;
#[cfg(test)]
use gtk4::TextBuffer;
#[cfg(test)]
use log::debug;
#[cfg(test)]
use std::sync::Once;

#[cfg(test)]
static INIT: Once = Once::new();

#[cfg(test)]
pub fn initialize() -> TextBuffer {
    INIT.call_once(|| {
        env_logger::builder().format_timestamp(None).init();
        debug!("CALL ONCE----------------> {:?}", gtk4::init());
    });
    let buffer = TextBuffer::new(None);
    let table = buffer.tag_table();
    for tag_name in tags::TEXT_TAGS {
        let text_tag = tags::TxtTag::from_str(tag_name).create();
        table.add(&text_tag);
    }
    buffer
}
