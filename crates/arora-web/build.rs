use std::env;
use std::path::PathBuf;

// Locate wasm artifacts + header yaml files at compile time so the tests
// can `include_bytes!` / `include_str!` them. We do NOT bindep the
// guest modules here. Running this crate's wasm-bindgen tests therefore
// requires an explicit wasm guest build to have produced the
// target/wasm32-wasip1 artifacts beforehand.
fn main() {
  let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
  let crates_dir = manifest_dir
    .parent()
    .expect("crates/arora-web has a parent");
  let workspace_root = crates_dir.parent().expect("crates/ has a parent");
  let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

  let test_rust_header_yaml = workspace_root
    .join("modules")
    .join("test-rust-wasm")
    .join("src")
    .join("arora_generated")
    .join("module.yaml");
  let test_rust_wasm = workspace_root
    .join("target")
    .join("wasm32-wasip1")
    .join(&profile)
    .join("test_rust_wasm.wasm");
  let vizij_orchestrator_header_yaml = workspace_root
    .join("modules")
    .join("vizij-orchestrator")
    .join("src")
    .join("arora_generated")
    .join("module.yaml");
  let vizij_orchestrator_wasm = workspace_root
    .join("target")
    .join("wasm32-wasip1")
    .join(&profile)
    .join("arora_vizij_orchestrator.wasm");

  println!(
    "cargo:rustc-env=TEST_RUST_WASM_HEADER_YAML={}",
    test_rust_header_yaml.display()
  );
  println!(
    "cargo:rustc-env=TEST_RUST_WASM_BYTES={}",
    test_rust_wasm.display()
  );
  println!(
    "cargo:rustc-env=VIZIJ_ORCHESTRATOR_HEADER_YAML={}",
    vizij_orchestrator_header_yaml.display()
  );
  println!(
    "cargo:rustc-env=VIZIJ_ORCHESTRATOR_WASM_BYTES={}",
    vizij_orchestrator_wasm.display()
  );
  println!("cargo:rerun-if-changed=build.rs");
}
