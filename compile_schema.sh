#!/bin/bash
schema_path=~/.local/share/glib-2.0/schemas/
/bin/cp io.github.aganzha.Stage.gschema.xml $schema_path
glib-compile-schemas $schema_path
# glib-compile-schemas ./src && cp ./src/gschemas.compiled ./target/debug && cp ./src/gschemas.compiled ./target/release
