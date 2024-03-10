#!/bin/bash
clear && RUST_BACKTRACE=1 RUST_LOG=debug flatpak run --filesystem=home --socket=ssh-auth --socket=gpg-agent --share=network com.github.aganzha.stage $@
