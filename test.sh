#!/bin/bash
clear && TERM=xterm-256color RUSTFLAGS='-A deprecated' RUST_BACKTRACE=1 RUST_LOG=debug RUST_TEST_THREADS=1 OUT_DIR=. cargo test -- --test-threads=1 --nocapture $@
