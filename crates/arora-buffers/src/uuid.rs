use arora_schema::value::{Value, Structure, StructureField, StructureWithoutId, EnumerationWithoutId};
use uuid::Uuid;

use crate::{
  BufferReader, BufferWriter,
  TYPE_BOOLEAN,
  TYPE_U8, TYPE_U16, TYPE_U32, TYPE_U64,
  TYPE_I8, TYPE_I16, TYPE_I32, TYPE_I64,
  TYPE_F32, TYPE_F64,
  TYPE_STRING, TYPE_STRUCTURE, TYPE_ENUMERATION, TYPE_ARRAY
};

pub fn serialize_to_writer(v: &Value, writer: &mut BufferWriter) {
  match v {
    Value::Unit => writer.add_unit(),
    Value::Boolean(b) => writer.add_boolean(*b),
    Value::U8(v) => writer.add_u8(*v),
    Value::U16(v) => writer.add_u16(*v),
    Value::U32(v) => writer.add_u32(*v),
    Value::U64(v) => writer.add_u64(*v),
    Value::I8(v) => writer.add_i8(*v),
    Value::I16(v) => writer.add_i16(*v),
    Value::I32(v) => writer.add_i32(*v),
    Value::I64(v) => writer.add_i64(*v),
    Value::F32(v) => writer.add_f32(*v),
    Value::F64(v) => writer.add_f64(*v),
    Value::String(v) => writer.add_string(v),
    Value::Structure(v) => {
      writer.begin_structure(v.id.as_bytes(), v.fields.len() as u32);
      for field in &v.fields {
        writer.add_structure_field(field.id.as_bytes());
        serialize_to_writer(field.value.as_ref(), writer);
      }
    }
    Value::Enumeration(v) => {
      writer.add_enumeration_value(v.id.as_bytes(), v.variant_id.as_bytes());
      serialize_to_writer(v.value.as_ref(), writer);
    }
    Value::ArrayBoolean(v) => {
      writer.add_array_primitive(TYPE_BOOLEAN, v.len() as u32);
      writer.add_boolean_raw_bulk(v);
    }
    Value::ArrayU8(v) => {
      writer.add_array_primitive(TYPE_U8, v.len() as u32);
      writer.add_u8_raw_bulk(v);
    }
    Value::ArrayU16(v) => {
      writer.add_array_primitive(TYPE_U16, v.len() as u32);
      writer.add_u16_raw_bulk(v);
    }
    Value::ArrayU32(v) => {
      writer.add_array_primitive(TYPE_U32, v.len() as u32);
      writer.add_u32_raw_bulk(v);
    }
    Value::ArrayU64(v) => {
      writer.add_array_primitive(TYPE_U64, v.len() as u32);
      writer.add_u64_raw_bulk(v);
    }
    Value::ArrayI8(v) => {
      writer.add_array_primitive(TYPE_I8, v.len() as u32);
      writer.add_i8_raw_bulk(v);
    }
    Value::ArrayI16(v) => {
      writer.add_array_primitive(TYPE_I16, v.len() as u32);
      writer.add_i16_raw_bulk(v);
    }
    Value::ArrayI32(v) => {
      writer.add_array_primitive(TYPE_I32, v.len() as u32);
      writer.add_i32_raw_bulk(v);
    }
    Value::ArrayI64(v) => {
      writer.add_array_primitive(TYPE_I64, v.len() as u32);
      writer.add_i64_raw_bulk(v);
    }
    Value::ArrayF32(v) => {
      writer.add_array_primitive(TYPE_F32, v.len() as u32);
      writer.add_f32_raw_bulk(v);
    }
    Value::ArrayF64(v) => {
      writer.add_array_primitive(TYPE_F64, v.len() as u32);
      writer.add_f64_raw_bulk(v);
    }
    Value::ArrayString(v) => {
      writer.add_array_primitive(TYPE_STRING, v.len() as u32);
      for s in v {
        writer.add_string(s);
      }
    }
    Value::ArrayStructure { id, elements } => {
      writer.add_array_structure(id.as_bytes(), elements.len() as u32);
      for structure in elements {
        writer.begin_structure_raw(structure.fields.len() as u32);
        for field in &structure.fields {
          writer.add_structure_field(field.id.as_bytes());
          serialize_to_writer(field.value.as_ref(), writer);
        }
      }
    }
    Value::ArrayEnumeration { id, elements } => {
      writer.add_array_enumeration(id.as_bytes(), elements.len() as u32);
      for enumeration in elements {
        writer.add_enumeration_value_raw(enumeration.variant_id.as_bytes());
        serialize_to_writer(enumeration.value.as_ref(), writer);
      }
    }
  }
}

