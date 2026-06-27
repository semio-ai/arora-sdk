use std::env;

// behavior-tree-nodes is an artifact (bindep) build-dependency, built for
// wasm32-wasip1. Cargo exposes its path to this build script as an environment
// variable named `CARGO_CDYLIB_FILE_BEHAVIOR_TREE_NODES[_<lib>]`. Re-export it
// under a stable name so the crate can `include_bytes!(env!("BT_NODES_WASM"))`.
fn main() {
    let wasm = env::vars()
        .find(|(k, _)| k.starts_with("CARGO_CDYLIB_FILE_BEHAVIOR_TREE_NODES"))
        .map(|(_, v)| v)
        .expect(
            "behavior-tree-nodes wasm artifact path not provided by cargo; \
             ensure bindeps are enabled (.cargo/config.toml) and the \
             wasm32-wasip1 target is installed",
        );
    println!("cargo:rustc-env=BT_NODES_WASM={wasm}");
    println!("cargo:rerun-if-changed=build.rs");
}
