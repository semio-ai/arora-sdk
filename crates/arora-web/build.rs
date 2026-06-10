use std::env;
use std::path::PathBuf;

// Expose the test-rust-wasm header yaml to the browser test for `include_str!`.
// The guest wasm comes from the test-rust-wasm cdylib artifact dependency
// (Cargo.toml); cargo passes its path to the test crate directly.
fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let crates_dir = manifest_dir
        .parent()
        .expect("crates/arora-web has a parent");
    let workspace_root = crates_dir.parent().expect("crates/ has a parent");

    let header_yaml = workspace_root
        .join("modules")
        .join("test-rust-wasm")
        .join("src")
        .join("arora_generated")
        .join("module.yaml");

    println!(
        "cargo:rustc-env=TEST_RUST_WASM_HEADER_YAML={}",
        header_yaml.display()
    );
    println!("cargo:rerun-if-changed=build.rs");
}
