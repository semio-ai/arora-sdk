/// This module provides the [`Value`] enum and related types for representing structured values,
/// including conversions from primitive types, arrays, and collections.
///
/// Note: HashSet<f32> and HashSet<f64> are not implemented because floating point types don't implement Hash due to NaN issues.
use derive_more::Display;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::keyvalue::KeyValue;

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
  #[serde(rename = "option")]
  Option,
  #[serde(rename = "struct")]
  Structure,
  #[serde(rename = "enum")]
  Enumeration,
  #[serde(rename = "bools")]
  ArrayBoolean,
  #[serde(rename = "u8s")]
  ArrayU8,
  #[serde(rename = "u16s")]
  ArrayU16,
  #[serde(rename = "u32s")]
  ArrayU32,
  #[serde(rename = "u64s")]
  ArrayU64,
  #[serde(rename = "i8s")]
  ArrayI8,
  #[serde(rename = "i16s")]
  ArrayI16,
  #[serde(rename = "i32s")]
  ArrayI32,
  #[serde(rename = "i64s")]
  ArrayI64,
  #[serde(rename = "f32s")]
  ArrayF32,
  #[serde(rename = "f64s")]
  ArrayF64,
  #[serde(rename = "strs")]
  ArrayString,
  #[serde(rename = "values")]
  ArrayValue,
  #[serde(rename = "structs")]
  ArrayStructure,
  #[serde(rename = "enums")]
  ArrayEnumeration,
  #[serde(rename = "keyvalues")]
  KeyValue,
  #[serde(rename = "uuids")]
  Uuid,
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
  #[serde(rename = "option")]
  #[display("[{}]", if let Some(v) = _0.as_ref() { format!("{}", v) } else { "null".to_string() })]
  Option(Option<Box<Value>>),
  #[serde(rename = "struct")]
  Structure(Structure),
  #[serde(rename = "enum")]
  Enumeration(Enumeration),
  #[serde(rename = "bools")]
  #[display("[{:?}]", _0)]
  ArrayBoolean(Vec<bool>),
  #[serde(rename = "u8s")]
  #[display("u8[{:?}]", _0)]
  ArrayU8(Vec<u8>),
  #[serde(rename = "u16s")]
  #[display("u16[{:?}]", _0)]
  ArrayU16(Vec<u16>),
  #[serde(rename = "u32s")]
  #[display("u32[{:?}]", _0)]
  ArrayU32(Vec<u32>),
  #[serde(rename = "u64s")]
  #[display("u64[{:?}]", _0)]
  ArrayU64(Vec<u64>),
  #[serde(rename = "i8s")]
  #[display("i8[{:?}]", _0)]
  ArrayI8(Vec<i8>),
  #[serde(rename = "i16s")]
  #[display("i16[{:?}]", _0)]
  ArrayI16(Vec<i16>),
  #[serde(rename = "i32s")]
  #[display("i32[{:?}]", _0)]
  ArrayI32(Vec<i32>),
  #[serde(rename = "i64s")]
  #[display("i64[{:?}]", _0)]
  ArrayI64(Vec<i64>),
  #[serde(rename = "f32s")]
  #[display("f32[{:?}]", _0)]
  ArrayF32(Vec<f32>),
  #[serde(rename = "f64s")]
  #[display("f64[{:?}]", _0)]
  ArrayF64(Vec<f64>),
  #[serde(rename = "strs")]
  #[display("[{:?}]", _0)]
  ArrayString(Vec<String>),
  #[serde(rename = "values")]
  #[display("[{:?}]", _0)]
  ArrayValue(Vec<Value>),
  #[serde(rename = "structs")]
  #[display("structs({}, {:?})", id, elements)]
  ArrayStructure {
    id: Uuid,
    elements: Vec<StructureWithoutId>,
  },
  #[serde(rename = "enums")]
  #[display("enums({}, {:?})", id, elements)]
  ArrayEnumeration {
    id: Uuid,
    elements: Vec<EnumerationWithoutId>,
  },
  #[serde(rename = "keyvalue")]
  KeyValue(KeyValue),
  #[serde(rename = "uuid")]
  #[display("uuid({})", _0)]
  Uuid(Uuid),
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

impl From<()> for Value {
  fn from(_: ()) -> Self {
    Value::Unit
  }
}

