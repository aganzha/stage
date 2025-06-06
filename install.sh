#!/bin/sh

export PROJECT_ROOT="$1"
export PROJECT_SOURCES="$1"/src
export CARGO_HOME=/run/build/stage/cargo
export OUT_DIR=.

cargo fetch --manifest-path "$SOURCE_ROOT"/Cargo.toml --offline --verbose

# resources
cp "$PROJECT_ROOT"/io.github.aganzha.Stage.metainfo.xml ./
glib-compile-resources "$PROJECT_ROOT"/io.github.aganzha.Stage.gresource.xml --target "$PROJECT_SOURCES"/gresources.compiled

cargo build --release --verbose --offline

glib-compile-schemas "$PROJECT_SOURCES" && cp "$PROJECT_SOURCES"/gschemas.compiled "$PROJECT_ROOT"/target/release

install -Dm755 "$PROJECT_ROOT"/target/release/stage -t /app/bin/
install -Dm744 "$PROJECT_ROOT"/target/release/gschemas.compiled -t /app/bin/

install -Dm644 "$PROJECT_ROOT"/io.github.aganzha.Stage.metainfo.xml -t /app/share/metainfo
install -Dm644 "$PROJECT_ROOT"/io.github.aganzha.Stage.desktop -t /app/share/applications
install -Dm644 "$PROJECT_ROOT"/icons/16x16/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/16x16/apps
install -Dm644 "$PROJECT_ROOT"/icons/32x32/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/32x32/apps
install -Dm644 "$PROJECT_ROOT"/icons/48x48/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/48x48/apps
install -Dm644 "$PROJECT_ROOT"/icons/64x64/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/64x64/apps
install -Dm644 "$PROJECT_ROOT"/icons/128x128/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/128x128/apps
install -Dm644 "$PROJECT_ROOT"/icons/256x256/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/256x256/apps
install -Dm644 "$PROJECT_ROOT"/icons/512x512/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/512x512/apps
install -Dm644 "$PROJECT_ROOT"/icons/io.github.aganzha.Stage.svg -t /app/share/icons/hicolor/scalable/apps
install -Dm644 "$PROJECT_ROOT"/icons/io.github.aganzha.Stage-symbolic.svg -t /app/share/icons/hicolor/symbolic/apps
install -Dm644 "$PROJECT_ROOT"/icons/org.gnome.Logs-symbolic.svg -t /app/share/icons/hicolor/symbolic/apps
