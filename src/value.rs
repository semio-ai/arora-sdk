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

#[cfg(test)]
mod tests {
  use super::*;
  use json5;
  use pretty_assertions::assert_eq;

  // Helper function for testing serialization/deserialization roundtrip
  fn test_serde_roundtrip<T>(value: &T, name: &str)
  where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug + Clone
  {
    let json = json5::to_string(value).unwrap();
    println!("{} JSON:\n{}", name, json);

    let deserialized: T = json5::from_str(&json).unwrap();
    assert_eq!(value, &deserialized, "Roundtrip serialization failed for {}", name);
  }

  #[test]
  fn test_type_serialization() {
    // Test all variants of Type enum
    for typ in [
      Type::Unit, Type::Boolean, Type::U8, Type::U16, Type::U32, Type::U64,
      Type::I8, Type::I16, Type::I32, Type::I64, Type::F32, Type::F64,
      Type::String, Type::Structure, Type::Enumeration,
      Type::ArrayBoolean, Type::ArrayU8, Type::ArrayU16, Type::ArrayU32, Type::ArrayU64,
      Type::ArrayI8, Type::ArrayI16, Type::ArrayI32, Type::ArrayI64,
      Type::ArrayF32, Type::ArrayF64, Type::ArrayString,
      Type::ArrayStructure, Type::ArrayEnumeration,
    ] {
      test_serde_roundtrip(&typ, &format!("Type::{:?}", typ));
    }
  }

  #[test]
  fn test_value_primitive_serialization() {
    // Test primitive value variants
    let primitives = vec![
      ("Unit", Value::Unit),
      ("Boolean_true", Value::Boolean(true)),
      ("Boolean_false", Value::Boolean(false)),
      ("U8_min", Value::U8(0)),
      ("U8_max", Value::U8(u8::MAX)),
      ("U16_max", Value::U16(u16::MAX)),
      ("U32_max", Value::U32(u32::MAX)),
      ("U64_max", Value::U64(u64::MAX)),
      ("I8_min", Value::I8(i8::MIN)),
      ("I8_max", Value::I8(i8::MAX)),
      ("I16_min", Value::I16(i16::MIN)),
      ("I32_min", Value::I32(i32::MIN)),
      ("I64_min", Value::I64(i64::MIN)),
      ("F32_zero", Value::F32(0.0)),
      ("F32_inf", Value::F32(f32::INFINITY)),
      ("F32_neg_inf", Value::F32(f32::NEG_INFINITY)),
      ("F64_zero", Value::F64(0.0)),
      ("F64_inf", Value::F64(f64::INFINITY)),
      ("F64_neg_inf", Value::F64(f64::NEG_INFINITY)),
      ("String_empty", Value::String("".to_string())),
      ("String_hello", Value::String("Hello, world!".to_string())),
      ("String_special", Value::String("Special chars: \n\t\r\"\\".to_string())),
    ];

    for (name, value) in primitives {
      test_serde_roundtrip(&value, name);
    }

    // Special handling for NaN values
    let f32_nan = Value::F32(f32::NAN);
    let f32_json = json5::to_string(&f32_nan).unwrap();
    println!("F32_NaN JSON:\n{}", f32_json);
    let deserialized_f32: Value = json5::from_str(&f32_json).unwrap();
    if let Value::F32(val) = deserialized_f32 {
      assert!(val.is_nan(), "Deserialized F32 should be NaN");
    }

    let f64_nan = Value::F64(f64::NAN);
    let f64_json = json5::to_string(&f64_nan).unwrap();
    println!("F64_NaN JSON:\n{}", f64_json);
    let deserialized_f64: Value = json5::from_str(&f64_json).unwrap();
    if let Value::F64(val) = deserialized_f64 {
      assert!(val.is_nan(), "Deserialized F64 should be NaN");
    }
  }

  #[test]
  fn test_value_array_serialization() {
    // Test array value variants
    let arrays = vec![
      ("ArrayBoolean_empty", Value::ArrayBoolean(vec![])),
      ("ArrayBoolean", Value::ArrayBoolean(vec![true, false])),
      ("ArrayU8", Value::ArrayU8(vec![0, 123, 255])),
      ("ArrayI32", Value::ArrayI32(vec![i32::MIN, 0, i32::MAX])),
      ("ArrayF64", Value::ArrayF64(vec![-1.0, 0.0, 1.0, f64::INFINITY])),
      ("ArrayString", Value::ArrayString(vec!["a".to_string(), "b".to_string()])),
    ];

    for (name, value) in arrays {
      test_serde_roundtrip(&value, name);
    }
  }

  #[test]
  fn test_structure_field_serialization() {
    let field = StructureField {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      value: Box::new(Value::String("test field".to_string())),
    };

    test_serde_roundtrip(&field, "StructureField");
  }

  #[test]
  fn test_structure_serialization() {
    // Test empty structure
    let empty_structure = Structure {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      fields: vec![],
    };

    test_serde_roundtrip(&empty_structure, "EmptyStructure");

    // Test populated structure
    let structure = Structure {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      fields: vec![
        StructureField {
          id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
          value: Box::new(Value::String("field1".to_string())),
        },
        StructureField {
          id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap(),
          value: Box::new(Value::I32(42)),
        },
      ],
    };

    test_serde_roundtrip(&structure, "Structure");
  }

  #[test]
  fn test_structure_without_id_serialization() {
    let structure_without_id = StructureWithoutId {
      fields: vec![
        StructureField {
          id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
          value: Box::new(Value::String("field1".to_string())),
        },
      ],
    };

    test_serde_roundtrip(&structure_without_id, "StructureWithoutId");
  }

  #[test]
  fn test_enumeration_serialization() {
    let enumeration = Enumeration {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      variant_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
      value: Box::new(Value::String("variant value".to_string())),
    };

    test_serde_roundtrip(&enumeration, "Enumeration");
  }

  #[test]
  fn test_enumeration_without_id_serialization() {
    let enumeration_without_id = EnumerationWithoutId {
      variant_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
      value: Box::new(Value::String("variant value".to_string())),
    };

    test_serde_roundtrip(&enumeration_without_id, "EnumerationWithoutId");
  }

  #[test]
  fn test_complex_nested_values() {
    // Complex nested structure with enumeration
    let complex_value = Value::Structure(
      Structure {
        id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
        fields: vec![
          StructureField {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            value: Box::new(Value::String("name".to_string())),
          },
          StructureField {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap(),
            value: Box::new(Value::Enumeration(
              Enumeration {
                id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap(),
                variant_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap(),
                value: Box::new(Value::Boolean(true)),
              }
            )),
          },
        ],
      }
    );

    test_serde_roundtrip(&complex_value, "ComplexNestedValue");
  }

  #[test]
  fn test_array_structure_and_enumeration() {
    // Test array of structures
    let array_structure = Value::ArrayStructure {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      elements: vec![
        StructureWithoutId {
          fields: vec![
            StructureField {
              id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
              value: Box::new(Value::String("element1".to_string())),
            },
          ],
        },
        StructureWithoutId { fields: vec![] }, // Empty structure
      ],
    };

    test_serde_roundtrip(&array_structure, "ArrayStructure");

    // Test array of enumerations
    let array_enumeration = Value::ArrayEnumeration {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      elements: vec![
        EnumerationWithoutId {
          variant_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
          value: Box::new(Value::String("variant1".to_string())),
        },
        EnumerationWithoutId {
          variant_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap(),
          value: Box::new(Value::U32(42)),
        },
      ],
    };

    test_serde_roundtrip(&array_enumeration, "ArrayEnumeration");
  }
}
