#!/usr/bin/env bash
# Stage artifacts under www/ and serve over HTTP. No bundler.
#
# Builds arora-web for the browser, copies the resulting JS shim +
# wasm into www/pkg/, copies guest modules into www/modules/<name>/,
# then runs `python3 -m http.server`.
#
# Requires: wasm-pack and python3. Guest WASM modules are built
# automatically if their artifacts are missing or stale.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
crate_dir="$(cd "$here/.." && pwd)"
workspace_root="$(cd "$crate_dir/../.." && pwd)"

guest_wasm="$workspace_root/target/wasm32-wasip1/debug/test_rust_wasm.wasm"
guest_yaml="$workspace_root/modules/test-rust-wasm/src/arora_generated/module.yaml"
guest_records="$workspace_root/modules/test-rust-wasm/records/records.json"

bt_nodes_wasm="$workspace_root/target/wasm32-wasip1/debug/test_behavior_tree_nodes.wasm"
bt_nodes_yaml="$workspace_root/modules/test-behavior-tree-nodes/src/arora_generated/module.yaml"
bt_nodes_records="$workspace_root/modules/test-behavior-tree-nodes/records/records.json"

echo "==> cargo build guest modules (wasm32-wasip1)"
(cd "$workspace_root" && cargo build -p test-rust-wasm -p test-behavior-tree-nodes --target wasm32-wasip1)

echo "==> wasm-pack build $crate_dir"
(cd "$crate_dir" && wasm-pack build --target web --dev)

echo "==> staging artifacts under www/"
rm -rf "$here/pkg" "$here/modules"
mkdir -p "$here/pkg" "$here/modules/test-rust-wasm" "$here/modules/test-behavior-tree-nodes"
cp "$crate_dir/pkg/arora_web.js" "$here/pkg/"
cp "$crate_dir/pkg/arora_web_bg.wasm" "$here/pkg/"
cp "$guest_yaml" "$here/modules/test-rust-wasm/module.yaml"
cp "$guest_wasm" "$here/modules/test-rust-wasm/test_rust_wasm.wasm"
cp "$guest_records" "$here/modules/test-rust-wasm/records.json"
cp "$bt_nodes_yaml" "$here/modules/test-behavior-tree-nodes/module.yaml"
cp "$bt_nodes_wasm" "$here/modules/test-behavior-tree-nodes/test_behavior_tree_nodes.wasm"
cp "$bt_nodes_records" "$here/modules/test-behavior-tree-nodes/records.json"

port="${PORT:-8080}"
echo "==> serving on http://localhost:$port"
echo "    original demo: http://localhost:$port/index.html"
echo "    behavior-tree demo: http://localhost:$port/demo.html"
cd "$here"
exec python3 -m http.server "$port"
