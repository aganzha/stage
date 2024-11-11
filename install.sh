glib-compile-schemas ./src && cp ./src/gschemas.compiled ./target/release
install -Dm755 ./target/release/stage -t /app/bin/
install -Dm744 ./target/release/gschemas.compiled -t /app/bin/
install -Dm644 io.github.aganzha.Stage.metainfo.xml -t /app/share/metainfo
install -Dm644 io.github.aganzha.Stage.desktop -t /app/share/applications
install -Dm644 ./icons/16x16/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/16x16/apps
install -Dm644 ./icons/32x32/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/32x32/apps
install -Dm644 ./icons/48x48/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/48x48/apps
install -Dm644 ./icons/64x64/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/64x64/apps
install -Dm644 ./icons/128x128/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/128x128/apps
install -Dm644 ./icons/256x256/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/256x256/apps
install -Dm644 ./icons/512x512/io.github.aganzha.Stage.png -t /app/share/icons/hicolor/512x512/apps
install -Dm644 ./icons/io.github.aganzha.Stage.svg -t /app/share/icons/hicolor/scalable/apps
install -Dm644 ./icons/io.github.aganzha.Stage.Devel.svg -t /app/share/icons/hicolor/scalable/apps
install -Dm644 ./icons/io.github.aganzha.Stage-symbolic.svg -t /app/share/icons/hicolor/symbolic/apps
