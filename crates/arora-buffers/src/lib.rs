use bytes::{BufMut, Buf};
use uuid::Uuid;
use std::collections::VecDeque;

pub const TYPE_UNIT: u8 = 0;
pub const TYPE_BOOLEAN: u8 = 1;
pub const TYPE_U8: u8 = 2;
pub const TYPE_U16: u8 = 3;
pub const TYPE_U32: u8 = 4;
pub const TYPE_U64: u8 = 5;
pub const TYPE_S8: u8 = 6;
pub const TYPE_S16: u8 = 7;
pub const TYPE_S32: u8 = 8;
pub const TYPE_S64: u8 = 9;
pub const TYPE_R32: u8 = 10;
pub const TYPE_R64: u8 = 11;
pub const TYPE_STRING: u8 = 12;
pub const TYPE_STRUCTURE: u8 = 13;
pub const TYPE_ENUMERATION: u8 = 14;
pub const TYPE_ARRAY: u8 = 15;
pub const TYPE_MAP: u8 = 16;

const ALIGNMENT: usize = 8; 

pub struct BufferWriter {
  backing: Vec<u8>,
}

impl BufferWriter {
  pub fn new() -> Self {
    let mut backing = Vec::with_capacity(128);
    // size placeholder
    backing.put_u32(0);
    Self {
      backing,
    }
  }

  fn align(&mut self) {
    let alignment_buffer = ALIGNMENT - self.backing.len() % ALIGNMENT;
    for _ in 0..alignment_buffer {
      self.backing.put_u8(0);
    }
  }

  pub fn begin_structure_raw(&mut self, field_count: u32) {
    self.backing.put_u32(field_count);
  }

  pub fn begin_structure(&mut self, id: &[u8], field_count: u32) {
    assert_eq!(id.len(), 16);
    self.backing.put_u8(TYPE_STRUCTURE);
    self.backing.put_slice(&id);
    self.begin_structure_raw(field_count);
  }

  pub fn add_enumeration_value_raw(&mut self, value_id: &[u8]) {
    assert_eq!(value_id.len(), 16);
    self.backing.put_slice(&value_id);
  }

  pub fn add_enumeration_value(&mut self, id: &[u8], value_id: &[u8]) {
    assert_eq!(id.len(), 16);
    self.backing.put_u8(TYPE_ENUMERATION);
    self.backing.put_slice(&id);
    self.add_enumeration_value_raw(value_id);
  }

  pub fn add_structure_field(&mut self, id: &[u8]) {
    assert_eq!(id.len(), 16);
    self.backing.put_slice(&id);
  }

  pub fn add_unit(&mut self) {
    self.backing.put_u8(TYPE_UNIT);
  }

  pub fn add_boolean_raw(&mut self, value: bool) {
    self.backing.put_u8(if value { 1 } else { 0 });
  }

  pub fn add_boolean(&mut self, value: bool) {
    self.backing.put_u8(TYPE_BOOLEAN);
    self.add_boolean_raw(value);
  }

  pub fn add_boolean_raw_bulk(&mut self, values: &[bool]) {
    self.align();
    for value in values {
      self.add_boolean_raw(*value);
    }
  }

  pub fn add_u8_raw(&mut self, value: u8) {
    self.backing.put_u8(value);
  }

  pub fn add_u8(&mut self, value: u8) {
    self.backing.put_u8(TYPE_U8);
    self.add_u8_raw(value);
  }

  pub fn add_u8_raw_bulk(&mut self, values: &[u8]) {
    self.align();
    for value in values {
      self.add_u8_raw(*value);
    }
  }

  pub fn add_u16_raw(&mut self, value: u16) {
    self.backing.put_u16_le(value);
  }

  pub fn add_u16(&mut self, value: u16) {
    self.backing.put_u8(TYPE_U16);
    self.add_u16_raw(value);
  }

