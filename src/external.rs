// SPDX-FileCopyrightText: 2024 Aleksey Ganzha <aganzha@yandex.ru>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::prelude::*;
use gtk4::{gio, glib};
use log::info;
use std::ffi::OsString;
use std::path::PathBuf;

pub fn open_at_line_via_dbus(executable: PathBuf, path: PathBuf, line_no: i32, col_no: i32) {
    let proxy = gio::DBusProxy::for_bus_sync(
        gio::BusType::Session,
        gio::DBusProxyFlags::empty(),
        None,
        "org.gnome.TextEditor",
        "/org/gnome/TextEditor",
        "org.gtk.Application",
        None::<&gio::Cancellable>,
    )
    .expect("cant get dbus proxy");

    let platform_type = glib::VariantTy::new("a{sv}").expect("bad type");
    let platform_ob = glib::Variant::from_data_with_type("", platform_type);

    let mut exe = OsString::from(executable);
    exe.push("\0");

    let byte_array_type = glib::VariantTy::new("ay").expect("bad type");
    let exe = glib::Variant::from_data_with_type(exe.as_encoded_bytes(), byte_array_type);

    let mut path = OsString::from(path);
    path.push("\0");
    let file_path = glib::Variant::from_data_with_type(path.as_encoded_bytes(), byte_array_type);

    let mut line = OsString::from("+");
    line.push(line_no.to_string());
    line.push(":");
    line.push(col_no.to_string());
    line.push("\0");
    let line_no = glib::Variant::from_data_with_type(line.as_encoded_bytes(), byte_array_type);

    let byte_array_array_type = glib::VariantTy::new("aay").expect("bad type");

    let byte_array_array = glib::Variant::parse(
        Some(byte_array_array_type),
        &format!(
            "[{},{},{}]",
            exe.print(true),
            file_path.print(true),
            line_no.print(true)
        ),
    )
    .unwrap();

    let object_path = glib::variant::ObjectPath::try_from(String::from("/org/gnome/TextEditor"));

    let path = object_path.unwrap().to_variant();

    let args = glib::Variant::tuple_from_iter([path, byte_array_array, platform_ob]);

    info!("dbus args {:?}", args);

    let result = proxy.call_sync(
        "CommandLine",
        Some(&args),
        gio::DBusCallFlags::empty(),
        1000,
        None::<&gio::Cancellable>,
    );
    info!("result in dbus call {:?}", result);
}

pub fn try_open_editor(path: PathBuf, line_no: i32, col_no: i32) {
    let (content_type, _) = gio::functions::content_type_guess(Some(path.clone()), &[]);
    if line_no > 0 {
        // it is possible to open TextEditor on certain line with DBUS
        for app_info in gio::AppInfo::all_for_type(&content_type) {
            if let Some(id) = app_info.id() {
                if id.contains("org.gnome.TextEditor") {
                    gio::spawn_blocking({
                        let exe = app_info.commandline().unwrap();
                        let path = path.clone();
                        move || open_at_line_via_dbus(exe, path, line_no, col_no)
                    });
                    return;
                }
            }
        }
    }

    if let Some(app_info) = gio::AppInfo::default_for_type(&content_type, false) {
        let file = gio::File::for_path(path);
        let opts: Option<&gio::AppLaunchContext> = None;
        app_info.launch(&[file], opts).expect("cant launch app");
    }
}
