extern crate cbindgen;

use std::env;

fn main() {
  if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
    return;
  }

  let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

  cbindgen::Builder::new()
    .with_crate(crate_dir)
    .generate()
    .expect("Unable to generate bindings")
    .write_to_file("../../target/include/arora/arora.h");
}
