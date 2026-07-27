use bytes::Buf;

use crate::ALIGNMENT;

pub struct BufferReader<'a> {
    backing: &'a [u8],
    /// Total buffer length, including the 4-byte size prefix — the origin the
    /// writer aligns against, so [`align`](Self::align) can mirror it.
    buffer_len: usize,
}

impl<'a> BufferReader<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer_len: buffer.len(),
            backing: &buffer[4..], // skip the first 4 bytes announcing the size
        }
    }

    pub fn align(&mut self) {
        // The writer pads against its backing length, which *includes* the
        // 4-byte size prefix (see `BufferWriter::align`). Mirror that absolute
        // offset from the buffer start; measuring against the remaining length
        // instead only agrees when the whole buffer is a multiple of ALIGNMENT.
        let absolute_offset = self.buffer_len - self.backing.len();
        let pad = ALIGNMENT - absolute_offset % ALIGNMENT;
        self.backing = &self.backing[pad..];
    }

    pub fn next_type(&mut self) -> Option<u8> {
        if self.backing.is_empty() {
            return None;
        }

        Some(self.backing.get_u8())
    }

    pub fn get_unit(&mut self) {}

    /// Aligns, then takes `bytes` raw bytes and advances past them. The returned
    /// slice borrows the buffer in place — the backbone of the bulk readers, and
    /// the reason a bulk read leaves the cursor after the array (like every other
    /// `get_*`), so a following field reads from the right place.
    fn take_aligned_bytes(&mut self, bytes: usize) -> &'a [u8] {
        self.align();
        let block = &self.backing[0..bytes];
        self.backing = &self.backing[bytes..];
        block
    }

    pub fn get_boolean(&mut self) -> bool {
        self.backing.get_u8() != 0
    }

    pub unsafe fn get_boolean_bulk(&mut self, count: usize) -> &'a [bool] {
        std::mem::transmute(self.take_aligned_bytes(count))
    }

    pub fn get_u8(&mut self) -> u8 {
        self.backing.get_u8()
    }

    pub fn get_u8_bulk(&mut self, count: usize) -> &'a [u8] {
        self.take_aligned_bytes(count)
    }

    pub fn get_u16(&mut self) -> u16 {
        self.backing.get_u16_le()
    }

    pub unsafe fn get_u16_bulk(&mut self, count: usize) -> &'a [u16] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 2).as_ptr() as *const u16,
            count,
        )
    }

    pub fn get_u32(&mut self) -> u32 {
        self.backing.get_u32_le()
    }

    pub unsafe fn get_u32_bulk(&mut self, count: usize) -> &'a [u32] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 4).as_ptr() as *const u32,
            count,
        )
    }

    pub fn get_u64(&mut self) -> u64 {
        self.backing.get_u64_le()
    }

    pub unsafe fn get_u64_bulk(&mut self, count: usize) -> &'a [u64] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 8).as_ptr() as *const u64,
            count,
        )
    }

    pub fn get_i8(&mut self) -> i8 {
        self.backing.get_i8()
    }

    pub unsafe fn get_i8_bulk(&mut self, count: usize) -> &'a [i8] {
        std::mem::transmute(self.take_aligned_bytes(count))
    }

    pub fn get_i16(&mut self) -> i16 {
        self.backing.get_i16_le()
    }

    pub unsafe fn get_i16_bulk(&mut self, count: usize) -> &'a [i16] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 2).as_ptr() as *const i16,
            count,
        )
    }

    pub fn get_i32(&mut self) -> i32 {
        self.backing.get_i32_le()
    }

    pub unsafe fn get_i32_bulk(&mut self, count: usize) -> &'a [i32] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 4).as_ptr() as *const i32,
            count,
        )
    }

    pub fn get_i64(&mut self) -> i64 {
        self.backing.get_i64_le()
    }

    pub unsafe fn get_i64_bulk(&mut self, count: usize) -> &'a [i64] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 8).as_ptr() as *const i64,
            count,
        )
    }

    pub fn get_f32(&mut self) -> f32 {
        self.backing.get_f32_le()
    }

    pub unsafe fn get_f32_bulk(&mut self, count: usize) -> &'a [f32] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 4).as_ptr() as *const f32,
            count,
        )
    }

    pub fn get_f64(&mut self) -> f64 {
        self.backing.get_f64_le()
    }

    pub unsafe fn get_f64_bulk(&mut self, count: usize) -> &'a [f64] {
        std::slice::from_raw_parts(
            self.take_aligned_bytes(count * 8).as_ptr() as *const f64,
            count,
        )
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

    /// Reads an array header: the element type tag and the element count. For a
    /// `TYPE_STRUCTURE` or `TYPE_ENUMERATION` element tag the 16-byte element
    /// type id follows and must be read next (e.g. with [`get_uuid`](Self::get_uuid));
    /// primitive and string element tags carry no id. The elements themselves
    /// follow after that: numeric elements are a single [`align`](Self::align)
    /// then raw little-endian payloads; string, structure and enumeration
    /// elements are self-describing entries read one by one.
    pub fn get_array(&mut self) -> (u8, u32) {
        (self.backing.get_u8(), self.backing.get_u32_le())
    }
}
