#!/usr/bin/env bash
# Stage artifacts under www/ and serve over HTTP. No bundler.
#
# Builds arora-web for the browser, copies the resulting JS shim +
# wasm into www/pkg/, copies the test-rust-wasm guest into
# www/test-rust-wasm/, then runs `python3 -m http.server`.
#
# Requires: wasm-pack, python3, and a prior `cargo test
# -p arora-integration-tests` (or `cargo build --workspace`) so that
# target/wasm32-wasip1/debug/test_rust_wasm.wasm exists.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
crate_dir="$(cd "$here/.." && pwd)"
workspace_root="$(cd "$crate_dir/../.." && pwd)"

guest_wasm="$workspace_root/target/wasm32-wasip1/debug/test_rust_wasm.wasm"
guest_yaml="$workspace_root/modules/test-rust-wasm/src/arora_generated/module.yaml"

if [[ ! -f "$guest_wasm" ]]; then
  echo "missing $guest_wasm" >&2
  echo "run: cargo test -p arora-integration-tests" >&2
  exit 1
fi

echo "==> wasm-pack build $crate_dir"
(cd "$crate_dir" && wasm-pack build --target web --dev)

echo "==> staging artifacts under www/"
rm -rf "$here/pkg" "$here/test-rust-wasm"
mkdir -p "$here/pkg" "$here/test-rust-wasm"
cp "$crate_dir/pkg/arora_web.js" "$here/pkg/"
cp "$crate_dir/pkg/arora_web_bg.wasm" "$here/pkg/"
cp "$guest_yaml" "$here/test-rust-wasm/module.yaml"
cp "$guest_wasm" "$here/test-rust-wasm/test_rust_wasm.wasm"

port="${PORT:-8080}"
echo "==> serving on http://localhost:$port"
cd "$here"
exec python3 -m http.server "$port"
