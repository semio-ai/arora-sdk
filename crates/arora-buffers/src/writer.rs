use bytes::BufMut;

use crate::{
    ALIGNMENT, TYPE_ARRAY, TYPE_BOOLEAN, TYPE_ENUMERATION, TYPE_ERROR, TYPE_F32, TYPE_F64,
    TYPE_I16, TYPE_I32, TYPE_I64, TYPE_I8, TYPE_MAP, TYPE_OPTION, TYPE_STRING, TYPE_STRUCTURE,
    TYPE_U16, TYPE_U32, TYPE_U64, TYPE_U8, TYPE_UNIT, TYPE_UUID,
};

pub struct BufferWriter {
    backing: Vec<u8>,
}

impl Default for BufferWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl BufferWriter {
    pub fn new() -> Self {
        let mut backing = Vec::with_capacity(128);
        // size placeholder
        backing.put_u32_le(0);
        Self { backing }
    }

    fn align(&mut self) {
        let alignment_buffer = ALIGNMENT - self.backing.len() % ALIGNMENT;
        for _ in 0..alignment_buffer {
            self.backing.put_u8(0);
        }
    }

    pub fn begin_structure_raw(&mut self, field_count: u32) {
        self.backing.put_u32_le(field_count);
    }

    pub fn begin_structure(&mut self, id: &[u8], field_count: u32) {
        assert_eq!(id.len(), 16);
        self.backing.put_u8(TYPE_STRUCTURE);
        self.backing.put_slice(id);
        self.begin_structure_raw(field_count);
    }

    pub fn add_enumeration_value_raw(&mut self, value_id: &[u8]) {
        assert_eq!(value_id.len(), 16);
        self.backing.put_slice(value_id);
    }

    pub fn add_enumeration_value(&mut self, id: &[u8], value_id: &[u8]) {
        assert_eq!(id.len(), 16);
        self.backing.put_u8(TYPE_ENUMERATION);
        self.backing.put_slice(id);
        self.add_enumeration_value_raw(value_id);
    }

    pub fn add_structure_field(&mut self, id: &[u8]) {
        assert_eq!(id.len(), 16);
        self.backing.put_slice(id);
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

    pub fn add_i8_raw(&mut self, value: i8) {
        self.backing.put_i8(value);
    }

    pub fn add_i8(&mut self, value: i8) {
        self.backing.put_u8(TYPE_I8);
        self.add_i8_raw(value);
    }

    pub fn add_i8_raw_bulk(&mut self, values: &[i8]) {
        self.align();
        for value in values {
            self.add_i8_raw(*value);
        }
    }

    pub fn add_i16_raw(&mut self, value: i16) {
        self.backing.put_i16_le(value);
    }

    pub fn add_i16(&mut self, value: i16) {
        self.backing.put_u8(TYPE_I16);
        self.add_i16_raw(value);
    }

    pub fn add_i16_raw_bulk(&mut self, values: &[i16]) {
        self.align();
        for value in values {
            self.add_i16_raw(*value);
        }
    }

    pub fn add_i32_raw(&mut self, value: i32) {
        self.backing.put_i32_le(value);
    }

    pub fn add_i32(&mut self, value: i32) {
        self.backing.put_u8(TYPE_I32);
        self.add_i32_raw(value);
    }

    pub fn add_i32_raw_bulk(&mut self, values: &[i32]) {
        self.align();
        for value in values {
            self.add_i32_raw(*value);
        }
    }

    pub fn add_i64_raw(&mut self, value: i64) {
        self.backing.put_i64_le(value);
    }

    pub fn add_i64(&mut self, value: i64) {
        self.backing.put_u8(TYPE_I64);
        self.add_i64_raw(value);
    }

    pub fn add_i64_raw_bulk(&mut self, values: &[i64]) {
        self.align();
        for value in values {
            self.add_i64_raw(*value);
        }
    }

    pub fn add_f32_raw(&mut self, value: f32) {
        self.backing.put_f32_le(value);
    }

    pub fn add_f32(&mut self, value: f32) {
        self.backing.put_u8(TYPE_F32);
        self.add_f32_raw(value);
    }

    pub fn add_f32_raw_bulk(&mut self, values: &[f32]) {
        self.align();
        for value in values {
            self.add_f32_raw(*value);
        }
    }

    pub fn add_f64_raw(&mut self, value: f64) {
        self.backing.put_f64_le(value);
    }

    pub fn add_f64(&mut self, value: f64) {
        self.backing.put_u8(TYPE_F64);
        self.add_f64_raw(value);
    }

    pub fn add_f64_raw_bulk(&mut self, values: &[f64]) {
        self.align();
        for value in values {
            self.add_f64_raw(*value);
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
        self.backing.put_u32_le(element_count);
        self.backing.put_slice(ty_id);
    }

    pub fn add_array_enumeration(&mut self, ty_id: &[u8], element_count: u32) {
        assert_eq!(ty_id.len(), 16);
        self.backing.put_u8(TYPE_ARRAY);
        self.backing.put_u8(TYPE_ENUMERATION);
        self.backing.put_u32_le(element_count);
        self.backing.put_slice(ty_id);
    }

    pub fn add_option_some(&mut self) {
        self.backing.put_u8(TYPE_OPTION);
        self.backing.put_u8(1);
    }

    pub fn add_option_none(&mut self) {
        self.backing.put_u8(TYPE_OPTION);
        self.backing.put_u8(0);
    }

    pub fn add_uuid_raw(&mut self, id: &[u8]) {
        assert_eq!(id.len(), 16);
        self.backing.put_slice(id);
    }

    pub fn add_uuid(&mut self, id: &[u8]) {
        self.backing.put_u8(TYPE_UUID);
        self.add_uuid_raw(id);
    }

    pub fn begin_map(&mut self, id: &[u8], field_count: u32) {
        assert_eq!(id.len(), 16);
        self.backing.put_u8(TYPE_MAP);
        self.backing.put_slice(id);
        self.backing.put_u32_le(field_count);
    }

    pub fn add_map_field_key(&mut self, key: &str) {
        self.backing.put_u32_le(key.len() as u32);
        self.backing.put_slice(key.as_bytes());
    }

    pub fn add_error(&mut self, message: &str) {
        self.backing.put_u8(TYPE_ERROR);
        self.add_string_raw(message);
    }

    pub fn finalize(&mut self) -> Box<[u8]> {
        let size = self.backing.len() as u32;
        self.backing[0..4].copy_from_slice(&size.to_le_bytes());
        std::mem::take(&mut self.backing).into_boxed_slice()
    }
}