fn deserialize_from_reader(reader: &mut BufferReader) -> Value {
  match reader.next_type() {
    Some(TYPE_U8) => Value::U8(reader.get_u8()),
    Some(TYPE_U16) => Value::U16(reader.get_u16()),
    Some(TYPE_U32) => Value::U32(reader.get_u32()),
    Some(TYPE_U64) => Value::U64(reader.get_u64()),
    Some(TYPE_I8) => Value::I8(reader.get_i8()),
    Some(TYPE_I16) => Value::I16(reader.get_i16()),
    Some(TYPE_I32) => Value::I32(reader.get_i32()),
    Some(TYPE_I64) => Value::I64(reader.get_i64()),
    Some(TYPE_F32) => Value::F32(reader.get_f32()),
    Some(TYPE_F64) => Value::F64(reader.get_f64()),
    Some(TYPE_STRING) => Value::String(reader.get_string().into()),
    Some(TYPE_STRUCTURE) => {
      let (id, field_count) = reader.get_structure();
      let mut fields = Vec::with_capacity(field_count as usize);
      for _ in 0..field_count {
        let field_id = reader.get_structure_field();
        fields.push(StructureField {
          id: Uuid::from_slice(field_id).unwrap(),
          value: deserialize_from_reader(reader).into(),
        });
      }
      Value::Structure(Structure {
        id: Uuid::from_slice(id).unwrap(),
        fields: fields,
      })
    }
    Some(TYPE_ARRAY) => {
      let (ty, count) = reader.get_array();
      unsafe { // calling get_xx_bulk functions is unsafe, but result is copied.
        match ty {
          TYPE_BOOLEAN => Value::ArrayBoolean(reader.get_boolean_bulk(count as usize).into()),
          TYPE_U8 => Value::ArrayU8(reader.get_u8_bulk(count as usize).into()),
          TYPE_U16 => Value::ArrayU16(reader.get_u16_bulk(count as usize).into()),
          TYPE_U32 => Value::ArrayU32(reader.get_u32_bulk(count as usize).into()),
          TYPE_U64 => Value::ArrayU64(reader.get_u64_bulk(count as usize).into()),
          TYPE_I8 => Value::ArrayI8(reader.get_i8_bulk(count as usize).into()),
          TYPE_I16 => Value::ArrayI16(reader.get_i16_bulk(count as usize).into()),
          TYPE_I32 => Value::ArrayI32(reader.get_i32_bulk(count as usize).into()),
          TYPE_I64 => Value::ArrayI64(reader.get_i64_bulk(count as usize).into()),
          TYPE_F32 => Value::ArrayF32(reader.get_f32_bulk(count as usize).into()),
          TYPE_F64 => Value::ArrayF64(reader.get_f64_bulk(count as usize).into()),
          TYPE_STRING => Value::ArrayString({
            let mut strings = Vec::with_capacity(count as usize);
            for _ in 0..count {
              strings.push(reader.get_string().into());
            }
            strings
          }),
          TYPE_STRUCTURE => {
            let mut structures = Vec::with_capacity(count as usize);
            let structure_id = reader.get_structure_field();
            for _ in 0..count {
              let field_count = reader.get_structure_raw();
              let mut fields = Vec::with_capacity(field_count as usize);
              for _ in 0..field_count {
                let field_id = reader.get_structure_field();
                fields.push(StructureField {
                  id: Uuid::from_slice(field_id).unwrap(),
                  value: deserialize_from_reader(reader).into(),
                });
              }
              structures.push(StructureWithoutId { fields: fields });
            }
            Value::ArrayStructure {
              id: Uuid::from_slice(structure_id).unwrap(),
              elements: structures,
            }
          }
          TYPE_ENUMERATION => {
            let mut enumerations = Vec::with_capacity(count as usize);
            let enumeration_id = reader.get_structure_field();
            for _ in 0..count {
              let variant_id = reader.get_enumeration_value_raw();
              enumerations.push(EnumerationWithoutId {
                variant_id: Uuid::from_slice(variant_id).unwrap(),
                value: deserialize_from_reader(reader).into(),
              });
            }
            Value::ArrayEnumeration {
              id: Uuid::from_slice(enumeration_id).unwrap(),
              elements: enumerations
            }
          }
          _ => panic!("unsupported array type"),
        }
      }
    }
    _ => panic!("Invalid type"),
  }
}

pub fn serialize(value: &Value) -> Box<[u8]> {
  let mut writer = BufferWriter::new();
  serialize_to_writer(value, &mut writer);
  writer.finalize()
}

pub fn deserialize(data: &[u8]) -> Value {
  let mut reader = BufferReader::new(data);
  deserialize_from_reader(&mut reader)
}
