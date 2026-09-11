#!/bin/sh
# The study's evidence: the guest artifact, then every test.
set -e
cd "$(dirname "$0")"
cargo build -q -p case-bt-nodes --target wasm32-wasip1
cargo test -q
