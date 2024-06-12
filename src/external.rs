use gtk4::prelude::*;
use gtk4::{gio, glib};
use log::info;

pub fn try_open_editor() {
    let proxy = gio::DBusProxy::for_bus(
        gio::BusType::Session,
        gio::DBusProxyFlags::empty(),
        None,
        "org.gnome.TextEditor",
        "/org/gnome/TextEditor",
        "org.gtk.Application",
        None::<&gio::Cancellable>,
        |result| {
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

                let file_path = glib::Variant::from_data_with_type(
                    "/home/aganzha/stage/src/main.rs\0",
                    byte_array_type
                );
                let line_no = glib::Variant::from_data_with_type(
                    "+12\0",
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
