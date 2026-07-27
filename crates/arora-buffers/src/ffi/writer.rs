//! The writer half of the arora-buffers C ABI — raw-pointer entry points
//! over [`BufferWriter`](crate::writer::BufferWriter). See [`crate::ffi`].
use crate::writer::BufferWriter;

#[no_mangle]
pub extern "C" fn arora_buffer_writer_new() -> *mut BufferWriter {
    Box::into_raw(Box::new(BufferWriter::new()))
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_free(writer: *mut BufferWriter) {
    unsafe {
        drop(Box::from_raw(writer));
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_unit(writer: *mut BufferWriter) {
    unsafe {
        (*writer).add_unit();
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_begin_structure(
    writer: *mut BufferWriter,
    id: *const u8,
    field_count: u32,
) {
    unsafe {
        let writer = &mut *writer;
        let id = std::slice::from_raw_parts(id, 16);
        writer.begin_structure(id, field_count);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_enumeration_value(
    writer: *mut BufferWriter,
    id: *const u8,
    value_id: *const u8,
) {
    unsafe {
        let writer = &mut *writer;
        let id = std::slice::from_raw_parts(id, 16);
        let value_id = std::slice::from_raw_parts(value_id, 16);
        writer.add_enumeration_value(id, value_id);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_structure_field(
    writer: *mut BufferWriter,
    id: *const u8,
) {
    unsafe {
        let writer = &mut *writer;
        let id = std::slice::from_raw_parts(id, 16);
        writer.add_structure_field(id);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_array_primitive(
    writer: *mut BufferWriter,
    element_type: u8,
    element_count: u32,
) {
    unsafe {
        let writer = &mut *writer;
        writer.add_array_primitive(element_type, element_count);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_array_structure(
    writer: *mut BufferWriter,
    id: *const u8,
    element_count: u32,
) {
    unsafe {
        let writer = &mut *writer;
        writer.add_array_structure(std::slice::from_raw_parts(id, 16), element_count);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_array_enumeration(
    writer: *mut BufferWriter,
    id: *const u8,
    element_count: u32,
) {
    unsafe {
        let writer = &mut *writer;
        writer.add_array_enumeration(std::slice::from_raw_parts(id, 16), element_count);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_begin_structure_raw(
    writer: *mut BufferWriter,
    field_count: u32,
) {
    unsafe {
        let writer = &mut *writer;
        writer.begin_structure_raw(field_count);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_boolean(writer: *mut BufferWriter, value: bool) {
    unsafe {
        let writer = &mut *writer;
        writer.add_boolean(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_boolean_raw(writer: *mut BufferWriter, value: bool) {
    unsafe {
        let writer = &mut *writer;
        writer.add_boolean_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_boolean_raw_bulk(
    writer: *mut BufferWriter,
    values: *const bool,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_boolean_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u8(writer: *mut BufferWriter, value: u8) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u8(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u8_raw(writer: *mut BufferWriter, value: u8) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u8_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u8_raw_bulk(
    writer: *mut BufferWriter,
    values: *const u8,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_u8_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u16(writer: *mut BufferWriter, value: u16) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u16(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u16_raw(writer: *mut BufferWriter, value: u16) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u16_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u16_raw_bulk(
    writer: *mut BufferWriter,
    values: *const u16,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_u16_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u32(writer: *mut BufferWriter, value: u32) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u32(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u32_raw(writer: *mut BufferWriter, value: u32) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u32_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u32_raw_bulk(
    writer: *mut BufferWriter,
    values: *const u32,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_u32_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u64(writer: *mut BufferWriter, value: u64) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u64(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u64_raw(writer: *mut BufferWriter, value: u64) {
    unsafe {
        let writer = &mut *writer;
        writer.add_u64_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_u64_raw_bulk(
    writer: *mut BufferWriter,
    values: *const u64,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_u64_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i8(writer: *mut BufferWriter, value: i8) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i8(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i8_raw(writer: *mut BufferWriter, value: i8) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i8_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i8_raw_bulk(
    writer: *mut BufferWriter,
    values: *const i8,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_i8_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i16(writer: *mut BufferWriter, value: i16) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i16(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i16_raw(writer: *mut BufferWriter, value: i16) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i16_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i16_raw_bulk(
    writer: *mut BufferWriter,
    values: *const i16,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_i16_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i32(writer: *mut BufferWriter, value: i32) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i32(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i32_raw(writer: *mut BufferWriter, value: i32) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i32_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i32_raw_bulk(
    writer: *mut BufferWriter,
    values: *const i32,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_i32_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i64(writer: *mut BufferWriter, value: i64) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i64(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i64_raw(writer: *mut BufferWriter, value: i64) {
    unsafe {
        let writer = &mut *writer;
        writer.add_i64_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_i64_raw_bulk(
    writer: *mut BufferWriter,
    values: *const i64,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_i64_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_f32(writer: *mut BufferWriter, value: f32) {
    unsafe {
        let writer = &mut *writer;
        writer.add_f32(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_f32_raw(writer: *mut BufferWriter, value: f32) {
    unsafe {
        let writer = &mut *writer;
        writer.add_f32_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_f32_raw_bulk(
    writer: *mut BufferWriter,
    values: *const f32,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_f32_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_f64(writer: *mut BufferWriter, value: f64) {
    unsafe {
        let writer = &mut *writer;
        writer.add_f64(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_f64_raw(writer: *mut BufferWriter, value: f64) {
    unsafe {
        let writer = &mut *writer;
        writer.add_f64_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_f64_raw_bulk(
    writer: *mut BufferWriter,
    values: *const f64,
    count: usize,
) {
    unsafe {
        let writer = &mut *writer;
        let values = std::slice::from_raw_parts(values, count);
        writer.add_f64_raw_bulk(values);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_string(
    writer: *mut BufferWriter,
    value: *const u8,
    size: u32,
) {
    unsafe {
        let writer = &mut *writer;
        let value = std::slice::from_raw_parts(value, size as usize);
        writer.add_string(std::str::from_utf8(value).unwrap());
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_option_some(writer: *mut BufferWriter) {
    unsafe {
        let writer = &mut *writer;
        writer.add_option_some();
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_option_none(writer: *mut BufferWriter) {
    unsafe {
        let writer = &mut *writer;
        writer.add_option_none();
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_uuid(writer: *mut BufferWriter, id: *const u8) {
    unsafe {
        let writer = &mut *writer;
        writer.add_uuid(std::slice::from_raw_parts(id, 16));
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_begin_map(
    writer: *mut BufferWriter,
    id: *const u8,
    field_count: u32,
) {
    unsafe {
        let writer = &mut *writer;
        writer.begin_map(std::slice::from_raw_parts(id, 16), field_count);
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_map_field_key(
    writer: *mut BufferWriter,
    key: *const u8,
    key_len: u32,
) {
    unsafe {
        let writer = &mut *writer;
        let key = std::slice::from_raw_parts(key, key_len as usize);
        writer.add_map_field_key(std::str::from_utf8(key).unwrap());
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_error(
    writer: *mut BufferWriter,
    message: *const u8,
    message_len: u32,
) {
    unsafe {
        let writer = &mut *writer;
        let message = std::slice::from_raw_parts(message, message_len as usize);
        writer.add_error(std::str::from_utf8(message).unwrap_or("invalid utf-8 in error"));
    }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_finalize(
    writer: *mut BufferWriter,
    length: *mut usize,
) -> *mut u8 {
    unsafe {
        let writer = &mut *writer;
        let backing = writer.finalize();
        if !length.is_null() {
            *length = backing.len();
        }
        Box::into_raw(backing) as *mut u8
    }
}
