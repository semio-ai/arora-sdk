use arora_schema::value::{Value, Structure, StructureField, StructureWithoutId, EnumerationWithoutId};
use uuid::Uuid;

use crate::{
  BufferReader, BufferWriter,
  TYPE_BOOLEAN,
  TYPE_U8, TYPE_U16, TYPE_U32, TYPE_U64,
  TYPE_S8, TYPE_S16, TYPE_S32, TYPE_S64,
  TYPE_R32, TYPE_R64,
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
    Value::S8(v) => writer.add_s8(*v),
    Value::S16(v) => writer.add_s16(*v),
    Value::S32(v) => writer.add_s32(*v),
    Value::S64(v) => writer.add_s64(*v),
    Value::R32(v) => writer.add_r32(*v),
    Value::R64(v) => writer.add_r64(*v),
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
    Value::ArrayS8(v) => {
      writer.add_array_primitive(TYPE_S8, v.len() as u32);
      writer.add_s8_raw_bulk(v);
    }
    Value::ArrayS16(v) => {
      writer.add_array_primitive(TYPE_S16, v.len() as u32);
      writer.add_s16_raw_bulk(v);
    }
    Value::ArrayS32(v) => {
      writer.add_array_primitive(TYPE_S32, v.len() as u32);
      writer.add_s32_raw_bulk(v);
    }
    Value::ArrayS64(v) => {
      writer.add_array_primitive(TYPE_S64, v.len() as u32);
      writer.add_s64_raw_bulk(v);
    }
    Value::ArrayR32(v) => {
      writer.add_array_primitive(TYPE_R32, v.len() as u32);
      writer.add_r32_raw_bulk(v);
    }
    Value::ArrayR64(v) => {
      writer.add_array_primitive(TYPE_R64, v.len() as u32);
      writer.add_r64_raw_bulk(v);
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
    Some(TYPE_S8) => Value::S8(reader.get_s8()),
    Some(TYPE_S16) => Value::S16(reader.get_s16()),
    Some(TYPE_S32) => Value::S32(reader.get_s32()),
    Some(TYPE_S64) => Value::S64(reader.get_s64()),
    Some(TYPE_R32) => Value::R32(reader.get_r32()),
    Some(TYPE_R64) => Value::R64(reader.get_r64()),
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
          TYPE_S8 => Value::ArrayS8(reader.get_s8_bulk(count as usize).into()),
          TYPE_S16 => Value::ArrayS16(reader.get_s16_bulk(count as usize).into()),
          TYPE_S32 => Value::ArrayS32(reader.get_s32_bulk(count as usize).into()),
          TYPE_S64 => Value::ArrayS64(reader.get_s64_bulk(count as usize).into()),
          TYPE_R32 => Value::ArrayR32(reader.get_r32_bulk(count as usize).into()),
          TYPE_R64 => Value::ArrayR64(reader.get_r64_bulk(count as usize).into()),
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
