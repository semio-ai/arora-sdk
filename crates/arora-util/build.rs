extern crate cbindgen;

use std::env;

use cbindgen::Language;

fn main() {
  let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

  cbindgen::Builder::new()
    .with_crate(crate_dir)
    .with_language(Language::C)
    .with_include_guard("_ARORA_UTIL_H_")
    .generate()
    .expect("Unable to generate bindings")
    .write_to_file("../../target/include/arora/util.h");
}
