#!/bin/bash
glib-compile-schemas ./src && cp ./src/gschemas.compiled ./target/debug