  pub fn add_u16_raw_bulk(&mut self, values: &[u16]) {
    self.align();
    for value in values {
      self.add_u16_raw(*value);
    }
  }

  pub fn add_u32_raw(&mut self, value: u32) {
    self.backing.put_u32_le(value);
  }

  pub fn add_u32(&mut self, value: u32) {
    self.backing.put_u8(TYPE_U32);
    self.add_u32_raw(value);
  }

  pub fn add_u32_raw_bulk(&mut self, values: &[u32]) {
    self.align();
    for value in values {
      self.add_u32_raw(*value);
    }
  }

  pub fn add_u64_raw(&mut self, value: u64) {
    self.backing.put_u64_le(value);
  }

  pub fn add_u64(&mut self, value: u64) {
    self.backing.put_u8(TYPE_U64);
    self.add_u64_raw(value);
  }

  pub fn add_u64_raw_bulk(&mut self, values: &[u64]) {
    self.align();
    for value in values {
      self.add_u64_raw(*value);
    }
  }

  pub fn add_s8_raw(&mut self, value: i8) {
    self.backing.put_i8(value);
  }

  pub fn add_s8(&mut self, value: i8) {
    self.backing.put_u8(TYPE_S8);
    self.add_s8_raw(value);
  }

  pub fn add_s8_raw_bulk(&mut self, values: &[i8]) {
    self.align();
    for value in values {
      self.add_s8_raw(*value);
    }
  }

  pub fn add_s16_raw(&mut self, value: i16) {
    self.backing.put_i16_le(value);
  }

  pub fn add_s16(&mut self, value: i16) {
    self.backing.put_u8(TYPE_S16);
    self.add_s16_raw(value);
  }

  pub fn add_s16_raw_bulk(&mut self, values: &[i16]) {
    self.align();
    for value in values {
      self.add_s16_raw(*value);
    }
  }

  pub fn add_s32_raw(&mut self, value: i32) {
    self.backing.put_i32_le(value);
  }

  pub fn add_s32(&mut self, value: i32) {
    self.backing.put_u8(TYPE_S32);
    self.add_s32_raw(value);
  }

  pub fn add_s32_raw_bulk(&mut self, values: &[i32]) {
    self.align();
    for value in values {
      self.add_s32_raw(*value);
    }
  }

  pub fn add_s64_raw(&mut self, value: i64) {
    self.backing.put_i64_le(value);
  }

  pub fn add_s64(&mut self, value: i64) {
    self.backing.put_u8(TYPE_S64);
    self.add_s64_raw(value);
  }

  pub fn add_s64_raw_bulk(&mut self, values: &[i64]) {
    self.align();
    for value in values {
      self.add_s64_raw(*value);
    }
  }

  pub fn add_r32_raw(&mut self, value: f32) {
    self.backing.put_f32(value);
  }

  pub fn add_r32(&mut self, value: f32) {
    self.backing.put_u8(TYPE_R32);
    self.add_r32_raw(value);
  }

  pub fn add_r32_raw_bulk(&mut self, values: &[f32]) {
    self.align();
    for value in values {
      self.add_r32_raw(*value);
    }
  }

  pub fn add_r64_raw(&mut self, value: f64) {
    self.backing.put_u8(TYPE_R64);
    self.backing.put_f64(value);
  }

  pub fn add_r64(&mut self, value: f64) {
    self.backing.put_u8(TYPE_R64);
    self.add_r64_raw(value);
  }

  pub fn add_r64_raw_bulk(&mut self, values: &[f64]) {
    self.align();
    for value in values {
      self.add_r64_raw(*value);
    }
  }

  pub fn add_string_raw(&mut self, value: &str) {
    self.backing.put_u32_le(value.len() as u32);
    self.backing.put_slice(value.as_bytes());
  }

  pub fn add_string(&mut self, value: &str) {
    self.backing.put_u8(TYPE_STRING);
    self.add_string_raw(value);
  }

