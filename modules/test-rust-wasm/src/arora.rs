// Let us pretend this is an auto-generated file.
//====================================================================================
// For writing buffers to pass data between modules.
use arora_buffers::BufferWriter;
// To access the module's implementation.
use super::*;

#[no_mangle]
pub extern "C" fn arora_function_5f423ba9_d5f9_46d7_a9b5_fb7d28f99ea6(_: i32) -> i32 {
  ping();
  let mut writer = BufferWriter::new();
  writer.add_unit();
  let result_buffer = writer.finalize();
  result_buffer.as_ptr() as i32
}

#[no_mangle]
pub extern "C" fn arora_function_00cd31a8_2cf4_48e6_a957_69a55de90424(_: i32) -> i32 {
  let result = succeed();
  let result_buffer: Box<[u8]> = result.into();
  result_buffer.as_ptr() as i32
}
