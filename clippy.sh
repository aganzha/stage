#/bin/bash
clean && cargo clippy --fix --bin "stage" --allow-dirty
