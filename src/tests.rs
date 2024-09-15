// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later
use crate::status_view::tags;
use gtk4::prelude::*;
use gtk4::TextBuffer;
use log::debug;
use std::sync::Once;

static INIT: Once = Once::new();

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
