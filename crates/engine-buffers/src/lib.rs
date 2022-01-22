use bytes::{BufMut, Buf};

const TYPE_UNIT: u8 = 0;
const TYPE_BOOLEAN: u8 = 1;
const TYPE_U8: u8 = 2;
const TYPE_U16: u8 = 3;
const TYPE_U32: u8 = 4;
const TYPE_U64: u8 = 5;
const TYPE_S8: u8 = 6;
const TYPE_S16: u8 = 7;
const TYPE_S32: u8 = 8;
const TYPE_S64: u8 = 9;
const TYPE_R32: u8 = 10;
const TYPE_R64: u8 = 11;
const TYPE_STRING: u8 = 12;
const TYPE_STRUCTURE: u8 = 13;
const TYPE_ENUMERATION: u8 = 14;


pub struct BufferWriter {
  backing: Vec<u8>,
}

impl BufferWriter {
  pub fn begin_structure(&mut self, id: &[u8], field_count: u32) {
    assert_eq!(id.len(), 16);

    self.backing.put_u8(TYPE_STRUCTURE);
    self.backing.put_slice(&id);
    self.backing.put_u32(field_count);
  }

  pub fn add_enumeration_value(&mut self, id: &[u8], value_id: &[u8]) {
    assert_eq!(id.len(), 16);
    assert_eq!(value_id.len(), 16);

    self.backing.put_u8(TYPE_ENUMERATION);
    self.backing.put_slice(&id);
    self.backing.put_slice(&value_id);
  }

  pub fn add_structure_field(&mut self, id: &[u8]) {
    assert_eq!(id.len(), 16);

    self.backing.put_slice(&id);
  }

  pub fn add_u8(&mut self, value: u8) {
    self.backing.put_u8(TYPE_U8);
    self.backing.put_u8(value);
  }

  pub fn add_u16(&mut self, value: u16) {
    self.backing.put_u8(TYPE_U16);
    self.backing.put_u16(value);
  }

  pub fn add_u32(&mut self, value: u32) {
    self.backing.put_u8(TYPE_U32);
    self.backing.put_u32(value);
  }

  pub fn add_u64(&mut self, value: u64) {
    self.backing.put_u8(TYPE_U64);
    self.backing.put_u64(value);
  }

  pub fn add_s8(&mut self, value: i8) {
    self.backing.put_u8(TYPE_S8);
    self.backing.put_i8(value);
  }

  pub fn add_s16(&mut self, value: i16) {
    self.backing.put_u8(TYPE_S16);
    self.backing.put_i16(value);
  }

  pub fn add_s32(&mut self, value: i32) {
    self.backing.put_u8(TYPE_S32);
    self.backing.put_i32(value);
  }

  pub fn add_s64(&mut self, value: i64) {
    self.backing.put_u8(TYPE_S64);
    self.backing.put_i64(value);
  }

  pub fn add_r32(&mut self, value: f32) {
    self.backing.put_u8(TYPE_R32);
    self.backing.put_f32(value);
  }

  pub fn add_r64(&mut self, value: f64) {
    self.backing.put_u8(TYPE_R64);
    self.backing.put_f64(value);
  }

  pub fn add_string(&mut self, value: &str) {
    self.backing.put_u8(TYPE_STRING);
    self.backing.put_u32(value.len() as u32);
    self.backing.put_slice(value.as_bytes());
  }
}

pub struct BufferReader<'a> {
  backing: &'a [u8],
}

impl<'a> BufferReader<'a> {
  pub fn next_type(&mut self) -> Option<u8> {
    if self.backing.len() == 0 {
      return None;
    }

    Some(self.backing.get_u8())
  }


}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_new() -> *mut BufferWriter {
  Box::into_raw(Box::new(BufferWriter {
    backing: Vec::new(),
  }))
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_free(writer: *mut BufferWriter) {
  unsafe {
    Box::from_raw(writer);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_begin_structure(writer: *mut BufferWriter, id: *const u8, field_count: u32) {
  unsafe {
    let writer = &mut *writer;
    let id = std::slice::from_raw_parts(id, 16);
    writer.begin_structure(id, field_count);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_enumeration_value(writer: *mut BufferWriter, id: *const u8, value_id: *const u8) {
  unsafe {
    let writer = &mut *writer;
    let id = std::slice::from_raw_parts(id, 16);
    let value_id = std::slice::from_raw_parts(value_id, 16);
    writer.add_enumeration_value(id, value_id);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_structure_field(writer: *mut BufferWriter, id: *const u8) {
  unsafe {
    let writer = &mut *writer;
    let id = std::slice::from_raw_parts(id, 16);
    writer.add_structure_field(id);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_u8(writer: *mut BufferWriter, value: u8) {
  unsafe {
    let writer = &mut *writer;
    writer.add_u8(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_u16(writer: *mut BufferWriter, value: u16) {
  unsafe {
    let writer = &mut *writer;
    writer.add_u16(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_u32(writer: *mut BufferWriter, value: u32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_u32(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_u64(writer: *mut BufferWriter, value: u64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_u64(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_s8(writer: *mut BufferWriter, value: i8) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s8(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_s16(writer: *mut BufferWriter, value: i16) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s16(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_s32(writer: *mut BufferWriter, value: i32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s32(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_s64(writer: *mut BufferWriter, value: i64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s64(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_r32(writer: *mut BufferWriter, value: f32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_r32(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_r64(writer: *mut BufferWriter, value: f64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_r64(value);
  }
}

#[no_mangle]
pub extern "C" fn engine_buffer_writer_add_string(writer: *mut BufferWriter, value: *const u8) {
  unsafe {
    let writer = &mut *writer;
    let value = std::slice::from_raw_parts(value, std::usize::MAX);
    writer.add_string(std::str::from_utf8(value).unwrap());
  }
}