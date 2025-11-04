#/bin/bash
export RUSTFLAGS='-A deprecated'
clear && OUT_DIR=. cargo clippy --fix --bin "stage" --allow-dirty --no-deps