impl From<bool> for Value {
  fn from(v: bool) -> Self {
    Value::Boolean(v)
  }
}

impl From<u8> for Value {
  fn from(v: u8) -> Self {
    Value::U8(v)
  }
}

impl From<u16> for Value {
  fn from(v: u16) -> Self {
    Value::U16(v)
  }
}

impl From<u32> for Value {
  fn from(v: u32) -> Self {
    Value::U32(v)
  }
}

impl From<u64> for Value {
  fn from(v: u64) -> Self {
    Value::U64(v)
  }
}

impl From<i8> for Value {
  fn from(v: i8) -> Self {
    Value::I8(v)
  }
}

impl From<i16> for Value {
  fn from(v: i16) -> Self {
    Value::I16(v)
  }
}

impl From<i32> for Value {
  fn from(v: i32) -> Self {
    Value::I32(v)
  }
}

impl From<i64> for Value {
  fn from(v: i64) -> Self {
    Value::I64(v)
  }
}

impl From<f32> for Value {
  fn from(v: f32) -> Self {
    Value::F32(v)
  }
}

impl From<f64> for Value {
  fn from(v: f64) -> Self {
    Value::F64(v)
  }
}

impl From<String> for Value {
  fn from(v: String) -> Self {
    Value::String(v)
  }
}

impl From<&str> for Value {
  fn from(v: &str) -> Self {
    Value::String(v.to_string())
  }
}

impl From<Uuid> for Value {
  fn from(v: Uuid) -> Self {
    Value::Uuid(v)
  }
}

// Option<T> -> Value::Option conversion
impl<T> From<Option<T>> for Value
where
  T: Into<Value>,
{
  fn from(opt: Option<T>) -> Self {
    match opt {
      Some(value) => Value::Option(Some(Box::new(value.into()))),
      None => Value::Option(None),
    }
  }
}

// Vec<Value> -> Value::ArrayValue conversion
impl From<Vec<Value>> for Value {
  fn from(vec: Vec<Value>) -> Self {
    Value::ArrayValue(vec)
  }
}

// &[Value] -> Value::ArrayValue conversion
impl From<&[Value]> for Value {
  fn from(slice: &[Value]) -> Self {
    Value::ArrayValue(slice.to_vec())
  }
}

// Macro to reduce repetition for Vec, slice, and HashSet conversions
macro_rules! impl_array_conversions {
    ($(($rust_type:ty, $variant:ident)),* $(,)?) => {
        $(
            // Vec<T> -> Value::Array*
            impl From<Vec<$rust_type>> for Value {
                fn from(vec: Vec<$rust_type>) -> Self {
                    Value::$variant(vec)
                }
            }

            // &[T] -> Value::Array*
            impl From<&[$rust_type]> for Value {
                fn from(slice: &[$rust_type]) -> Self {
                    Value::$variant(slice.to_vec())
                }
            }

            // HashSet<T> -> Value::Array* (for hashable types only)
            impl From<std::collections::HashSet<$rust_type>> for Value {
                fn from(set: std::collections::HashSet<$rust_type>) -> Self {
                    Value::$variant(set.into_iter().collect())
                }
            }
        )*
    };
}

// Apply the macro for all supported array types
impl_array_conversions! {
    (bool, ArrayBoolean),
    (u8, ArrayU8),
    (u16, ArrayU16),
    (u32, ArrayU32),
    (u64, ArrayU64),
    (i8, ArrayI8),
    (i16, ArrayI16),
    (i32, ArrayI32),
    (i64, ArrayI64),
    (String, ArrayString),
}

// Separate implementations for floating point types (no HashSet support)
macro_rules! impl_float_array_conversions {
    ($(($rust_type:ty, $variant:ident)),* $(,)?) => {
        $(
            // Vec<T> -> Value::Array*
            impl From<Vec<$rust_type>> for Value {
                fn from(vec: Vec<$rust_type>) -> Self {
                    Value::$variant(vec)
                }
            }

            // &[T] -> Value::Array*
            impl From<&[$rust_type]> for Value {
                fn from(slice: &[$rust_type]) -> Self {
                    Value::$variant(slice.to_vec())
                }
            }
        )*
    };
}

