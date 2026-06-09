use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::{
  read::BufferReader, write::BufferWriter, TYPE_ARRAY, TYPE_BOOLEAN, TYPE_ENUMERATION, TYPE_F32,
  TYPE_F64, TYPE_I16, TYPE_I32, TYPE_I64, TYPE_I8, TYPE_STRING, TYPE_STRUCTURE, TYPE_U16, TYPE_U32,
  TYPE_U64, TYPE_U8,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureField<'a> {
  pub id: Cow<'a, [u8]>,
  pub value: Value<'a>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Structure<'a> {
  pub id: Cow<'a, [u8]>,
  pub fields: Vec<StructureField<'a>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureRaw<'a> {
  pub fields: Vec<StructureField<'a>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Enumeration<'a> {
  pub id: Cow<'a, [u8]>,
  pub variant_id: Cow<'a, [u8]>,
  pub value: Box<Value<'a>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumerationRaw<'a> {
  pub variant_id: Cow<'a, [u8]>,
  pub value: Box<Value<'a>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value<'a> {
  #[serde(rename = "unit")]
  Unit,
  #[serde(rename = "book")]
  Boolean(bool),
  #[serde(rename = "u8")]
  U8(u8),
  #[serde(rename = "u16")]
  U16(u16),
  #[serde(rename = "u32")]
  U32(u32),
  #[serde(rename = "u64")]
  U64(u64),
  #[serde(rename = "i8")]
  I8(i8),
  #[serde(rename = "i16")]
  I16(i16),
  #[serde(rename = "i32")]
  I32(i32),
  #[serde(rename = "i64")]
  I64(i64),
  #[serde(rename = "f32")]
  F32(f32),
  #[serde(rename = "f64")]
  F64(f64),
  #[serde(rename = "str")]
  String(Cow<'a, str>),
  #[serde(rename = "struct")]
  Structure(Structure<'a>),
  #[serde(rename = "enum")]
  Enumeration(Enumeration<'a>),
  #[serde(rename = "bool[]")]
  ArrayBoolean(Cow<'a, [bool]>),
  #[serde(rename = "u8[]")]
  ArrayU8(Cow<'a, [u8]>),
  #[serde(rename = "u16[]")]
  ArrayU16(Cow<'a, [u16]>),
  #[serde(rename = "u32[]")]
  ArrayU32(Cow<'a, [u32]>),
  #[serde(rename = "u64[]")]
  ArrayU64(Cow<'a, [u64]>),
  #[serde(rename = "i8[]")]
  ArrayI8(Cow<'a, [i8]>),
  #[serde(rename = "i16[]")]
  ArrayI16(Cow<'a, [i16]>),
  #[serde(rename = "i32[]")]
  ArrayI32(Cow<'a, [i32]>),
  #[serde(rename = "i64[]")]
  ArrayI64(Cow<'a, [i64]>),
  #[serde(rename = "f32[]")]
  ArrayF32(Cow<'a, [f32]>),
  #[serde(rename = "f64[]")]
  ArrayF64(Cow<'a, [f64]>),
  #[serde(rename = "str[]")]
  ArrayString(Vec<Cow<'a, str>>),
  #[serde(rename = "struct[]")]
  ArrayStructure(Cow<'a, [u8]>, Vec<StructureRaw<'a>>),
  #[serde(rename = "enum[]")]
  ArrayEnumeration(Cow<'a, [u8]>, Vec<EnumerationRaw<'a>>),
}

impl<'a> Value<'a> {
  unsafe fn deserialize_reader(reader: &mut BufferReader<'a>) -> Value<'a> {
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
            id: field_id.into(),
            value: Value::deserialize_reader(reader),
          });
        }
        Value::Structure(Structure {
          id: id.into(),
          fields,
        })
      }
      Some(TYPE_ENUMERATION) => Value::Enumeration(Enumeration {
        id: reader.get_structure_field().into(),
        variant_id: reader.get_enumeration_value_raw().into(),
        value: Box::new(Value::deserialize_reader(reader)),
      }),
      Some(TYPE_ARRAY) => {
        let (ty, count) = reader.get_array();
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
                  id: field_id.into(),
                  value: Value::deserialize_reader(reader),
                });
              }
              structures.push(StructureRaw { fields });
            }
            Value::ArrayStructure(structure_id.into(), structures)
          }
          TYPE_ENUMERATION => {
            let mut enumerations = Vec::with_capacity(count as usize);
            let enumeration_id = reader.get_structure_field();
            for _ in 0..count {
              let variant_id = reader.get_enumeration_value_raw();
              enumerations.push(EnumerationRaw {
                variant_id: variant_id.into(),
                value: Box::new(Value::deserialize_reader(reader)),
              });
            }
            Value::ArrayEnumeration(enumeration_id.into(), enumerations)
          }
          _ => panic!("unsupported array type"),
        }
      }
      _ => panic!("Invalid type"),
    }
  }

  pub unsafe fn deserialize(data: &'a [u8]) -> Value<'a> {
    let mut reader = BufferReader::new(data);
    Self::deserialize_reader(&mut reader)
  }

  fn serialize_writer(&self, writer: &mut BufferWriter) {
    match self {
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
        writer.begin_structure(&v.id, v.fields.len() as u32);
        for field in &v.fields {
          writer.add_structure_field(&field.id);
          field.value.serialize_writer(writer);
        }
      }
      Value::Enumeration(v) => {
        writer.add_enumeration_value(&v.id, &v.variant_id);
        v.value.serialize_writer(writer);
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
      Value::ArrayStructure(id, v) => {
        writer.add_array_structure(id, v.len() as u32);
        for structure in v {
          writer.begin_structure_raw(structure.fields.len() as u32);
          for field in &structure.fields {
            writer.add_structure_field(&field.id);
            field.value.serialize_writer(writer);
          }
        }
      }
      Value::ArrayEnumeration(id, v) => {
        writer.add_array_enumeration(id, v.len() as u32);
        for enumeration in v {
          writer.add_enumeration_value_raw(&enumeration.variant_id);
          enumeration.value.serialize_writer(writer);
        }
      }
    }
  }

  pub fn serialize(&self) -> Box<[u8]> {
    let mut writer = BufferWriter::new();
    self.serialize_writer(&mut writer);
    writer.finalize()
  }
}

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::{bail, Result};

  #[test]
  pub fn u8_yaml() -> Result<()> {
    if let Value::U8(value) = serde_yaml::from_str(U8_YAML)? {
      assert_eq!(42, value);
    } else {
      bail!("parsed value was not an u8");
    }
    Ok(())
  }

  #[test]
  // The literals are arbitrary sample values for round-tripping an f32 array,
  // not intended to be std::f32::consts::PI / E.
  #[allow(clippy::approx_constant)]
  pub fn array_f32_yaml() -> Result<()> {
    if let Value::ArrayF32(values) = serde_yaml::from_str(ARRAY_F32_YAML)? {
      assert_eq!(vec![3.14159, 2.718, 1.618], values.to_vec());
    } else {
      bail!("parsed value was not an array of f32");
    }
    Ok(())
  }

  pub const U8_YAML: &str = "\
u8: 42
";

  pub const ARRAY_F32_YAML: &str = "\
f32[]: [3.14159, 2.718, 1.618]
";
}
