#!/usr/bin/env bash
# Stage artifacts under www/ and serve over HTTP. No bundler.
#
# Builds arora-web for the browser, copies the resulting JS shim +
# wasm into www/pkg/, copies guest modules into www/modules/<name>/,
# then runs `python3 -m http.server`.
#
# Requires: wasm-pack, python3, and a prior `cargo test
# -p arora-integration-tests` (or `cargo build --workspace`) so that
# target/wasm32-wasip1/debug/*.wasm artifacts exist.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
crate_dir="$(cd "$here/.." && pwd)"
workspace_root="$(cd "$crate_dir/../.." && pwd)"

guest_wasm="$workspace_root/target/wasm32-wasip1/debug/test_rust_wasm.wasm"
guest_yaml="$workspace_root/modules/test-rust-wasm/src/arora_generated/module.yaml"

bt_nodes_wasm="$workspace_root/target/wasm32-wasip1/debug/behavior_tree_nodes.wasm"
bt_nodes_yaml="$workspace_root/modules/behavior-tree-nodes/src/arora_generated/module.yaml"

if [[ ! -f "$guest_wasm" ]]; then
  echo "missing $guest_wasm" >&2
  echo "run: cargo test -p arora-integration-tests" >&2
  exit 1
fi

if [[ ! -f "$bt_nodes_wasm" ]]; then
  echo "missing $bt_nodes_wasm" >&2
  echo "run: cargo build -p behavior-tree-nodes --target wasm32-wasip1" >&2
  exit 1
fi

echo "==> wasm-pack build $crate_dir"
(cd "$crate_dir" && wasm-pack build --target web --dev)

echo "==> staging artifacts under www/"
rm -rf "$here/pkg" "$here/modules"
mkdir -p "$here/pkg" "$here/modules/test-rust-wasm" "$here/modules/behavior-tree-nodes"
cp "$crate_dir/pkg/arora_web.js" "$here/pkg/"
cp "$crate_dir/pkg/arora_web_bg.wasm" "$here/pkg/"
cp "$guest_yaml" "$here/modules/test-rust-wasm/module.yaml"
cp "$guest_wasm" "$here/modules/test-rust-wasm/test_rust_wasm.wasm"
cp "$bt_nodes_yaml" "$here/modules/behavior-tree-nodes/module.yaml"
cp "$bt_nodes_wasm" "$here/modules/behavior-tree-nodes/behavior_tree_nodes.wasm"

port="${PORT:-8080}"
echo "==> serving on http://localhost:$port"
echo "    original demo: http://localhost:$port/index.html"
echo "    behavior-tree demo: http://localhost:$port/demo.html"
cd "$here"
exec python3 -m http.server "$port"
