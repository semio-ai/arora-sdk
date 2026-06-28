use std::env;

// behavior-tree-nodes is an artifact (bindep) build-dependency, built for
// wasm32-wasip1. Cargo exposes its path to this build script as an environment
// variable named `CARGO_CDYLIB_FILE_BEHAVIOR_TREE_NODES[_<lib>]`. Re-export it
// under a stable name so the crate can `include_bytes!(env!("BT_NODES_WASM"))`.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // The behavior-tree-nodes artifact is an optional build-dependency behind the
    // `native` feature. When it's present, re-export its path for lib.rs to
    // `include_bytes!`. When it's absent — e.g. the wasm build, which uses
    // --no-default-features — the bytes are supplied at runtime instead
    // (`Arora::start_with_nodes`), so there is nothing to embed.
    if let Some((_, wasm)) =
        env::vars().find(|(k, _)| k.starts_with("CARGO_CDYLIB_FILE_BEHAVIOR_TREE_NODES"))
    {
        println!("cargo:rustc-env=BT_NODES_WASM={wasm}");
    }
}
