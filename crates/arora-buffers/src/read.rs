use bytes::Buf;

use crate::ALIGNMENT;

pub struct BufferReader<'a> {
  backing: &'a [u8],
}

impl<'a> BufferReader<'a> {
  pub fn new(buffer: &'a [u8]) -> Self {
    Self {
      backing: &buffer[4..], // skip the first 4 bytes announcing the size
    }
  }

  pub fn align(&mut self) {
    let remainder = self.backing.len() % ALIGNMENT;
    self.backing = &self.backing[remainder..];
  }

  pub fn next_type(&mut self) -> Option<u8> {
    if self.backing.is_empty() {
      return None;
    }

    Some(self.backing.get_u8())
  }

  pub fn get_unit(&mut self) {}

  pub fn get_boolean(&mut self) -> bool {
    self.backing.get_u8() != 0
  }

  pub unsafe fn get_boolean_bulk(&mut self, count: usize) -> &'a [bool] {
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
    self.backing.get_u16_le()
  }

  pub unsafe fn get_u16_bulk(&mut self, count: usize) -> &'a [u16] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 2])
  }

  pub fn get_u32(&mut self) -> u32 {
    self.backing.get_u32_le()
  }

  pub unsafe fn get_u32_bulk(&mut self, count: usize) -> &'a [u32] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 4])
  }

  pub fn get_u64(&mut self) -> u64 {
    self.backing.get_u64_le()
  }

  pub unsafe fn get_u64_bulk(&mut self, count: usize) -> &'a [u64] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 8])
  }

  pub fn get_i8(&mut self) -> i8 {
    self.backing.get_i8()
  }

  pub unsafe fn get_i8_bulk(&mut self, count: usize) -> &'a [i8] {
    self.align();
    std::mem::transmute(&self.backing[0..count])
  }

  pub fn get_i16(&mut self) -> i16 {
    self.backing.get_i16_le()
  }

  pub unsafe fn get_i16_bulk(&mut self, count: usize) -> &'a [i16] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 2])
  }

  pub fn get_i32(&mut self) -> i32 {
    self.backing.get_i32_le()
  }

  pub unsafe fn get_i32_bulk(&mut self, count: usize) -> &'a [i32] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 4])
  }

  pub fn get_i64(&mut self) -> i64 {
    self.backing.get_i64_le()
  }

  pub unsafe fn get_i64_bulk(&mut self, count: usize) -> &'a [i64] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 8])
  }

  pub fn get_f32(&mut self) -> f32 {
    self.backing.get_f32_le()
  }

  pub unsafe fn get_f32_bulk(&mut self, count: usize) -> &'a [f32] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 4])
  }

  pub fn get_f64(&mut self) -> f64 {
    self.backing.get_f64_le()
  }

  pub unsafe fn get_f64_bulk(&mut self, count: usize) -> &'a [f64] {
    self.align();
    std::mem::transmute(&self.backing[0..count * 8])
  }

  pub fn get_string(&mut self) -> &'a str {
    let len = self.backing.get_u32_le();
    let ret = std::str::from_utf8(&self.backing[0..len as usize]).unwrap();
    self.backing.advance(len as usize);
    ret
  }

  pub fn get_structure(&mut self) -> (&'a [u8], u32) {
    let id = &self.backing[0..16];
    self.backing.advance(16);
    let field_count = self.get_structure_raw();
    (id, field_count)
  }

  pub fn get_structure_raw(&mut self) -> u32 {
    self.backing.get_u32_le()
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

  pub fn get_option_presence(&mut self) -> bool {
    self.backing.get_u8() != 0
  }

  pub fn get_uuid(&mut self) -> &'a [u8] {
    let id = &self.backing[0..16];
    self.backing.advance(16);
    id
  }

  pub fn get_map(&mut self) -> (&'a [u8], u32) {
    let id = &self.backing[0..16];
    self.backing.advance(16);
    let field_count = self.backing.get_u32_le();
    (id, field_count)
  }

  pub fn get_map_field_key(&mut self) -> &'a str {
    let len = self.backing.get_u32_le();
    let ret = std::str::from_utf8(&self.backing[0..len as usize]).unwrap();
    self.backing.advance(len as usize);
    ret
  }

  pub fn get_array(&mut self) -> (u8, u32) {
    (self.backing.get_u8(), self.backing.get_u32_le())
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_new<'a>(buffer: *const u8) -> *mut BufferReader<'a> {
  let size_buf: &[u8] = unsafe { std::slice::from_raw_parts(buffer, 4) };
  let size = u32::from_be_bytes(size_buf.try_into().unwrap());
  unsafe {
    Box::into_raw(Box::new(BufferReader::new(std::slice::from_raw_parts(
      buffer,
      size as usize,
    ))))
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_free(reader: *mut BufferReader) {
  unsafe {
    drop(Box::from_raw(reader));
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
pub extern "C" fn arora_buffer_reader_get_structure(
  reader: *mut BufferReader,
) -> GetStructureResult {
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

    reader.get_structure_raw()
  }
}

#[repr(C)]
pub struct GetEnumerationValueResult {
  pub id: *const u8,
  pub value_id: *const u8,
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_enumeration_value(
  reader: *mut BufferReader,
) -> GetEnumerationValueResult {
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
    GetArrayResult { ty, element_count }
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
pub extern "C" fn arora_buffer_reader_get_u8_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const u8 {
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
pub extern "C" fn arora_buffer_reader_get_u16_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const u16 {
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
pub extern "C" fn arora_buffer_reader_get_u32_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const u32 {
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
pub extern "C" fn arora_buffer_reader_get_u64_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const u64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_u64_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i8(reader: *mut BufferReader) -> i8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i8()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i8_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const i8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i8_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i16(reader: *mut BufferReader) -> i16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i16()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i16_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const i16 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i16_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i32(reader: *mut BufferReader) -> i32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i32()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i32_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const i32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i32_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i64(reader: *mut BufferReader) -> i64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i64()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_i64_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const i64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_i64_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_f32(reader: *mut BufferReader) -> f32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_f32()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_f32_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const f32 {
  unsafe {
    let reader = &mut *reader;
    reader.get_f32_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_f64(reader: *mut BufferReader) -> f64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_f64()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_f64_bulk(
  reader: *mut BufferReader,
  count: usize,
) -> *const f64 {
  unsafe {
    let reader = &mut *reader;
    reader.get_f64_bulk(count).as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_string(
  reader: *mut BufferReader,
  length: *mut u32,
) -> *const u8 {
  unsafe {
    let reader = &mut *reader;
    let string = reader.get_string();
    *length = string.len() as u32;
    string.as_ptr()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_option_presence(reader: *mut BufferReader) -> bool {
  unsafe {
    let reader = &mut *reader;
    reader.get_option_presence()
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_uuid(reader: *mut BufferReader) -> *const u8 {
  unsafe {
    let reader = &mut *reader;
    reader.get_uuid().as_ptr()
  }
}

#[repr(C)]
pub struct GetMapResult {
  pub id: *const u8,
  pub field_count: u32,
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_map(reader: *mut BufferReader) -> GetMapResult {
  unsafe {
    let reader = &mut *reader;
    let (id, field_count) = reader.get_map();
    GetMapResult {
      id: id.as_ptr(),
      field_count,
    }
  }
}

#[no_mangle]
pub extern "C" fn arora_buffer_reader_get_map_field_key(
  reader: *mut BufferReader,
  length: *mut u32,
) -> *const u8 {
  unsafe {
    let reader = &mut *reader;
    let key = reader.get_map_field_key();
    *length = key.len() as u32;
    key.as_ptr()
  }
}
