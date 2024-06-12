use gtk4::prelude::*;
use gtk4::{gio, glib};
use log::{info, debug};
use std::path::PathBuf;
use std::ffi::OsString;

pub fn try_open_editor(path: PathBuf, line_no: u32) {
    debug!("ooooooooooooooooooooooooooooooo > {:?} {:?}", path, line_no);
    // for app_info in gio::AppInfo::all_for_type("text/rust") {
    //     //     // new-window new-document
    //     info!("aaaaaaaaaaalll apps {:?} {:?} {:?} {:?}", app_info.id(), app_info.name(), app_info.commandline(), app_info.executable());
    //                     //     if app_info.name() == "Text Editor" { // Text Editor
    // }
    let proxy = gio::DBusProxy::for_bus(
        gio::BusType::Session,
        gio::DBusProxyFlags::empty(),
        None,
        "org.gnome.TextEditor",
        "/org/gnome/TextEditor",
        "org.gtk.Application",
        None::<&gio::Cancellable>,
        move |result| {
            if let Ok(proxy) = result {

                let platform_type = glib::VariantTy::new("a{sv}").expect("bad type");
                let platform_ob = glib::Variant::from_data_with_type(
                    "",
                    platform_type
                );

                let byte_array_type = glib::VariantTy::new("ay").expect("bad type");
                let exe = glib::Variant::from_data_with_type(
                    "gnome-text-editor %U\0",
                    // "gnome-text-editor %U",
                    byte_array_type
                );

                let mut path = OsString::from(path);
                path.push("\0");
                let file_path = glib::Variant::from_data_with_type(
                    path.as_encoded_bytes(),
                    byte_array_type
                );

                let mut line = OsString::from("+");
                line.push(line_no.to_string());
                line.push("\0");
                let line_no = glib::Variant::from_data_with_type(
                    line.as_encoded_bytes(),
                    byte_array_type
                );

                let byte_array_array_type = glib::VariantTy::new("aay").expect("bad type");
                info!("byte_array_array_type == {:?}", byte_array_array_type);
                let byte_array_array = glib::Variant::parse(
                    Some(byte_array_array_type),
                    &format!("[{},{},{}]", exe.print(true), file_path.print(true), line_no.print(true))
                ).unwrap();

                let object_path = glib::variant::ObjectPath::try_from(String::from("/org/gnome/TextEditor"));

                let path = object_path.unwrap().to_variant();

                let args = glib::Variant::tuple_from_iter([path, byte_array_array, platform_ob]);


                info!("dbus args {:?}", args);
                // [INFO  stage] dbus args Variant { ptr: 0x560ee73df4f0, type: VariantTy { inner: "(oaaya{sv})" }, value: "(objectpath '/org/gnome/TextEditor', [b'gnome-text-editor %U', b'/home/aganzha/stage/src/main.rs', b'+12'], @a{sv} {})" }

                let result = proxy.call_sync(
                    "CommandLine",
                    Some(
                        &args
                    ),
                    gio::DBusCallFlags::empty(),
                    1000,
                    None::<&gio::Cancellable>
                );
            }
        }
    );
}