  pub fn add_string_raw_bulk(&mut self, values: &[&str]) {
    self.align();
    for value in values {
      self.add_string_raw(value);
    }
  }

  pub fn add_array_primitive(&mut self, ty: u8, element_count: u32) {
    self.backing.put_u8(TYPE_ARRAY);
    self.backing.put_u8(ty);
    self.backing.put_u32_le(element_count);
  }

  pub fn add_array_structure(&mut self, ty_id: &[u8], element_count: u32) {
    assert_eq!(ty_id.len(), 16);
    self.backing.put_u8(TYPE_ARRAY);
    self.backing.put_u8(TYPE_STRUCTURE);
    self.backing.put_u32(element_count);
    self.backing.put_slice(ty_id);
  }

  pub fn add_array_enumeration(&mut self, ty_id: &[u8], element_count: u32) {
    assert_eq!(ty_id.len(), 16);
    self.backing.put_u8(TYPE_ARRAY);
    self.backing.put_u8(TYPE_ENUMERATION);
    self.backing.put_u32(element_count);
    self.backing.put_slice(ty_id);
  }

  pub fn finalize(&mut self) -> &[u8] {
    let size = self.backing.len() as u32;
    self.backing[0..4].copy_from_slice(&size.to_be_bytes());
    &self.backing
  }
}

pub struct BufferReader<'a> {
  size: u32,
  backing: &'a [u8],
}

impl<'a> BufferReader<'a> {
  pub fn new(mut buffer: &'a [u8]) -> Self {
    let size = buffer.get_u32();
    Self {
      size,
      backing: buffer,
    }
  }

  pub fn align(&mut self) {
    let remainder = self.backing.len() % ALIGNMENT;
    self.backing = &self.backing[remainder..];
  }

  pub fn next_type(&mut self) -> Option<u8> {
    if self.backing.len() == 0 {
      return None;
    }

    Some(self.backing.get_u8())
  }

  pub fn get_unit(&mut self) {
  }

  pub fn get_boolean(&mut self) -> bool {
    self.backing.get_u8() != 0
  }

  pub unsafe fn get_boolean_bulk(&mut self, count: usize) -> &[bool] {
    self.align();
    std::mem::transmute(&self.backing[0..count])
  }

  pub fn get_u8(&mut self) -> u8 {
    self.backing.get_u8()
  }

