extern crate cbindgen;

use std::env;

use cbindgen::Language;

fn main() {
  let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

  cbindgen::Builder::new()
    .with_crate(crate_dir)
    .with_language(Language::C)
    .rename_item("BufferWriter", "arora_buffer_writer")
    .rename_item("BufferReader", "arora_buffer_reader")
    .rename_item("GetStructureResult", "arora_get_structure_result")
    .rename_item("GetEnumerationValueResult", "arora_get_enumeration_value_result")
    .generate()
    .expect("Unable to generate bindings")
    .write_to_file("../../target/include/arora/buffers.h");
}
