#!/bin/bash
# The write_fanout bench's Ir per tick under callgrind, on Linux: for each case,
# a run of T ticks minus a run of 0 ticks, divided by T. Run from the
# repository root, in a container:
#
#   docker run --rm -v "$PWD":/src -v arora-bench-target:/target \
#     -e CARGO_TARGET_DIR=/target rust:1-bookworm \
#     /src/crates/arora-behavior-tree/benches/write_fanout_callgrind.sh
#
# SPECS="<case>:<sink> …" restricts the cases; T sets the tick count (200).
set -euo pipefail
apt-get update -qq >/dev/null && apt-get install -y -qq valgrind >/dev/null
cd /src
cargo bench -p arora-behavior-tree --bench write_fanout --no-run 2>&1 | tail -2
BIN=$(ls -t /target/release/deps/write_fanout-* | grep -v '\.d$' | head -1)
T=${T:-200}
ir() { valgrind --tool=callgrind --callgrind-out-file=/dev/null "$BIN" "$@" 2>&1 | sed -n 's/.*Collected : \([0-9]*\).*/\1/p'; }
printf "%-14s %-6s %4s %12s\n" case sink n "Ir/tick"
for spec in ${SPECS:-baseline:store baseline:local write_keys:store write_keys:local unpack:store unpack:local unpack_lookup:store unpack_lookup:local set_select:store set_select:local spread:store spread:local set_spread:store set_spread:local store_slots:store store_batch:store}; do
  case=${spec%%:*}; sink=${spec##*:}
  for n in 1 10 30 100; do
    zero=$(ir "$case" "$n" 0 "$sink"); full=$(ir "$case" "$n" "$T" "$sink")
    printf "%-14s %-6s %4s %12s\n" "$case" "$sink" "$n" $(( (full - zero) / T ))
  done
done
