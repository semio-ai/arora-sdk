
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::{
  BufferReader, BufferWriter,
  TYPE_BOOLEAN,
  TYPE_U8, TYPE_U16, TYPE_U32, TYPE_U64,
  TYPE_S8, TYPE_S16, TYPE_S32, TYPE_S64,
  TYPE_R32, TYPE_R64,
  TYPE_STRING, TYPE_STRUCTURE, TYPE_ARRAY, TYPE_ENUMERATION,
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
  S8(i8),
  #[serde(rename = "i16")]
  S16(i16),
  #[serde(rename = "i32")]
  S32(i32),
  #[serde(rename = "i64")]
  S64(i64),
  #[serde(rename = "f32")]
  R32(f32),
  #[serde(rename = "f64")]
  R64(f64),
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
  ArrayS8(Cow<'a, [i8]>),
  #[serde(rename = "i16[]")]
  ArrayS16(Cow<'a, [i16]>),
  #[serde(rename = "i32[]")]
  ArrayS32(Cow<'a, [i32]>),
  #[serde(rename = "i64[]")]
  ArrayS64(Cow<'a, [i64]>),
  #[serde(rename = "f32[]")]
  ArrayR32(Cow<'a, [f32]>),
  #[serde(rename = "f64[]")]
  ArrayR64(Cow<'a, [f64]>),
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
            id: field_id.into(),
            value: Value::deserialize_reader(reader),
          });
        }
        Value::Structure(Structure {
          id: id.into(),
          fields: fields,
        })
      }
      Some(TYPE_ARRAY) => {
        let (ty, count) = reader.get_array();
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
                  id: field_id.into(),
                  value: Value::deserialize_reader(reader),
                });
              }
              structures.push(StructureRaw { fields: fields });
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
      Value::S8(v) => writer.add_s8(*v),
      Value::S16(v) => writer.add_s16(*v),
      Value::S32(v) => writer.add_s32(*v),
      Value::S64(v) => writer.add_s64(*v),
      Value::R32(v) => writer.add_r32(*v),
      Value::R64(v) => writer.add_r64(*v),
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
  use anyhow::{Result, bail};

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
  pub fn array_f32_yaml() -> Result<()> {
    if let Value::ArrayR32(values) = serde_yaml::from_str(ARRAY_F32_YAML)? {
      assert_eq!(vec![3.14159, 2.718, 1.618], values.to_vec());
    } else {
      bail!("parsed value was not an array of f32");
    }
    Ok(())
  }

  pub const U8_YAML: &'static str = "\
u8: 42
";

  pub const ARRAY_F32_YAML: &'static str = "\
f32[]: [3.14159, 2.718, 1.618]
";
}
