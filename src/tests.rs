// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: LGPL-3.0-or-later
use gtk4::prelude::*;
use log::debug;
use std::sync::Once;

static INIT: Once = Once::new();

pub fn initialize() {
    INIT.call_once(|| {
        env_logger::builder().format_timestamp(None).init();
        debug!("CALL ONCE----------------> {:?}", gtk4::init());
    });
}
