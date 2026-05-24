use std::env;
use std::path::PathBuf;

// Locate the wasm artifact + header yaml at compile time so the test
// can `include_bytes!` / `include_str!` them. We do NOT bindep the
// guest module — that gets pulled in (and built fresh) by the
// workspace integration tests at `tests/Cargo.toml`. Running this
// crate's wasm-bindgen tests therefore requires
// `cargo test -p arora-integration-tests` (or `cargo build --workspace`
// followed by the wasm bindep walk) to have produced
// `target/wasm32-wasip1/debug/test_rust_wasm.wasm` beforehand.
fn main() {
  let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
  let crates_dir = manifest_dir.parent().expect("crates/arora-web has a parent");
  let workspace_root = crates_dir.parent().expect("crates/ has a parent");

  let header_yaml = workspace_root
    .join("modules")
    .join("test-rust-wasm")
    .join("src")
    .join("arora_generated")
    .join("module.yaml");
  let wasm = workspace_root
    .join("target")
    .join("wasm32-wasip1")
    .join("debug")
    .join("test_rust_wasm.wasm");

  println!("cargo:rustc-env=TEST_RUST_WASM_HEADER_YAML={}", header_yaml.display());
  println!("cargo:rustc-env=TEST_RUST_WASM_BYTES={}", wasm.display());
  println!("cargo:rerun-if-changed=build.rs");
}
