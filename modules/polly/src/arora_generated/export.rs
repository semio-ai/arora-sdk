use crate::{arora_generated, hello_world};
use arora_buffers::*;
#[doc = "hello_world"]
#[no_mangle]
pub extern "C" fn arora_function_e5a41333_4848_411f_878c_f1d662ebb4a0(input_addr: i32) -> i32 {
  let input_ptr = input_addr as *const u8;
  const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
  let input_size_bytes: &[u8; 4] =
    unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
      .try_into()
      .expect("input is too small");
  let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
  let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
  let mut reader = BufferReader::new(&input);
  let type_raw_id_opt = reader.next_type();
  assert!(!type_raw_id_opt.is_none());
  assert_eq!(type_raw_id_opt.unwrap(), TYPE_STRUCTURE);
  let (structure_raw_id, field_count) = reader.get_structure();
  assert_eq!(HELLO_WORLD_FUNCTION_RAW_ID, structure_raw_id);
  assert_eq!(0, field_count);
  let mut writer = BufferWriter::new();
  writer.begin_structure(&HELLO_WORLD_FUNCTION_RAW_ID, (0usize + 1) as u32);
  writer.add_structure_field(&HELLO_WORLD_FUNCTION_RAW_ID);
  let result = hello_world();
  arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
  let result_buffer = writer.finalize();
  Box::leak(result_buffer).as_ptr() as i32
}
#[doc = "hello_world: e5a41333-4848-411f-878c-f1d662ebb4a0"]
pub const HELLO_WORLD_FUNCTION_RAW_ID: [u8; 16] = [
  0xe5, 0xa4, 0x13, 0x33, 0x48, 0x48, 0x41, 0x1f, 0x87, 0x8c, 0xf1, 0xd6, 0x62, 0xeb, 0xb4, 0xa0,
];
