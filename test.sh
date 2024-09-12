#!/bin/bash
# ./test.sh --test choose_conflict_side
clear && RUST_BACKTRACE=1 RUST_LOG=debug cargo test -- --test-threads=1 --nocapture $@
