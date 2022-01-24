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
    .rename_item("TYPE_UNIT", "ARORA_BUFFER_TYPE_UNIT")
    .rename_item("TYPE_BOOLEAN", "ARORA_BUFFER_TYPE_BOOLEAN")
    .rename_item("TYPE_U8", "ARORA_BUFFER_TYPE_U8")
    .rename_item("TYPE_U16", "ARORA_BUFFER_TYPE_U16")
    .rename_item("TYPE_U32", "ARORA_BUFFER_TYPE_U32")
    .rename_item("TYPE_U64", "ARORA_BUFFER_TYPE_U64")
    .rename_item("TYPE_S8", "ARORA_BUFFER_TYPE_S8")
    .rename_item("TYPE_S16", "ARORA_BUFFER_TYPE_S16")
    .rename_item("TYPE_S32", "ARORA_BUFFER_TYPE_S32")
    .rename_item("TYPE_S64", "ARORA_BUFFER_TYPE_S64")
    .rename_item("TYPE_R32", "ARORA_BUFFER_TYPE_R32")
    .rename_item("TYPE_R64", "ARORA_BUFFER_TYPE_R64")
    .rename_item("TYPE_STRING", "ARORA_BUFFER_TYPE_STRING")
    .rename_item("TYPE_STRUCTURE", "ARORA_BUFFER_TYPE_STRUCTURE")
    .rename_item("TYPE_ENUMERATION", "ARORA_BUFFER_TYPE_ENUMERATION")
    .with_include_guard("_ARORA_BUFFER_H_")
    .generate()
    .expect("Unable to generate bindings")
    .write_to_file("../../target/include/arora/buffers.h");
}