  pub fn get_u8_bulk(&mut self, count: usize) -> &'a [u8] {
    self.align();
    &self.backing[0..count]
  }

  pub fn get_u16(&mut self) -> u16 {
    self.backing.get_u16()
  }

  pub unsafe fn get_u16_bulk(&mut self, count: usize) -> &'a [u16] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 2])
  }

  pub fn get_u32(&mut self) -> u32 {
    self.backing.get_u32()
  }

  pub unsafe fn get_u32_bulk(&mut self, count: usize) -> &'a [u32] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 4])
  }

  pub fn get_u64(&mut self) -> u64 {
    self.backing.get_u64()
  }

  pub unsafe fn get_u64_bulk(&mut self, count: usize) -> &'a [u64] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 8])
  }

  pub fn get_s8(&mut self) -> i8 {
    self.backing.get_i8()
  }

  pub unsafe fn get_s8_bulk(&mut self, count: usize) -> &'a [i8] {
    self.align();
    std::mem::transmute(&self.backing[0..count])
  }

  pub fn get_s16(&mut self) -> i16 {
    self.backing.get_i16()
  }

  pub unsafe fn get_s16_bulk(&mut self, count: usize) -> &'a [i16] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 2])
  }

  pub fn get_s32(&mut self) -> i32 {
    self.backing.get_i32()
  }

  pub unsafe fn get_s32_bulk(&mut self, count: usize) -> &'a [i32] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 4])
  }

  pub fn get_s64(&mut self) -> i64 {
    self.backing.get_i64()
  }

  pub unsafe fn get_s64_bulk(&mut self, count: usize) -> &'a [i64] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 8])
  }

  pub fn get_r32(&mut self) -> f32 {
    self.backing.get_f32()
  }

  pub unsafe fn get_r32_bulk(&mut self, count: usize) -> &'a [f32] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 4])
  }

  pub fn get_r64(&mut self) -> f64 {
    self.backing.get_f64()
  }

  pub unsafe fn get_r64_bulk(&mut self, count: usize) -> &'a [f64] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 8])
  }

  pub fn get_string(&mut self) -> &'a str {
    let len = self.backing.get_u32();
    let ret = std::str::from_utf8(&self.backing[0..len as usize]).unwrap();
    self.backing.advance(len as usize);
    ret
  }

  pub fn get_structure(&mut self) -> (&'a [u8], u32) {
    let id = &self.backing[0..16];
    self.backing.advance(16);
    let field_count = self.backing.get_u32();
    (id, field_count)
  }

  pub fn get_structure_raw(&mut self) -> u32 {
    let field_count = self.backing.get_u32();
    field_count
  }

  pub fn get_structure_field(&mut self) -> &'a [u8] {
    let id = &self.backing[0..16];
    self.backing.advance(16);
    id
  }

  pub fn get_enumeration_value(&mut self) -> (&'a [u8], &'a [u8]) {
    let id = &self.backing[0..16];
    self.backing.advance(16);
    let value_id = &self.backing[0..16];
    self.backing.advance(16);
    (id, value_id)
  }
  
  pub fn get_enumeration_value_raw(&mut self) -> &'a [u8] {
    let id = &self.backing[0..16];
    self.backing.advance(16);
    id
  }

  pub fn get_array(&mut self) -> (u8, u32) {
    (self.backing.get_u8(), self.backing.get_u32())
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_new() -> *mut BufferWriter {
  Box::into_raw(Box::new(BufferWriter::new()))
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_free(writer: *mut BufferWriter) {
  unsafe {
    Box::from_raw(writer);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_begin_structure(writer: *mut BufferWriter, id: *const u8, field_count: u32) {
  unsafe {
    let writer = &mut *writer;
    let id = std::slice::from_raw_parts(id, 16);
    writer.begin_structure(id, field_count);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_enumeration_value(writer: *mut BufferWriter, id: *const u8, value_id: *const u8) {
  unsafe {
    let writer = &mut *writer;
    let id = std::slice::from_raw_parts(id, 16);
    let value_id = std::slice::from_raw_parts(value_id, 16);
    writer.add_enumeration_value(id, value_id);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_structure_field(writer: *mut BufferWriter, id: *const u8) {
  unsafe {
    let writer = &mut *writer;
    let id = std::slice::from_raw_parts(id, 16);
    writer.add_structure_field(id);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_array_primitive(writer: *mut BufferWriter, element_type: u8, element_count: u32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_array_primitive(element_type, element_count);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_array_structure(writer: *mut BufferWriter, id: *const u8, element_count: u32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_array_structure(std::slice::from_raw_parts(id, 16), element_count);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_array_enumeration(writer: *mut BufferWriter, id: *const u8, element_count: u32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_array_enumeration(std::slice::from_raw_parts(id, 16), element_count);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_begin_structure_raw(writer: *mut BufferWriter, field_count: u32) {
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
pub extern "C" fn arora_buffer_writer_add_boolean_raw_bulk(writer: *mut BufferWriter, values: *const bool, count: usize) {
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
pub extern "C" fn arora_buffer_writer_add_u8_raw_bulk(writer: *mut BufferWriter, values: *const u8, count: usize) {
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
pub extern "C" fn arora_buffer_writer_add_u16_raw_bulk(writer: *mut BufferWriter, values: *const u16, count: usize) {
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
pub extern "C" fn arora_buffer_writer_add_u32_raw_bulk(writer: *mut BufferWriter, values: *const u32, count: usize) {
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
pub extern "C" fn arora_buffer_writer_add_s8(writer: *mut BufferWriter, value: i8) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s8(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s8_raw(writer: *mut BufferWriter, value: i8) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s8_raw(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s8_raw_bulk(writer: *mut BufferWriter, values: *const i8, count: usize) {
  unsafe {
    let writer = &mut *writer;
    let values = std::slice::from_raw_parts(values, count);
    writer.add_s8_raw_bulk(values);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s16(writer: *mut BufferWriter, value: i16) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s16(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s16_raw(writer: *mut BufferWriter, value: i16) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s16_raw(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s16_raw_bulk(writer: *mut BufferWriter, values: *const i16, count: usize) {
  unsafe {
    let writer = &mut *writer;
    let values = std::slice::from_raw_parts(values, count);
    writer.add_s16_raw_bulk(values);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s32(writer: *mut BufferWriter, value: i32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s32(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s32_raw(writer: *mut BufferWriter, value: i32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s32_raw(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s32_raw_bulk(writer: *mut BufferWriter, values: *const i32, count: usize) {
  unsafe {
    let writer = &mut *writer;
    let values = std::slice::from_raw_parts(values, count);
    writer.add_s32_raw_bulk(values);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s64(writer: *mut BufferWriter, value: i64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s64(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s64_raw(writer: *mut BufferWriter, value: i64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s64_raw(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_s64_raw_bulk(writer: *mut BufferWriter, values: *const i64, count: usize) {
  unsafe {
    let writer = &mut *writer;
    let values = std::slice::from_raw_parts(values, count);
    writer.add_s64_raw_bulk(values);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_r32(writer: *mut BufferWriter, value: f32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_r32(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_r32_raw(writer: *mut BufferWriter, value: f32) {
  unsafe {
    let writer = &mut *writer;
    writer.add_r32_raw(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_r32_raw_bulk(writer: *mut BufferWriter, values: *const f32, count: usize) {
  unsafe {
    let writer = &mut *writer;
    let values = std::slice::from_raw_parts(values, count);
    writer.add_r32_raw_bulk(values);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_r64(writer: *mut BufferWriter, value: f64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_r64(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_r64_raw(writer: *mut BufferWriter, value: f64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_r64_raw(value);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_r64_raw_bulk(writer: *mut BufferWriter, values: *const f64, count: usize) {
  unsafe {
    let writer = &mut *writer;
    let values = std::slice::from_raw_parts(values, count);
    writer.add_r64_raw_bulk(values);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_add_string(writer: *mut BufferWriter, value: *const u8, size: u32) {
  unsafe {
    let writer = &mut *writer;
    let value = std::slice::from_raw_parts(value, size as usize);
    writer.add_string(std::str::from_utf8(value).unwrap());
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_writer_finalize(writer: *mut BufferWriter, length: *mut usize) -> *const u8 {
  unsafe {
    let writer = &mut *writer;
    let backing = writer.finalize();
    if !length.is_null() {
      *length = backing.len();
    }
    backing.as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_new<'a>(buffer: *const u8) -> *mut BufferReader<'a> {
  let size_buf: &[u8] = unsafe { std::slice::from_raw_parts(buffer, 4) };
  let size = u32::from_be_bytes(size_buf.try_into().unwrap());
  unsafe {
    Box::into_raw(Box::new(BufferReader::new(std::slice::from_raw_parts(buffer, size as usize))))
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_free(reader: *mut BufferReader) {
  unsafe {
    Box::from_raw(reader);
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_next_type(reader: *mut BufferReader) -> i16 {
  unsafe {
    let reader = &mut *reader;
    match reader.next_type() {
      Some(value) => value as i16,
      None => -1,
    }
  }
}

#[repr(C)]
pub struct GetStructureResult {
  pub id: *const u8,
  pub field_count: u32,
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_structure(reader: *mut BufferReader) -> GetStructureResult {
  unsafe {
    let reader = &mut *reader;
    let (id, field_count) = reader.get_structure();
    GetStructureResult {
      id: id.as_ptr(),
      field_count,
    }
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_structure_raw(reader: *mut BufferReader) -> u32 {
  unsafe {
    let reader = &mut *reader;
    let field_count = reader.get_structure_raw();
    field_count
  }
}

#[repr(C)]
pub struct GetEnumerationValueResult {
  pub id: *const u8,
  pub value_id: *const u8,
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_enumeration_value(reader: *mut BufferReader) -> GetEnumerationValueResult {
  unsafe {
    let reader = &mut *reader;
    let (id, value_id) = reader.get_enumeration_value();
    GetEnumerationValueResult {
      id: id.as_ptr(),
      value_id: value_id.as_ptr(),
    }
  }
}

#[repr(C)]
pub struct GetArrayResult {
  pub ty: u8,
  pub element_count: u32,
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_array(reader: *mut BufferReader) -> GetArrayResult {
  unsafe {
    let reader = &mut *reader;
    let (ty, element_count) = reader.get_array();
    GetArrayResult {
      ty,
      element_count,
    }
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_structure_field(reader: *mut BufferReader) -> *const u8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_structure_field().as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_boolean(reader: *mut BufferReader) -> bool {
  unsafe {
    let reader = &mut *reader;
    reader.get_boolean()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u8(reader: *mut BufferReader) -> u8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u8()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u8_bulk(reader: *mut BufferReader, count: usize) -> *const u8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u8_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u16(reader: *mut BufferReader) -> u16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u16()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u16_bulk(reader: *mut BufferReader, count: usize) -> *const u16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u16_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u32(reader: *mut BufferReader) -> u32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u32()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u32_bulk(reader: *mut BufferReader, count: usize) -> *const u32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u32_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u64(reader: *mut BufferReader) -> u64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u64()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_u64_bulk(reader: *mut BufferReader, count: usize) -> *const u64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u64_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s8(reader: *mut BufferReader) -> i8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s8()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s8_bulk(reader: *mut BufferReader, count: usize) -> *const i8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s8_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s16(reader: *mut BufferReader) -> i16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s16()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s16_bulk(reader: *mut BufferReader, count: usize) -> *const i16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s16_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s32(reader: *mut BufferReader) -> i32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s32()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s32_bulk(reader: *mut BufferReader, count: usize) -> *const i32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s32_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s64(reader: *mut BufferReader) -> i64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s64()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_s64_bulk(reader: *mut BufferReader, count: usize) -> *const i64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s64_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_r32(reader: *mut BufferReader) -> f32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_r32()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_r32_bulk(reader: *mut BufferReader, count: usize) -> *const f32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_r32_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_r64(reader: *mut BufferReader) -> f64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_r64()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_r64_bulk(reader: *mut BufferReader, count: usize) -> *const f64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_r64_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_string(reader: *mut BufferReader, length: *mut u32) -> *const u8 {
  unsafe {
    let reader = &mut *reader;
    let string = reader.get_string();
    *length = string.len() as u32;
    string.as_ptr()
  }
}

#[derive(Debug, Clone)]
pub struct StructureField<'a> {
  pub id: &'a [u8],
  pub value: Value<'a>,
}

#[derive(Debug, Clone)]
pub struct Structure<'a> {
  pub id: &'a [u8],
  pub fields: Vec<StructureField<'a>>,
}

#[derive(Debug, Clone)]
pub struct Enumeration<'a> {
  pub id: &'a [u8],
  pub variant_id: &'a [u8],
  pub value: Value<'a>,
}

#[derive(Debug, Clone)]
pub enum Value<'a> {
  Unit,
  Boolean(bool),
  U8(u8),
  U16(u16),
  U32(u32),
  U64(u64),
  S8(i8),
  S16(i16),
  S32(i32),
  S64(i64),
  R32(f32),
  R64(f64),
  String(&'a str),
  Structure(Structure<'a>),
  ArrayU8(&'a [u8]),
  ArrayU16(&'a [u16]),
  ArrayU32(&'a [u32]),
  ArrayU64(&'a [u64]),
  ArrayS8(&'a [i8]),
  ArrayS16(&'a [i16]),
  ArrayS32(&'a [i32]),
  ArrayS64(&'a [i64]),
  ArrayR32(&'a [f32]),
  ArrayR64(&'a [f64]),
  ArrayString(Vec<&'a str>),
  ArrayStructure(Vec<Structure<'a>>),
}

impl<'a> Value<'a> {
  pub unsafe fn parse(data: &'a [u8]) -> Value {
    let mut reader = BufferReader::new(data);
    match reader.next_type() {
      Some(TYPE_U8) => Value::U8(reader.get_u8()),
      Some(TYPE_U16) => Value::U16(reader.get_u16()),
      Some(TYPE_U32) => Value::U32(reader.get_u32()),
      Some(TYPE_U64) => Value::U64(reader.get_u64()),
      Some(TYPE_S8) => Value::S8(reader.get_s8()),
      Some(TYPE_S16) => Value::S16(reader.get_s16()),
      Some(TYPE_S32) => Value::S32(reader.get_s32()),
      Some(TYPE_S64) => Value::S64(reader.get_s64()),
      Some(TYPE_R32) => Value::R32(reader.get_r32()),
      Some(TYPE_R64) => Value::R64(reader.get_r64()),
      Some(TYPE_STRING) => Value::String(reader.get_string()),
      Some(TYPE_STRUCTURE) => {
        let (id, field_count) = reader.get_structure();
        let mut fields = Vec::with_capacity(field_count as usize);
        for _ in 0..field_count {
          let field_id = reader.get_structure_field();
          fields.push(StructureField {
            id: field_id,
            value: Value::parse(reader.backing),
          });
        }
        Value::Structure(Structure {
          id: id,
          fields: fields,
        })  
      },
      Some(TYPE_ARRAY) => {
        let (ty, count) = reader.get_array();
        match ty {
          TYPE_U8 => Value::ArrayU8(reader.get_u8_bulk(count as usize)),
          TYPE_U16 => Value::ArrayU16(reader.get_u16_bulk(count as usize)),
          TYPE_U32 => Value::ArrayU32(reader.get_u32_bulk(count as usize)),
          TYPE_U64 => Value::ArrayU64(reader.get_u64_bulk(count as usize)),
          TYPE_S8 => Value::ArrayS8(reader.get_s8_bulk(count as usize)),
          TYPE_S16 => Value::ArrayS16(reader.get_s16_bulk(count as usize)),
          TYPE_S32 => Value::ArrayS32(reader.get_s32_bulk(count as usize)),
          TYPE_S64 => Value::ArrayS64(reader.get_s64_bulk(count as usize)),
          TYPE_R32 => Value::ArrayR32(reader.get_r32_bulk(count as usize)),
          TYPE_R64 => Value::ArrayR64(reader.get_r64_bulk(count as usize)),
          TYPE_STRING => Value::ArrayString({
            let mut strings = Vec::with_capacity(count as usize);
            for _ in 0..count {
              strings.push(reader.get_string());
            }
            strings
          }),
          TYPE_STRUCTURE => Value::ArrayStructure({
            let mut structures = Vec::with_capacity(count as usize);
            let structure_id = reader.get_structure_field();
            for _ in 0..count {
              let field_count = reader.get_structure_raw();
              let mut fields = Vec::with_capacity(field_count as usize);
              for _ in 0..field_count {
                let field_id = reader.get_structure_field();
                fields.push(StructureField {
                  id: field_id,
                  value: Value::parse(reader.backing),
                });
              }
              structures.push(Structure {
                id: structure_id,
                fields: fields,
              });
            }
            structures
          }),
          _ => panic!("unsupported array type"),
        }
      },
      _ => panic!("Invalid type")
    }
  }
}