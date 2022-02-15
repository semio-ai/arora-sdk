use derive_more::Display;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Value representation for received parameters.
//=====================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
  #[serde(rename = "unit")]
  Unit,
  #[serde(rename = "bool")]
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
  #[serde(rename = "i8[]")]
  ArrayI8(Vec<i8>),
  #[serde(rename = "i16[]")]
  ArrayI16(Vec<i16>),
  #[serde(rename = "i32[]")]
  ArrayI32(Vec<i32>),
  #[serde(rename = "i64[]")]
  ArrayI64(Vec<i64>),
  #[serde(rename = "f32[]")]
  ArrayF32(Vec<f32>),
  #[serde(rename = "f64[]")]
  ArrayF64(Vec<f64>),
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

/// A common error type for conversion erros from and to [`Value`].
#[derive(Display, Debug)]
pub struct ConversionError {}

impl std::error::Error for ConversionError {}
