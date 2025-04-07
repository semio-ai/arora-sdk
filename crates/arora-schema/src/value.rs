use derive_more::Display;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
pub enum Type {
  #[serde(rename = "unit")]
  Unit,
  #[serde(rename = "bool")]
  Boolean,
  #[serde(rename = "u8")]
  U8,
  #[serde(rename = "u16")]
  U16,
  #[serde(rename = "u32")]
  U32,
  #[serde(rename = "u64")]
  U64,
  #[serde(rename = "i8")]
  I8,
  #[serde(rename = "i16")]
  I16,
  #[serde(rename = "i32")]
  I32,
  #[serde(rename = "i64")]
  I64,
  #[serde(rename = "f32")]
  F32,
  #[serde(rename = "f64")]
  F64,
  #[serde(rename = "str")]
  String,
  #[serde(rename = "struct")]
  Structure,
  #[serde(rename = "enum")]
  Enumeration,
  #[serde(rename = "bool[]")]
  ArrayBoolean,
  #[serde(rename = "u8[]")]
  ArrayU8,
  #[serde(rename = "u16[]")]
  ArrayU16,
  #[serde(rename = "u32[]")]
  ArrayU32,
  #[serde(rename = "u64[]")]
  ArrayU64,
  #[serde(rename = "i8[]")]
  ArrayI8,
  #[serde(rename = "i16[]")]
  ArrayI16,
  #[serde(rename = "i32[]")]
  ArrayI32,
  #[serde(rename = "i64[]")]
  ArrayI64,
  #[serde(rename = "f32[]")]
  ArrayF32,
  #[serde(rename = "f64[]")]
  ArrayF64,
  #[serde(rename = "str[]")]
  ArrayString,
  #[serde(rename = "struct[]")]
  ArrayStructure,
  #[serde(rename = "enum[]")]
  ArrayEnumeration
}

// Value representation for received parameters.
//=====================================================================
#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
pub enum Value {
  #[serde(rename = "unit")]
  #[display("()")]
  Unit,
  #[serde(rename = "bool")]
  Boolean(bool),
  #[serde(rename = "u8")]
  #[display("{}u8", _0)]
  U8(u8),
  #[serde(rename = "u16")]
  #[display("{}u16", _0)]
  U16(u16),
  #[serde(rename = "u32")]
  #[display("{}u32", _0)]
  U32(u32),
  #[serde(rename = "u64")]
  #[display("{}u64", _0)]
  U64(u64),
  #[serde(rename = "i8")]
  #[display("{}i8", _0)]
  I8(i8),
  #[serde(rename = "i16")]
  #[display("{}i16", _0)]
  I16(i16),
  #[serde(rename = "i32")]
  #[display("{}i32", _0)]
  I32(i32),
  #[serde(rename = "i64")]
  #[display("{}i64", _0)]
  I64(i64),
  #[serde(rename = "f32")]
  #[display("{}f32", _0)]
  F32(f32),
  #[serde(rename = "f64")]
  #[display("{}f64", _0)]
  F64(f64),
  #[serde(rename = "str")]
  #[display("\"{}\"", _0)]
  String(String),
  #[serde(rename = "struct")]
  Structure(Structure),
  #[serde(rename = "enum")]
  Enumeration(Enumeration),
  #[serde(rename = "bool[]")]
  #[display("[{:?}]", _0)]
  ArrayBoolean(Vec<bool>),
  #[serde(rename = "u8[]")]
  #[display("u8[{:?}]", _0)]
  ArrayU8(Vec<u8>),
  #[serde(rename = "u16[]")]
  #[display("u16[{:?}]", _0)]
  ArrayU16(Vec<u16>),
  #[serde(rename = "u32[]")]
  #[display("u32[{:?}]", _0)]
  ArrayU32(Vec<u32>),
  #[serde(rename = "u64[]")]
  #[display("u64[{:?}]", _0)]
  ArrayU64(Vec<u64>),
  #[serde(rename = "i8[]")]
  #[display("i8[{:?}]", _0)]
  ArrayI8(Vec<i8>),
  #[serde(rename = "i16[]")]
  #[display("i16[{:?}]", _0)]
  ArrayI16(Vec<i16>),
  #[serde(rename = "i32[]")]
  #[display("i32[{:?}]", _0)]
  ArrayI32(Vec<i32>),
  #[serde(rename = "i64[]")]
  #[display("i64[{:?}]", _0)]
  ArrayI64(Vec<i64>),
  #[serde(rename = "f32[]")]
  #[display("f32[{:?}]", _0)]
  ArrayF32(Vec<f32>),
  #[serde(rename = "f64[]")]
  #[display("f64[{:?}]", _0)]
  ArrayF64(Vec<f64>),
  #[serde(rename = "str[]")]
  #[display("[{:?}]", _0)]
  ArrayString(Vec<String>),
  #[serde(rename = "struct[]")]
  #[display("struct[]({}, {:?})", id, elements)]
  ArrayStructure {
    id: Uuid,
    elements: Vec<StructureWithoutId>,
  },
  #[serde(rename = "enum[]")]
  #[display("enum[]({}, {:?})", id, elements)]
  ArrayEnumeration {
    id: Uuid,
    elements: Vec<EnumerationWithoutId>,
  },
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("{}::{}({})", id, variant_id, value)]
pub struct Enumeration {
  pub id: Uuid,
  pub variant_id: Uuid,
  pub value: Box<Value>,
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("{}({:?})", id, fields)]
pub struct Structure {
  pub id: Uuid,
  pub fields: Vec<StructureField>,
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("{}: {}", id, value)]
pub struct StructureField {
  pub id: Uuid,
  pub value: Box<Value>,
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("({:?})", fields)]
pub struct StructureWithoutId {
  // #[serde(flatten)]
  pub fields: Vec<StructureField>,
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("{}({})", variant_id, value)]
pub struct EnumerationWithoutId {
  pub variant_id: Uuid,
  pub value: Box<Value>,
}

/// A common error type for conversion erros from and to [`Value`].
#[derive(Display, Debug)]
pub struct ConversionError {
  pub message: String,
}

impl std::error::Error for ConversionError {}
