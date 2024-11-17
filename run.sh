#!/bin/bash
clear && RUST_BACKTRACE=1 RUST_LOG=debug OUT_DIR=. cargo run $@

