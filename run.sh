#!/bin/bash
clear
export RUST_BACKTRACE=1
export RUST_LOG=debug
export RUSTFLAGS='-A deprecated'
export OUT_DIR=.
cargo run $@

