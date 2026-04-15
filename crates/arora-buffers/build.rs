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
    .rename_item(
      "GetEnumerationValueResult",
      "arora_get_enumeration_value_result",
    )
    .rename_item("GetArrayResult", "arora_get_array_result")
    .rename_item("TYPE_UNIT", "ARORA_BUFFER_TYPE_UNIT")
    .rename_item("TYPE_BOOLEAN", "ARORA_BUFFER_TYPE_BOOLEAN")
    .rename_item("TYPE_U8", "ARORA_BUFFER_TYPE_U8")
    .rename_item("TYPE_U16", "ARORA_BUFFER_TYPE_U16")
    .rename_item("TYPE_U32", "ARORA_BUFFER_TYPE_U32")
    .rename_item("TYPE_U64", "ARORA_BUFFER_TYPE_U64")
    .rename_item("TYPE_I8", "ARORA_BUFFER_TYPE_I8")
    .rename_item("TYPE_I16", "ARORA_BUFFER_TYPE_I16")
    .rename_item("TYPE_I32", "ARORA_BUFFER_TYPE_I32")
    .rename_item("TYPE_I64", "ARORA_BUFFER_TYPE_I64")
    .rename_item("TYPE_F32", "ARORA_BUFFER_TYPE_F32")
    .rename_item("TYPE_F64", "ARORA_BUFFER_TYPE_F64")
    .rename_item("TYPE_STRING", "ARORA_BUFFER_TYPE_STRING")
    .rename_item("TYPE_STRUCTURE", "ARORA_BUFFER_TYPE_STRUCTURE")
    .rename_item("TYPE_ENUMERATION", "ARORA_BUFFER_TYPE_ENUMERATION")
    .rename_item("TYPE_ARRAY", "ARORA_BUFFER_TYPE_ARRAY")
    .rename_item("TYPE_MAP", "ARORA_BUFFER_TYPE_MAP")
    .rename_item("TYPE_OPTION", "ARORA_BUFFER_TYPE_OPTION")
    .rename_item("TYPE_UUID", "ARORA_BUFFER_TYPE_UUID")
    .rename_item("TYPE_VALUE", "ARORA_BUFFER_TYPE_VALUE")
    .rename_item("GetMapResult", "arora_get_map_result")
    .with_include_guard("_ARORA_BUFFER_H_")
    .generate()
    .expect("Unable to generate bindings")
    .write_to_file("../../target/include/arora/buffers.h");
}
