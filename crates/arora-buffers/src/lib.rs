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

pub struct BufferWriter {
  backing: Vec<u8>,
}

impl BufferWriter {
  pub fn new() -> Self {
    let mut backing = Vec::new();
    // size placeholder
    backing.put_u32(0);
    Self {
      backing,
    }
  }

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

  pub fn add_unit(&mut self) {
    self.backing.put_u8(TYPE_UNIT);
  }

  pub fn add_boolean(&mut self, value: bool) {
    self.backing.put_u8(TYPE_BOOLEAN);
    self.backing.put_u8(if value { 1 } else { 0 });
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
  pub fn new(mut backing: &'a [u8]) -> Self {
    let size = backing.get_u32();
    Self {
      size,
      backing,
    }
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

  pub fn get_u8(&mut self) -> u8 {
    self.backing.get_u8()
  }

  pub fn get_u16(&mut self) -> u16 {
    self.backing.get_u16()
  }

  pub fn get_u32(&mut self) -> u32 {
    self.backing.get_u32()
  }

  pub fn get_u64(&mut self) -> u64 {
    self.backing.get_u64()
  }

  pub fn get_s8(&mut self) -> i8 {
    self.backing.get_i8()
  }

  pub fn get_s16(&mut self) -> i16 {
    self.backing.get_i16()
  }

  pub fn get_s32(&mut self) -> i32 {
    self.backing.get_i32()
  }

  pub fn get_s64(&mut self) -> i64 {
    self.backing.get_i64()
  }

  pub fn get_r32(&mut self) -> f32 {
    self.backing.get_f32()
  }

  pub fn get_r64(&mut self) -> f64 {
    self.backing.get_f64()
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
pub extern "C" fn arora_buffer_writer_add_boolean(writer: *mut BufferWriter, value: bool) {
  unsafe {
    let writer = &mut *writer;
    writer.add_boolean(value);
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
pub extern "C" fn arora_buffer_writer_add_u16(writer: *mut BufferWriter, value: u16) {
  unsafe {
    let writer = &mut *writer;
    writer.add_u16(value);
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
pub extern "C" fn arora_buffer_writer_add_u64(writer: *mut BufferWriter, value: u64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_u64(value);
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
pub extern "C" fn arora_buffer_writer_add_s16(writer: *mut BufferWriter, value: i16) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s16(value);
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
pub extern "C" fn arora_buffer_writer_add_s64(writer: *mut BufferWriter, value: i64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_s64(value);
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
pub extern "C" fn arora_buffer_writer_add_r64(writer: *mut BufferWriter, value: f64) {
  unsafe {
    let writer = &mut *writer;
    writer.add_r64(value);
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
pub extern "C" fn arora_buffer_reader_new<'a>(buffer: *const u8, size: usize) -> *mut BufferReader<'a> {
  unsafe {
    Box::into_raw(Box::new(BufferReader::new(std::slice::from_raw_parts(buffer, size))))
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
    println!("arora_buffer_reader_get_structure {:?} {}", id, field_count);
    GetStructureResult {
      id: id.as_ptr(),
      field_count,
    }
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
pub extern "C" fn arora_buffer_reader_get_u16(reader: *mut BufferReader) -> u16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u16()
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
pub extern "C" fn arora_buffer_reader_get_u64(reader: *mut BufferReader) -> u64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u64()
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
pub extern "C" fn arora_buffer_reader_get_s16(reader: *mut BufferReader) -> i16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s16()
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
pub extern "C" fn arora_buffer_reader_get_s64(reader: *mut BufferReader) -> i64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_s64()
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
pub extern "C" fn arora_buffer_reader_get_r64(reader: *mut BufferReader) -> f64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_r64()
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

pub struct BufferPrinter<'a> {
  reader: BufferReader<'a>,
}

impl <'a> BufferPrinter<'a> {
  pub fn new(reader: BufferReader<'a>) -> BufferPrinter<'a> {
    BufferPrinter {
      reader,
    }
  }

  pub fn print(&mut self, indent: usize) {
    match self.reader.next_type() {
      Some(TYPE_STRUCTURE) => {
        let (id, field_count) = self.reader.get_structure();
        println!("{}Structure {:?} {}", "  ".repeat(indent), id, field_count);
        for _ in 0..field_count {
          let field_id = Uuid::from_slice(self.reader.get_structure_field()).unwrap();
          println!("{}  Field {:?}", "  ".repeat(indent), field_id);
          self.print(indent + 2);
        }
      },
      Some(TYPE_ENUMERATION) => {
        let (id, value_id) = self.reader.get_enumeration_value();
        println!("{}Enumeration {:?} {:?}", "  ".repeat(indent), id, value_id);
      },
      Some(TYPE_BOOLEAN) => {
        println!("{}Boolean {}", "  ".repeat(indent), self.reader.get_boolean());
      },
      Some(TYPE_U8) => {
        println!("{}U8 {}", "  ".repeat(indent), self.reader.get_u8());
      },
      Some(TYPE_U16) => {
        println!("{}U16 {}", "  ".repeat(indent), self.reader.get_u16());
      },
      Some(TYPE_U32) => {
        println!("{}U32 {}", "  ".repeat(indent), self.reader.get_u32());
      },
      Some(TYPE_U64) => {
        println!("{}U64 {}", "  ".repeat(indent), self.reader.get_u64());
      },
      Some(TYPE_S8) => {
        println!("{}S8 {}", "  ".repeat(indent), self.reader.get_s8());
      },
      Some(TYPE_S16) => {
        println!("{}S16 {}", "  ".repeat(indent), self.reader.get_s16());
      },
      Some(TYPE_S32) => {
        println!("{}S32 {}", "  ".repeat(indent), self.reader.get_s32());
      },
      Some(TYPE_S64) => {
        println!("{}S64 {}", "  ".repeat(indent), self.reader.get_s64());
      },
      Some(TYPE_R32) => {
        println!("{}R32 {}", "  ".repeat(indent), self.reader.get_r32());
      },
      Some(TYPE_R64) => {
        println!("{}R64 {}", "  ".repeat(indent), self.reader.get_r64());
      },
      Some(TYPE_STRING) => {
        let length = self.reader.get_string().len();
        println!("{}String {}", "  ".repeat(indent), length);
      },
      _ => {}
    }
  }
}