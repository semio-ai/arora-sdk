use arora_buffers::{
  BufferWriter,
  TYPE_BOOLEAN,
  TYPE_U8, TYPE_U16, TYPE_U32, TYPE_U64,
  TYPE_S8, TYPE_S16, TYPE_S32, TYPE_S64,
  TYPE_R32, TYPE_R64,
  TYPE_STRING};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Value representation for received parameters.
//=====================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
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
  #[serde(rename = "s8")]
  S8(i8),
  #[serde(rename = "s16")]
  S16(i16),
  #[serde(rename = "s32")]
  S32(i32),
  #[serde(rename = "s64")]
  S64(i64),
  #[serde(rename = "f32")]
  R32(f32),
  #[serde(rename = "f64")]
  R64(f64),
  #[serde(rename = "str")]
  String(String),
  #[serde(rename = "struct")]
  Structure(Structure),
  #[serde(rename = "enum")]
  Enumeration(Enumeration),
  #[serde(rename = "bool[]")]
  ArrayBoolean(Vec<bool>),
  #[serde(rename = "u8[]")]
  ArrayU8(Vec<u8>),
  #[serde(rename = "u16[]")]
  ArrayU16(Vec<u16>),
  #[serde(rename = "u32[]")]
  ArrayU32(Vec<u32>),
  #[serde(rename = "u64[]")]
  ArrayU64(Vec<u64>),
  #[serde(rename = "s8[]")]
  ArrayS8(Vec<i8>),
  #[serde(rename = "s16[]")]
  ArrayS16(Vec<i16>),
  #[serde(rename = "s32[]")]
  ArrayS32(Vec<i32>),
  #[serde(rename = "s64[]")]
  ArrayS64(Vec<i64>),
  #[serde(rename = "f32[]")]
  ArrayR32(Vec<f32>),
  #[serde(rename = "f64[]")]
  ArrayR64(Vec<f64>),
  #[serde(rename = "str[]")]
  ArrayString(Vec<String>),
  #[serde(rename = "struct[]")]
  ArrayStructure {
    id: Uuid,
    elements: Vec<StructureWithoutId>,
  },
  #[serde(rename = "enum[]")]
  ArrayEnumeration {
    id: Uuid,
    elements: Vec<EnumerationWithoutId>,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Enumeration {
  pub id: Uuid,
  pub variant_id: Uuid,
  pub value: Box<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Structure {
  pub id: Uuid,
  pub fields: Vec<StructureField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructureField {
  pub id: Uuid,
  pub value: Box<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructureWithoutId {
  // #[serde(flatten)]
  pub fields: Vec<StructureField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnumerationWithoutId {
  pub variant_id: Uuid,
  pub value: Box<Value>,
}

/// A call is described like a structure in arora engine.
pub type Call = Structure;

pub fn serialize(v: &Value) -> Box<[u8]> {
  let mut writer = BufferWriter::new();
  serialize_to_writer(v, &mut writer);
  writer.finalize()
}

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

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::{Result, bail};
  use std::str::FromStr;

  #[test]
  pub fn parse_call_test() -> Result<()> {
    let call: Call = serde_yaml::from_str(CALL_TEST)?;
    assert_eq!(call.id, Uuid::from_str("07f5740c-ba4a-45af-8ec5-bedde5737e99")?);
    if let Value::Structure(Structure { id, fields }) = &call.fields[1].value.as_ref() {
      assert_eq!(*id, Uuid::from_str("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c")?);
      if let Value::S32(v) = fields[1].value.as_ref() {
        assert_eq!(*v, 113);
      } else {
        bail!("expected s32 value under second field of struct arg");
      }
    } else {
      bail!("expected a string under arg 55dbec70-1c3a-433e-a6e6-27446b7f065e");
    }
    Ok(())
  }

  #[test]
  pub fn parse_call_test_2() -> Result<()> {
    let call: Call = serde_yaml::from_str(CALL_TEST_2)?;
    assert_eq!(call.id, Uuid::from_str("b213a552-77ad-465a-a26d-352e8eccfd63")?);
    assert_eq!(call.fields.len(), 2);
    Ok(())
  }

  pub const CALL_TEST: &'static str = "\
id: 07f5740c-ba4a-45af-8ec5-bedde5737e99
fields:
- id: b41899c3-66dc-40d4-ab61-d1ccf5231c88
  value:
    enum:
      id: 325a5767-e344-4532-860e-0749bcf2e428
      variant_id: 766e9e9a-446d-4e46-83e6-14b7ca101169
      value: unit
- id: 63086e48-804f-403a-8862-3358ddedc08d
  value:
    struct:
      id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c
      fields:
      - id: 7d94a956-e50d-4cc4-9714-f62e1f9b134e
        value:
          enum[]:
            id: 325a5767-e344-4532-860e-0749bcf2e428
            elements:
              - variant_id: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
                value: unit
      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563
        value:
          s32: 113
";

  pub const CALL_TEST_2: &'static str = "\
id: b213a552-77ad-465a-a26d-352e8eccfd63
fields:
- id: 55dbec70-1c3a-433e-a6e6-27446b7f065e
  value:
    u32: 42
- id: abf9ca4e-e03f-431a-a32b-4911f809c399
  value:
    u32: 64
";
}
