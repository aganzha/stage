#/bin/bash
clear && cargo clippy --fix --bin "stage" --allow-dirty --no-deps
