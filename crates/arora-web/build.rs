use std::env;
use std::path::PathBuf;

// Expose the test-rust-wasm header yaml path to the browser test so it can
// `include_str!` it. The wasm artifact itself is a dev-dependency artifact
// (see Cargo.toml: test-rust-wasm = { artifact = "cdylib", target =
// "wasm32-wasip1" }); cargo builds it on demand for the tests and exposes its
// path to the test crate directly as CARGO_CDYLIB_FILE_TEST_RUST_WASM_*, so no
// separate `cargo build --target wasm32-wasip1` step is needed and build.rs
// does not have to forward the wasm path.
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

  println!("cargo:rustc-env=TEST_RUST_WASM_HEADER_YAML={}", header_yaml.display());
  println!("cargo:rerun-if-changed=build.rs");
}
