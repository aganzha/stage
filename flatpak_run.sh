#!/bin/bash
clear && RUST_BACKTRACE=1 RUST_LOG=debug flatpak run com.github.aganzha.stage $@
