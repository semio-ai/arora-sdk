use crate::arora_generated::arora::arora_dispatch;
use arora_buffers::*;
#[doc = "test-rust-wasm: 665d6ec9-3fc9-4cfa-9100-5c8964e95aec"]
const TEST_RUST_WASM_MODULE_ID: [u8; 16] = [
    0x66, 0x5d, 0x6e, 0xc9, 0x3f, 0xc9, 0x4c, 0xfa, 0x91, 0x00, 0x5c, 0x89, 0x64, 0xe9, 0x5a, 0xec,
];
pub fn cos(angle: f32) -> f32 {
    #[doc = "cos: c13757cb-2311-4c93-abcc-cb12d6cbb859"]
    const COS_FUNCTION_RAW_ID: [u8; 16] = [
        0xc1, 0x37, 0x57, 0xcb, 0x23, 0x11, 0x4c, 0x93, 0xab, 0xcc, 0xcb, 0x12, 0xd6, 0xcb, 0xb8,
        0x59,
    ];
    #[doc = "cos.angle: 6c2a157c-4235-47b0-bff3-1eeef3e5747d"]
    const COS_ANGLE_PARAMETER_RAW_ID: [u8; 16] = [
        0x6c, 0x2a, 0x15, 0x7c, 0x42, 0x35, 0x47, 0xb0, 0xbf, 0xf3, 0x1e, 0xee, 0xf3, 0xe5, 0x74,
        0x7d,
    ];
    let mut writer = BufferWriter::new();
    writer.begin_structure(COS_FUNCTION_RAW_ID.as_slice(), 1u32);
    writer.add_structure_field(COS_ANGLE_PARAMETER_RAW_ID.as_slice());
    writer.add_f32(angle);
    let arg = writer.finalize();
    let result_buffer_addr = unsafe {
        arora_dispatch(
            TEST_RUST_WASM_MODULE_ID.as_ptr() as usize,
            COS_FUNCTION_RAW_ID.as_ptr() as usize,
            arg.as_ptr() as usize,
        )
    };
    let result_buffer_ptr = result_buffer_addr as *const u8;
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input =
        unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE + input_size) };
    let mut reader = BufferReader::new(&input);
    let type_raw_id_opt = reader.next_type();
    assert!(!type_raw_id_opt.is_none());
    assert_eq!(type_raw_id_opt.unwrap(), TYPE_STRUCTURE);
    let (result_struct_id, result_field_count) = reader.get_structure();
    assert_eq!(result_struct_id, COS_FUNCTION_RAW_ID);
    assert_eq!(result_field_count, 1u32);
    let first_field_id = reader.get_structure_field();
    assert_eq!(first_field_id, COS_FUNCTION_RAW_ID);
    let ret = {
        {
            let _next_type = reader.next_type();
            assert_eq!(_next_type, Some(TYPE_F32), "type mismatch");
        }
        reader.get_f32()
    };
    ret
}
