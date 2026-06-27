extern crate cbindgen;

use std::env;
use std::path::Path;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // Expose test-rust-wasm's low-level header to the wasm32 browser test
    // (tests/browser.rs) for `include_str!`. Harmless on other targets.
    let header_yaml =
        Path::new(&crate_dir).join("../../modules/test-rust-wasm/src/arora_generated/module.yaml");
    println!(
        "cargo:rustc-env=TEST_RUST_WASM_HEADER_YAML={}",
        header_yaml.display()
    );
    println!("cargo:rerun-if-changed=build.rs");

    // cbindgen generates the C ABI header; the browser host has no C ABI.
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("../../target/include/arora/arora.h");
}