// Apply the macro for floating point types (no HashSet due to Hash requirements)
impl_float_array_conversions! {
    (f32, ArrayF32),
    (f64, ArrayF64),
}

// All slice and HashSet conversions are now generated by the macros above

#[cfg(test)]
mod tests {
  use super::*;
  use json5;
  use pretty_assertions::assert_eq;

  // Helper function for testing serialization/deserialization roundtrip
  fn test_serde_roundtrip<T>(value: &T, name: &str)
  where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug + Clone,
  {
    // First with JSON
    let json = json5::to_string(value).unwrap();
    println!("{} JSON:\n{}", name, json);

    let deserialized: T = json5::from_str(&json).unwrap();
    assert_eq!(
      value, &deserialized,
      "Roundtrip JSON serialization failed for {name}"
    );

    // Then with RON
    let ron = ron::to_string(value).unwrap();
    println!("{} RON:\n{}", name, ron);
    let deserialized: T = ron::from_str(&ron).unwrap();
    assert_eq!(
      value, &deserialized,
      "Roundtrip RON serialization failed for {name}"
    );
  }

  #[test]
  fn test_type_serialization() {
    // Test all variants of Type enum
    for typ in [
      Type::Unit,
      Type::Boolean,
      Type::U8,
      Type::U16,
      Type::U32,
      Type::U64,
      Type::I8,
      Type::I16,
      Type::I32,
      Type::I64,
      Type::F32,
      Type::F64,
      Type::String,
      Type::Option,
      Type::Structure,
      Type::Enumeration,
      Type::ArrayBoolean,
      Type::ArrayU8,
      Type::ArrayU16,
      Type::ArrayU32,
      Type::ArrayU64,
      Type::ArrayI8,
      Type::ArrayI16,
      Type::ArrayI32,
      Type::ArrayI64,
      Type::ArrayF32,
      Type::ArrayF64,
      Type::ArrayString,
      Type::ArrayValue,
      Type::ArrayStructure,
      Type::ArrayEnumeration,
      Type::KeyValue,
      Type::Uuid,
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
      (
        "String_special",
        Value::String("Special chars: \n\t\r\"\\".to_string()),
      ),
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
      (
        "ArrayF64",
        Value::ArrayF64(vec![-1.0, 0.0, 1.0, f64::INFINITY]),
      ),
      (
        "ArrayString",
        Value::ArrayString(vec!["a".to_string(), "b".to_string()]),
      ),
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
      fields: vec![StructureField {
        id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
        value: Box::new(Value::String("field1".to_string())),
      }],
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
    let complex_value = Value::Structure(Structure {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      fields: vec![
        StructureField {
          id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
          value: Box::new(Value::String("name".to_string())),
        },
        StructureField {
          id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap(),
          value: Box::new(Value::Enumeration(Enumeration {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap(),
            variant_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap(),
            value: Box::new(Value::Boolean(true)),
          })),
        },
      ],
    });

    test_serde_roundtrip(&complex_value, "ComplexNestedValue");
  }

  #[test]
  fn test_array_structure_and_enumeration() {
    // Test array of structures
    let array_structure = Value::ArrayStructure {
      id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
      elements: vec![
        StructureWithoutId {
          fields: vec![StructureField {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            value: Box::new(Value::String("element1".to_string())),
          }],
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

  #[test]
  fn test_from_conversions_primitives() {
    // Test From conversions for primitive types
    assert_eq!(Value::from(()), Value::Unit);
    assert_eq!(Value::from(true), Value::Boolean(true));
    assert_eq!(Value::from(false), Value::Boolean(false));

    assert_eq!(Value::from(42u8), Value::U8(42));
    assert_eq!(Value::from(1234u16), Value::U16(1234));
    assert_eq!(Value::from(123456u32), Value::U32(123456));
    assert_eq!(Value::from(12345678901234u64), Value::U64(12345678901234));

    assert_eq!(Value::from(-42i8), Value::I8(-42));
    assert_eq!(Value::from(-1234i16), Value::I16(-1234));
    assert_eq!(Value::from(-123456i32), Value::I32(-123456));
    assert_eq!(Value::from(-12345678901234i64), Value::I64(-12345678901234));

    assert_eq!(Value::from(3.14f32), Value::F32(3.14f32));
    assert_eq!(
      Value::from(3.141592653589793f64),
      Value::F64(3.141592653589793f64)
    );

    assert_eq!(
      Value::from("hello".to_string()),
      Value::String("hello".to_string())
    );
    assert_eq!(Value::from("world"), Value::String("world".to_string()));

    let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    assert_eq!(Value::from(uuid), Value::Uuid(uuid));
  }

  #[test]
  fn test_from_conversions_arrays_vec() {
    // Test Vec conversions
    assert_eq!(
      Value::from(vec![true, false, true]),
      Value::ArrayBoolean(vec![true, false, true])
    );
    assert_eq!(
      Value::from(vec![1u8, 2u8, 3u8]),
      Value::ArrayU8(vec![1, 2, 3])
    );
    assert_eq!(
      Value::from(vec![100u16, 200u16]),
      Value::ArrayU16(vec![100, 200])
    );
    assert_eq!(
      Value::from(vec![1000u32, 2000u32]),
      Value::ArrayU32(vec![1000, 2000])
    );
    assert_eq!(
      Value::from(vec![10000u64, 20000u64]),
      Value::ArrayU64(vec![10000, 20000])
    );

    assert_eq!(Value::from(vec![-1i8, -2i8]), Value::ArrayI8(vec![-1, -2]));
    assert_eq!(
      Value::from(vec![-100i16, -200i16]),
      Value::ArrayI16(vec![-100, -200])
    );
    assert_eq!(
      Value::from(vec![-1000i32, -2000i32]),
      Value::ArrayI32(vec![-1000, -2000])
    );
    assert_eq!(
      Value::from(vec![-10000i64, -20000i64]),
      Value::ArrayI64(vec![-10000, -20000])
    );

    assert_eq!(
      Value::from(vec![1.5f32, 2.5f32]),
      Value::ArrayF32(vec![1.5, 2.5])
    );
    assert_eq!(
      Value::from(vec![1.5f64, 2.5f64]),
      Value::ArrayF64(vec![1.5, 2.5])
    );

    assert_eq!(
      Value::from(vec!["hello".to_string(), "world".to_string()]),
      Value::ArrayString(vec!["hello".to_string(), "world".to_string()])
    );
  }

  #[test]
  fn test_from_conversions_arrays_slices() {
    // Test slice conversions
    let bool_slice = &[true, false, true][..];
    assert_eq!(
      Value::from(bool_slice),
      Value::ArrayBoolean(vec![true, false, true])
    );

    let u32_slice = &[1u32, 2u32, 3u32][..];
    assert_eq!(Value::from(u32_slice), Value::ArrayU32(vec![1, 2, 3]));

    let string_slice = &["a".to_string(), "b".to_string()][..];
    assert_eq!(
      Value::from(string_slice),
      Value::ArrayString(vec!["a".to_string(), "b".to_string()])
    );
  }

  #[test]
  fn test_from_conversions_hashset() {
    use std::collections::HashSet;

    // Test HashSet conversions (note: order is not guaranteed, so we check contents)
    let bool_set: HashSet<bool> = [true, false].into_iter().collect();
    if let Value::ArrayBoolean(vec) = Value::from(bool_set) {
      assert_eq!(vec.len(), 2);
      assert!(vec.contains(&true));
      assert!(vec.contains(&false));
    } else {
      panic!("Expected ArrayBoolean");
    }

    let u32_set: HashSet<u32> = [1, 2, 3].into_iter().collect();
    if let Value::ArrayU32(vec) = Value::from(u32_set) {
      assert_eq!(vec.len(), 3);
      assert!(vec.contains(&1));
      assert!(vec.contains(&2));
      assert!(vec.contains(&3));
    } else {
      panic!("Expected ArrayU32");
    }

    let string_set: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
    if let Value::ArrayString(vec) = Value::from(string_set) {
      assert_eq!(vec.len(), 2);
      assert!(vec.contains(&"a".to_string()));
      assert!(vec.contains(&"b".to_string()));
    } else {
      panic!("Expected ArrayString");
    }

    // Test empty HashSet
    let empty_set: HashSet<u32> = HashSet::new();
    assert_eq!(Value::from(empty_set), Value::ArrayU32(vec![]));
  }

  #[test]
  fn test_from_conversions_empty_arrays() {
    // Test empty array conversions
    assert_eq!(Value::from(Vec::<bool>::new()), Value::ArrayBoolean(vec![]));
    assert_eq!(Value::from(Vec::<u8>::new()), Value::ArrayU8(vec![]));
    assert_eq!(Value::from(Vec::<u16>::new()), Value::ArrayU16(vec![]));
    assert_eq!(Value::from(Vec::<u32>::new()), Value::ArrayU32(vec![]));
    assert_eq!(Value::from(Vec::<u64>::new()), Value::ArrayU64(vec![]));
    assert_eq!(Value::from(Vec::<i8>::new()), Value::ArrayI8(vec![]));
    assert_eq!(Value::from(Vec::<i16>::new()), Value::ArrayI16(vec![]));
    assert_eq!(Value::from(Vec::<i32>::new()), Value::ArrayI32(vec![]));
    assert_eq!(Value::from(Vec::<i64>::new()), Value::ArrayI64(vec![]));
    assert_eq!(Value::from(Vec::<f32>::new()), Value::ArrayF32(vec![]));
    assert_eq!(Value::from(Vec::<f64>::new()), Value::ArrayF64(vec![]));
    assert_eq!(
      Value::from(Vec::<String>::new()),
      Value::ArrayString(vec![])
    );
  }

  #[test]
  fn test_from_conversions_option() {
    // Test Option<T> conversions for various primitive types

    // Test Some variants
    assert_eq!(
      Value::from(Some(42u32)),
      Value::Option(Some(Box::new(Value::U32(42))))
    );
    assert_eq!(
      Value::from(Some(true)),
      Value::Option(Some(Box::new(Value::Boolean(true))))
    );
    assert_eq!(
      Value::from(Some("hello".to_string())),
      Value::Option(Some(Box::new(Value::String("hello".to_string()))))
    );
    assert_eq!(
      Value::from(Some(3.14f64)),
      Value::Option(Some(Box::new(Value::F64(3.14))))
    );

    // Test None variants
    assert_eq!(Value::from(None::<u32>), Value::Option(None));
    assert_eq!(Value::from(None::<bool>), Value::Option(None));
    assert_eq!(Value::from(None::<String>), Value::Option(None));
    assert_eq!(Value::from(None::<f64>), Value::Option(None));

    // Test nested Option with Value
    let nested_value = Value::ArrayU32(vec![1, 2, 3]);
    assert_eq!(
      Value::from(Some(nested_value.clone())),
      Value::Option(Some(Box::new(nested_value)))
    );
    assert_eq!(Value::from(None::<Value>), Value::Option(None));
  }

  #[test]
  fn test_from_conversions_array_value() {
    // Test Vec<Value> -> ArrayValue conversions

    // Test empty Vec<Value>
    assert_eq!(Value::from(Vec::<Value>::new()), Value::ArrayValue(vec![]));

    // Test Vec<Value> with mixed types
    let mixed_values = vec![
      Value::U32(42),
      Value::Boolean(true),
      Value::String("test".to_string()),
      Value::F64(3.14),
      Value::Unit,
    ];
    assert_eq!(
      Value::from(mixed_values.clone()),
      Value::ArrayValue(mixed_values.clone())
    );

    // Test slice conversion
    let values_slice = &[
      Value::I32(-10),
      Value::Boolean(false),
      Value::String("slice".to_string()),
    ][..];
    assert_eq!(
      Value::from(values_slice),
      Value::ArrayValue(vec![
        Value::I32(-10),
        Value::Boolean(false),
        Value::String("slice".to_string()),
      ])
    );

    // Test ArrayValue with nested arrays
    let nested_array = vec![
      Value::ArrayU32(vec![1, 2, 3]),
      Value::ArrayString(vec!["a".to_string(), "b".to_string()]),
      Value::ArrayBoolean(vec![true, false]),
    ];
    assert_eq!(
      Value::from(nested_array.clone()),
      Value::ArrayValue(nested_array)
    );

    // Test ArrayValue with Options
    let option_array = vec![
      Value::Option(Some(Box::new(Value::U32(1)))),
      Value::Option(None),
      Value::Option(Some(Box::new(Value::String("test".to_string())))),
    ];
    assert_eq!(
      Value::from(option_array.clone()),
      Value::ArrayValue(option_array)
    );
  }
}
