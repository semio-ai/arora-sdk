#!/usr/bin/env bash
# Stage the demo under www/ and serve it over HTTP. No bundler.
#
# Builds arora-web for the browser into www/pkg/, and stages each guest as a
# module directory, www/modules/<name>/ — header.json, the guest and records/
# — from what its build or its declaration writes:
#
# - test-rust-wasm declares itself in Rust: its `module_dir` example writes
#   the header and the records from the declaration;
# - test-behavior-tree-nodes is generated from its module.yaml: its build
#   writes the header to src/arora_generated/header.json and the records to
#   records/.
#
# A static server lists no directory, so each module directory also gets
# record-files.json: the paths of its records under records/.
#
# Requires: wasm-pack, jq and python3.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
crate_dir="$(cd "$here/.." && pwd)"
workspace_root="$(cd "$crate_dir/../.." && pwd)"
# Where cargo builds: CARGO_TARGET_DIR when it is set, the workspace's
# target/ otherwise.
target_dir="$(cd "$workspace_root" && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)"
guests="$target_dir/wasm32-wasip2/debug"
modules="$here/modules"
bt_nodes="$workspace_root/modules/test-behavior-tree-nodes"

echo "==> cargo build guest modules (wasm32-wasip2)"
(cd "$workspace_root" && cargo build -p test-rust-wasm -p test-behavior-tree-nodes --target wasm32-wasip2)

echo "==> wasm-pack build $crate_dir"
(cd "$crate_dir" && wasm-pack build --target web --dev)

echo "==> staging under www/"
rm -rf "$here/pkg" "$modules"
mkdir -p "$here/pkg" "$modules/test-behavior-tree-nodes"
cp "$crate_dir/pkg/arora_web.js" "$crate_dir/pkg/arora_web_bg.wasm" "$here/pkg/"

(cd "$workspace_root" && cargo run -p test-rust-wasm --example module_dir -- \
  "$modules/test-rust-wasm" "$guests/test_rust_wasm.wasm")

cp "$bt_nodes/src/arora_generated/header.json" "$modules/test-behavior-tree-nodes/"
cp "$guests/test_behavior_tree_nodes.wasm" "$modules/test-behavior-tree-nodes/"
cp -R "$bt_nodes/records" "$modules/test-behavior-tree-nodes/records"

for dir in "$modules"/*; do
  (cd "$dir/records" && find . -mindepth 2 -type f -name '*.yaml' | sed 's|^\./||' | LC_ALL=C sort \
    | jq -R . | jq -s .) > "$dir/record-files.json"
done

port="${PORT:-8080}"
echo "==> serving on http://localhost:$port"
echo "    module explorer: http://localhost:$port/index.html"
echo "    behavior-tree demo: http://localhost:$port/demo.html"
cd "$here"
exec python3 -m http.server "$port"
