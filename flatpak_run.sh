#!/bin/bash
clear && RUST_BACKTRACE=1 RUST_LOG=debug flatpak run io.github.aganzha.Stage $@
