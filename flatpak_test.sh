#!/bin/bash
./compile_resources.sh
./compile_schema.sh
RUST_BACKTRACE=1 RUST_LOG=debug OUT_DIR=. cargo test --no-run -- --nocapture
find target/debug/deps/ -type f -executable -name 'stage*' -exec '{}' ';'
