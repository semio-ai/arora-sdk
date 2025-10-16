use js_sys::{Array, Object, Reflect};
use uuid::Uuid;
use wasm_bindgen::prelude::*;

use crate::gen_bb_uuid;
use crate::keyvalue::{KeyValue, KeyValueField};
use crate::value::{
  Enumeration, EnumerationWithoutId, Structure, StructureField, StructureWithoutId,
  Type as NativeType, Value as NativeValue,
};

/// ValueType enum exposed to JavaScript/TypeScript
///
/// This is a separate type from [`crate::value::Type`] because:
/// - `ValueType` needs `#[repr(u8)]` for stable WASM FFI with explicit discriminants
/// - `Type` needs `#[serde(rename)]` attributes for JSON serialization
/// - `wasm_bindgen` and `serde` attributes don't compose well on the same enum
///
/// Use `From` implementations to convert between them:
/// ```rust,ignore
/// let native_type = Type::F64;
/// let wasm_type: ValueType = native_type.into();
/// let back: Type = wasm_type.into();
/// ```
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ValueType {
  Unit = 0,
  Boolean = 1,
  U8 = 2,
  U16 = 3,
  U32 = 4,
  U64 = 5,
  I8 = 6,
  I16 = 7,
  I32 = 8,
  I64 = 9,
  F32 = 10,
  F64 = 11,
  String = 12,
  Option = 13,
  Structure = 14,
  Enumeration = 15,
  ArrayBoolean = 16,
  ArrayU8 = 17,
  ArrayU16 = 18,
  ArrayU32 = 19,
  ArrayU64 = 20,
  ArrayI8 = 21,
  ArrayI16 = 22,
  ArrayI32 = 23,
  ArrayI64 = 24,
  ArrayF32 = 25,
  ArrayF64 = 26,
  ArrayString = 27,
  ArrayValue = 28,
  ArrayStructure = 29,
  ArrayEnumeration = 30,
  KeyValue = 31,
  Uuid = 32,
}

// Conversions between ValueType and Type
impl From<NativeType> for ValueType {
  fn from(t: NativeType) -> Self {
    match t {
      NativeType::Unit => ValueType::Unit,
      NativeType::Boolean => ValueType::Boolean,
      NativeType::U8 => ValueType::U8,
      NativeType::U16 => ValueType::U16,
      NativeType::U32 => ValueType::U32,
      NativeType::U64 => ValueType::U64,
      NativeType::I8 => ValueType::I8,
      NativeType::I16 => ValueType::I16,
      NativeType::I32 => ValueType::I32,
      NativeType::I64 => ValueType::I64,
      NativeType::F32 => ValueType::F32,
      NativeType::F64 => ValueType::F64,
      NativeType::String => ValueType::String,
      NativeType::Option => ValueType::Option,
      NativeType::Structure => ValueType::Structure,
      NativeType::Enumeration => ValueType::Enumeration,
      NativeType::ArrayBoolean => ValueType::ArrayBoolean,
      NativeType::ArrayU8 => ValueType::ArrayU8,
      NativeType::ArrayU16 => ValueType::ArrayU16,
      NativeType::ArrayU32 => ValueType::ArrayU32,
      NativeType::ArrayU64 => ValueType::ArrayU64,
      NativeType::ArrayI8 => ValueType::ArrayI8,
      NativeType::ArrayI16 => ValueType::ArrayI16,
      NativeType::ArrayI32 => ValueType::ArrayI32,
      NativeType::ArrayI64 => ValueType::ArrayI64,
      NativeType::ArrayF32 => ValueType::ArrayF32,
      NativeType::ArrayF64 => ValueType::ArrayF64,
      NativeType::ArrayString => ValueType::ArrayString,
      NativeType::ArrayValue => ValueType::ArrayValue,
      NativeType::ArrayStructure => ValueType::ArrayStructure,
      NativeType::ArrayEnumeration => ValueType::ArrayEnumeration,
      NativeType::KeyValue => ValueType::KeyValue,
      NativeType::Uuid => ValueType::Uuid,
    }
  }
}

impl From<ValueType> for NativeType {
  fn from(vt: ValueType) -> Self {
    match vt {
      ValueType::Unit => NativeType::Unit,
      ValueType::Boolean => NativeType::Boolean,
      ValueType::U8 => NativeType::U8,
      ValueType::U16 => NativeType::U16,
      ValueType::U32 => NativeType::U32,
      ValueType::U64 => NativeType::U64,
      ValueType::I8 => NativeType::I8,
      ValueType::I16 => NativeType::I16,
      ValueType::I32 => NativeType::I32,
      ValueType::I64 => NativeType::I64,
      ValueType::F32 => NativeType::F32,
      ValueType::F64 => NativeType::F64,
      ValueType::String => NativeType::String,
      ValueType::Option => NativeType::Option,
      ValueType::Structure => NativeType::Structure,
      ValueType::Enumeration => NativeType::Enumeration,
      ValueType::ArrayBoolean => NativeType::ArrayBoolean,
      ValueType::ArrayU8 => NativeType::ArrayU8,
      ValueType::ArrayU16 => NativeType::ArrayU16,
      ValueType::ArrayU32 => NativeType::ArrayU32,
      ValueType::ArrayU64 => NativeType::ArrayU64,
      ValueType::ArrayI8 => NativeType::ArrayI8,
      ValueType::ArrayI16 => NativeType::ArrayI16,
      ValueType::ArrayI32 => NativeType::ArrayI32,
      ValueType::ArrayI64 => NativeType::ArrayI64,
      ValueType::ArrayF32 => NativeType::ArrayF32,
      ValueType::ArrayF64 => NativeType::ArrayF64,
      ValueType::ArrayString => NativeType::ArrayString,
      ValueType::ArrayValue => NativeType::ArrayValue,
      ValueType::ArrayStructure => NativeType::ArrayStructure,
      ValueType::ArrayEnumeration => NativeType::ArrayEnumeration,
      ValueType::KeyValue => NativeType::KeyValue,
      ValueType::Uuid => NativeType::Uuid,
    }
  }
}

// Helper functions for number conversions with range checking
fn parse_u8(value: &JsValue) -> Result<u8, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < 0.0 || n > u8::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for u8", n));
  }
  Ok(n as u8)
}

fn parse_u16(value: &JsValue) -> Result<u16, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < 0.0 || n > u16::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for u16", n));
  }
  Ok(n as u16)
}

fn parse_u32(value: &JsValue) -> Result<u32, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < 0.0 || n > u32::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for u32", n));
  }
  Ok(n as u32)
}

fn parse_u64(value: &JsValue) -> Result<u64, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < 0.0 || n > u64::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for u64", n));
  }
  Ok(n as u64)
}

fn parse_i8(value: &JsValue) -> Result<i8, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < i8::MIN as f64 || n > i8::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for i8", n));
  }
  Ok(n as i8)
}

fn parse_i16(value: &JsValue) -> Result<i16, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < i16::MIN as f64 || n > i16::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for i16", n));
  }
  Ok(n as i16)
}

fn parse_i32(value: &JsValue) -> Result<i32, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < i32::MIN as f64 || n > i32::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for i32", n));
  }
  Ok(n as i32)
}

fn parse_i64(value: &JsValue) -> Result<i64, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  if n < i64::MIN as f64 || n > i64::MAX as f64 || n.fract() != 0.0 {
    return Err(format!("Value {} out of range for i64", n));
  }
  Ok(n as i64)
}

fn parse_f32(value: &JsValue) -> Result<f32, String> {
  let n = value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())?;
  Ok(n as f32)
}

fn parse_f64(value: &JsValue) -> Result<f64, String> {
  value
    .as_f64()
    .ok_or_else(|| "Expected number value".to_string())
}

// Macro to parse typed arrays
macro_rules! parse_typed_array {
  ($value:expr, $parser:expr, $type_name:expr) => {{
    let arr = Array::from($value);
    let mut vec = Vec::new();
    for i in 0..arr.length() {
      let item = arr.get(i);
      vec.push($parser(&item).map_err(|e| format!("Array element {} error: {}", i, e))?);
    }
    vec
  }};
}

/// WASM wrapper for Value
#[wasm_bindgen]
pub struct Value {
  inner: NativeValue,
}

#[wasm_bindgen]
impl Value {
  /// Create a new Value with the specified type and JavaScript value
  #[wasm_bindgen(constructor)]
  pub fn new(value_type: ValueType, value: JsValue) -> Result<Value, String> {
    let inner = match value_type {
      ValueType::Unit => NativeValue::Unit,

      ValueType::Boolean => {
        let b = value
          .as_bool()
          .ok_or_else(|| "Expected boolean value".to_string())?;
        NativeValue::Boolean(b)
      }

      ValueType::U8 => NativeValue::U8(parse_u8(&value)?),
      ValueType::U16 => NativeValue::U16(parse_u16(&value)?),
      ValueType::U32 => NativeValue::U32(parse_u32(&value)?),
      ValueType::U64 => NativeValue::U64(parse_u64(&value)?),
      ValueType::I8 => NativeValue::I8(parse_i8(&value)?),
      ValueType::I16 => NativeValue::I16(parse_i16(&value)?),
      ValueType::I32 => NativeValue::I32(parse_i32(&value)?),
      ValueType::I64 => NativeValue::I64(parse_i64(&value)?),
      ValueType::F32 => NativeValue::F32(parse_f32(&value)?),
      ValueType::F64 => NativeValue::F64(parse_f64(&value)?),

      ValueType::String => {
        let s = value
          .as_string()
          .ok_or_else(|| "Expected string value".to_string())?;
        NativeValue::String(s)
      }

      ValueType::Option => {
        if value.is_null() || value.is_undefined() {
          NativeValue::Option(None)
        } else {
          let inner_value = Value::from(value)?;
          NativeValue::Option(Some(Box::new(inner_value.inner)))
        }
      }

      ValueType::Uuid => {
        let s = value
          .as_string()
          .ok_or_else(|| "Expected string value for UUID".to_string())?;
        let uuid = Uuid::parse_str(&s).map_err(|e| format!("Invalid UUID: {}", e))?;
        NativeValue::Uuid(uuid)
      }

      ValueType::ArrayBoolean => {
        let vec = parse_typed_array!(
          &value,
          |v: &JsValue| {
            v.as_bool()
              .ok_or_else(|| "Expected boolean value".to_string())
          },
          "boolean"
        );
        NativeValue::ArrayBoolean(vec)
      }

      ValueType::ArrayU8 => NativeValue::ArrayU8(parse_typed_array!(&value, parse_u8, "u8")),
      ValueType::ArrayU16 => NativeValue::ArrayU16(parse_typed_array!(&value, parse_u16, "u16")),
      ValueType::ArrayU32 => NativeValue::ArrayU32(parse_typed_array!(&value, parse_u32, "u32")),
      ValueType::ArrayU64 => NativeValue::ArrayU64(parse_typed_array!(&value, parse_u64, "u64")),
      ValueType::ArrayI8 => NativeValue::ArrayI8(parse_typed_array!(&value, parse_i8, "i8")),
      ValueType::ArrayI16 => NativeValue::ArrayI16(parse_typed_array!(&value, parse_i16, "i16")),
      ValueType::ArrayI32 => NativeValue::ArrayI32(parse_typed_array!(&value, parse_i32, "i32")),
      ValueType::ArrayI64 => NativeValue::ArrayI64(parse_typed_array!(&value, parse_i64, "i64")),
      ValueType::ArrayF32 => NativeValue::ArrayF32(parse_typed_array!(&value, parse_f32, "f32")),
      ValueType::ArrayF64 => NativeValue::ArrayF64(parse_typed_array!(&value, parse_f64, "f64")),

      ValueType::ArrayString => {
        let vec = parse_typed_array!(
          &value,
          |v: &JsValue| {
            v.as_string()
              .ok_or_else(|| "Expected string value".to_string())
          },
          "string"
        );
        NativeValue::ArrayString(vec)
      }

      ValueType::ArrayValue => {
        let arr = Array::from(&value);
        let mut vec = Vec::new();
        for i in 0..arr.length() {
          let item = arr.get(i);
          let v = Value::from(item)?;
          vec.push(v.inner);
        }
        NativeValue::ArrayValue(vec)
      }

      ValueType::Structure
      | ValueType::Enumeration
      | ValueType::ArrayStructure
      | ValueType::ArrayEnumeration => {
        return Err("Structure and Enumeration types must be created from JSON".to_string());
      }

      ValueType::KeyValue => {
        // Convert JS object to KeyValue
        let obj = Object::from(value);
        let entries = Object::entries(&obj);
        let mut fields = std::collections::HashMap::new();

        for i in 0..entries.length() {
          let entry = Array::from(&entries.get(i));
          let key = entry
            .get(0)
            .as_string()
            .ok_or_else(|| format!("Key {} is not a string", i))?;
          let val = entry.get(1);
          let value = Value::from(val)?;

          let field = KeyValueField {
            id: gen_bb_uuid(),
            name: key.clone(),
            value: Some(Box::new(value.inner)),
          };
          fields.insert(key, field);
        }

        let kv = KeyValue {
          id: gen_bb_uuid(),
          fields,
        };
        NativeValue::KeyValue(kv)
      }
    };

    Ok(Value { inner })
  }

  /// Get the type of this value
  #[wasm_bindgen(getter)]
  pub fn r#type(&self) -> ValueType {
    match &self.inner {
      NativeValue::Unit => ValueType::Unit,
      NativeValue::Boolean(_) => ValueType::Boolean,
      NativeValue::U8(_) => ValueType::U8,
      NativeValue::U16(_) => ValueType::U16,
      NativeValue::U32(_) => ValueType::U32,
      NativeValue::U64(_) => ValueType::U64,
      NativeValue::I8(_) => ValueType::I8,
      NativeValue::I16(_) => ValueType::I16,
      NativeValue::I32(_) => ValueType::I32,
      NativeValue::I64(_) => ValueType::I64,
      NativeValue::F32(_) => ValueType::F32,
      NativeValue::F64(_) => ValueType::F64,
      NativeValue::String(_) => ValueType::String,
      NativeValue::Option(_) => ValueType::Option,
      NativeValue::Structure(_) => ValueType::Structure,
      NativeValue::Enumeration(_) => ValueType::Enumeration,
      NativeValue::ArrayBoolean(_) => ValueType::ArrayBoolean,
      NativeValue::ArrayU8(_) => ValueType::ArrayU8,
      NativeValue::ArrayU16(_) => ValueType::ArrayU16,
      NativeValue::ArrayU32(_) => ValueType::ArrayU32,
      NativeValue::ArrayU64(_) => ValueType::ArrayU64,
      NativeValue::ArrayI8(_) => ValueType::ArrayI8,
      NativeValue::ArrayI16(_) => ValueType::ArrayI16,
      NativeValue::ArrayI32(_) => ValueType::ArrayI32,
      NativeValue::ArrayI64(_) => ValueType::ArrayI64,
      NativeValue::ArrayF32(_) => ValueType::ArrayF32,
      NativeValue::ArrayF64(_) => ValueType::ArrayF64,
      NativeValue::ArrayString(_) => ValueType::ArrayString,
      NativeValue::ArrayValue(_) => ValueType::ArrayValue,
      NativeValue::ArrayStructure { .. } => ValueType::ArrayStructure,
      NativeValue::ArrayEnumeration { .. } => ValueType::ArrayEnumeration,
      NativeValue::KeyValue(_) => ValueType::KeyValue,
      NativeValue::Uuid(_) => ValueType::Uuid,
    }
  }

  /// Set the value with type checking
  pub fn set(&mut self, value: JsValue) -> Result<(), String> {
    let value_type = self.r#type();
    let new_value = Value::new(value_type, value)?;
    self.inner = new_value.inner;
    Ok(())
  }

  /// Get the value as a JavaScript value
  pub fn get(&self) -> JsValue {
    value_to_js(&self.inner, None)
  }

  /// Get the value with optional type registry for complex types
  #[wasm_bindgen(js_name = getAs)]
  pub fn get_as(&self, type_registry: JsValue) -> Result<JsValue, String> {
    // For now, type_registry is unused - placeholder for future implementation
    let registry = if type_registry.is_null() || type_registry.is_undefined() {
      None
    } else {
      Some(type_registry)
    };
    Ok(value_to_js(&self.inner, registry))
  }

  /// Create a Value from a JavaScript value with automatic type detection
  pub fn from(value: JsValue) -> Result<Value, String> {
    let inner = js_to_value(&value)?;
    Ok(Value { inner })
  }
}

/// Convert a NativeValue to JsValue
fn value_to_js(value: &NativeValue, _type_registry: Option<JsValue>) -> JsValue {
  match value {
    NativeValue::Unit => JsValue::NULL,
    NativeValue::Boolean(b) => JsValue::from(*b),
    NativeValue::U8(n) => JsValue::from(*n),
    NativeValue::U16(n) => JsValue::from(*n),
    NativeValue::U32(n) => JsValue::from(*n),
    NativeValue::U64(n) => JsValue::from(*n as f64),
    NativeValue::I8(n) => JsValue::from(*n),
    NativeValue::I16(n) => JsValue::from(*n),
    NativeValue::I32(n) => JsValue::from(*n),
    NativeValue::I64(n) => JsValue::from(*n as f64),
    NativeValue::F32(n) => JsValue::from(*n),
    NativeValue::F64(n) => JsValue::from(*n),
    NativeValue::String(s) => JsValue::from(s.as_str()),
    NativeValue::Uuid(u) => JsValue::from(u.to_string()),

    NativeValue::Option(opt) => match opt {
      Some(v) => value_to_js(v, _type_registry.clone()),
      None => JsValue::NULL,
    },

    NativeValue::ArrayBoolean(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayU8(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayU16(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayU32(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayU64(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item as f64));
      }
      js_arr.into()
    }

    NativeValue::ArrayI8(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayI16(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayI32(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayI64(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item as f64));
      }
      js_arr.into()
    }

    NativeValue::ArrayF32(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayF64(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    NativeValue::ArrayString(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(item.as_str()));
      }
      js_arr.into()
    }

    NativeValue::ArrayValue(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&value_to_js(item, _type_registry.clone()));
      }
      js_arr.into()
    }

    NativeValue::Structure(s) => structure_to_js(s),
    NativeValue::Enumeration(e) => enumeration_to_js(e),

    NativeValue::ArrayStructure { id, elements } => {
      let obj = Object::new();
      Reflect::set(&obj, &JsValue::from("id"), &JsValue::from(id.to_string())).unwrap();

      let js_arr = Array::new();
      for elem in elements {
        js_arr.push(&structure_without_id_to_js(elem));
      }
      Reflect::set(&obj, &JsValue::from("elements"), &js_arr).unwrap();
      obj.into()
    }

    NativeValue::ArrayEnumeration { id, elements } => {
      let obj = Object::new();
      Reflect::set(&obj, &JsValue::from("id"), &JsValue::from(id.to_string())).unwrap();

      let js_arr = Array::new();
      for elem in elements {
        js_arr.push(&enumeration_without_id_to_js(elem));
      }
      Reflect::set(&obj, &JsValue::from("elements"), &js_arr).unwrap();
      obj.into()
    }

    NativeValue::KeyValue(kv) => {
      let obj = Object::new();
      for (key, field) in &kv.fields {
        if let Some(val) = &field.value {
          Reflect::set(
            &obj,
            &JsValue::from(key.as_str()),
            &value_to_js(val, _type_registry.clone()),
          )
          .unwrap();
        }
      }
      obj.into()
    }
  }
}

fn structure_to_js(s: &Structure) -> JsValue {
  let obj = Object::new();
  Reflect::set(&obj, &JsValue::from("id"), &JsValue::from(s.id.to_string())).unwrap();

  let fields_obj = Object::new();
  for field in &s.fields {
    Reflect::set(
      &fields_obj,
      &JsValue::from(field.id.to_string()),
      &value_to_js(&field.value, None),
    )
    .unwrap();
  }
  Reflect::set(&obj, &JsValue::from("fields"), &fields_obj).unwrap();
  obj.into()
}

fn structure_without_id_to_js(s: &StructureWithoutId) -> JsValue {
  let fields_obj = Object::new();
  for field in &s.fields {
    Reflect::set(
      &fields_obj,
      &JsValue::from(field.id.to_string()),
      &value_to_js(&field.value, None),
    )
    .unwrap();
  }
  fields_obj.into()
}

fn enumeration_to_js(e: &Enumeration) -> JsValue {
  let obj = Object::new();
  Reflect::set(&obj, &JsValue::from("id"), &JsValue::from(e.id.to_string())).unwrap();
  Reflect::set(
    &obj,
    &JsValue::from("variant_id"),
    &JsValue::from(e.variant_id.to_string()),
  )
  .unwrap();
  Reflect::set(&obj, &JsValue::from("value"), &value_to_js(&e.value, None)).unwrap();
  obj.into()
}

fn enumeration_without_id_to_js(e: &EnumerationWithoutId) -> JsValue {
  let obj = Object::new();
  Reflect::set(
    &obj,
    &JsValue::from("variant_id"),
    &JsValue::from(e.variant_id.to_string()),
  )
  .unwrap();
  Reflect::set(&obj, &JsValue::from("value"), &value_to_js(&e.value, None)).unwrap();
  obj.into()
}

/// Convert JsValue to NativeValue with automatic type detection
fn js_to_value(value: &JsValue) -> Result<NativeValue, String> {
  if value.is_null() || value.is_undefined() {
    return Ok(NativeValue::Unit);
  }

  if let Some(b) = value.as_bool() {
    return Ok(NativeValue::Boolean(b));
  }

  if let Some(n) = value.as_f64() {
    // Default to f64 for JavaScript numbers
    return Ok(NativeValue::F64(n));
  }

  if let Some(s) = value.as_string() {
    return Ok(NativeValue::String(s));
  }

  // Check if it's an array
  if Array::is_array(value) {
    let arr = Array::from(value);
    if arr.length() == 0 {
      // Empty array defaults to ArrayValue
      return Ok(NativeValue::ArrayValue(vec![]));
    }

    // Check if all elements are of the same primitive type
    let mut all_bool = true;
    let mut all_number = true;
    let mut all_string = true;

    for i in 0..arr.length() {
      let item = arr.get(i);
      if item.is_null() || item.is_undefined() {
        // null/undefined means it's a mixed array
        all_bool = false;
        all_number = false;
        all_string = false;
        break;
      }
      if item.as_bool().is_none() {
        all_bool = false;
      }
      if item.as_f64().is_none() {
        all_number = false;
      }
      if item.as_string().is_none() {
        all_string = false;
      }
    }

    if all_bool {
      // Boolean array
      let mut vec = Vec::new();
      for i in 0..arr.length() {
        let item = arr.get(i);
        vec.push(item.as_bool().unwrap());
      }
      return Ok(NativeValue::ArrayBoolean(vec));
    }

    if all_number {
      // Number array - default to f64
      let mut vec = Vec::new();
      for i in 0..arr.length() {
        let item = arr.get(i);
        vec.push(item.as_f64().unwrap());
      }
      return Ok(NativeValue::ArrayF64(vec));
    }

    if all_string {
      // String array
      let mut vec = Vec::new();
      for i in 0..arr.length() {
        let item = arr.get(i);
        vec.push(item.as_string().unwrap());
      }
      return Ok(NativeValue::ArrayString(vec));
    }

    // Mixed or complex array - use ArrayValue
    let mut vec = Vec::new();
    for i in 0..arr.length() {
      let item = arr.get(i);
      vec.push(js_to_value(&item)?);
    }
    return Ok(NativeValue::ArrayValue(vec));
  }

  // Check if it's an object
  if value.is_object() {
    let obj = Object::from(value.clone());

    // Check if it's a Structure
    if let Ok(id_val) = Reflect::get(&obj, &JsValue::from("id")) {
      if let Some(id_str) = id_val.as_string() {
        if let Ok(id) = Uuid::parse_str(&id_str) {
          // Check for variant_id (Enumeration)
          if let Ok(variant_val) = Reflect::get(&obj, &JsValue::from("variant_id")) {
            if let Some(variant_str) = variant_val.as_string() {
              if let Ok(variant_id) = Uuid::parse_str(&variant_str) {
                let value_val = Reflect::get(&obj, &JsValue::from("value"))
                  .map_err(|_| "Missing value field in enumeration".to_string())?;
                let inner_value = js_to_value(&value_val)?;
                return Ok(NativeValue::Enumeration(Enumeration {
                  id,
                  variant_id,
                  value: Box::new(inner_value),
                }));
              }
            }
          }

          // Check for fields (Structure)
          if let Ok(fields_val) = Reflect::get(&obj, &JsValue::from("fields")) {
            if fields_val.is_object() {
              let fields_obj = Object::from(fields_val);
              let entries = Object::entries(&fields_obj);
              let mut fields = Vec::new();

              for i in 0..entries.length() {
                let entry = Array::from(&entries.get(i));
                let field_id_str = entry
                  .get(0)
                  .as_string()
                  .ok_or_else(|| format!("Field key {} is not a string", i))?;
                let field_id = Uuid::parse_str(&field_id_str)
                  .map_err(|e| format!("Invalid field UUID: {}", e))?;
                let field_value = js_to_value(&entry.get(1))?;
                fields.push(StructureField {
                  id: field_id,
                  value: Box::new(field_value),
                });
              }

              return Ok(NativeValue::Structure(Structure { id, fields }));
            }
          }
        }
      }
    }

    // Regular object - convert to KeyValue
    let entries = Object::entries(&obj);
    let mut fields = std::collections::HashMap::new();

    for i in 0..entries.length() {
      let entry = Array::from(&entries.get(i));
      let key = entry
        .get(0)
        .as_string()
        .ok_or_else(|| format!("Key {} is not a string", i))?;
      let val = entry.get(1);
      let value = js_to_value(&val)?;

      let field = KeyValueField {
        id: gen_bb_uuid(),
        name: key.clone(),
        value: Some(Box::new(value)),
      };
      fields.insert(key, field);
    }

    let kv = KeyValue {
      id: gen_bb_uuid(),
      fields,
    };
    return Ok(NativeValue::KeyValue(kv));
  }

  Err(format!("Unsupported JavaScript value type"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use wasm_bindgen_test::*;

  wasm_bindgen_test_configure!(run_in_browser);

  #[wasm_bindgen_test]
  fn test_value_type_numbers() {
    // Verify enum values match expected numbers
    assert_eq!(ValueType::Unit as u8, 0);
    assert_eq!(ValueType::Boolean as u8, 1);
    assert_eq!(ValueType::U8 as u8, 2);
    assert_eq!(ValueType::F64 as u8, 11);
    assert_eq!(ValueType::Uuid as u8, 32);
  }

  #[wasm_bindgen_test]
  fn test_type_conversions() {
    // Test conversion from NativeType to ValueType
    assert_eq!(ValueType::from(NativeType::Unit), ValueType::Unit);
    assert_eq!(ValueType::from(NativeType::Boolean), ValueType::Boolean);
    assert_eq!(ValueType::from(NativeType::F64), ValueType::F64);
    assert_eq!(ValueType::from(NativeType::String), ValueType::String);
    assert_eq!(ValueType::from(NativeType::ArrayF32), ValueType::ArrayF32);

    // Test conversion from ValueType to NativeType
    assert_eq!(NativeType::from(ValueType::Unit), NativeType::Unit);
    assert_eq!(NativeType::from(ValueType::Boolean), NativeType::Boolean);
    assert_eq!(NativeType::from(ValueType::F64), NativeType::F64);
    assert_eq!(NativeType::from(ValueType::String), NativeType::String);
    assert_eq!(NativeType::from(ValueType::ArrayF32), NativeType::ArrayF32);

    // Test round-trip conversion
    let original = NativeType::Structure;
    let wasm_type: ValueType = original.clone().into();
    let back: NativeType = wasm_type.into();
    assert_eq!(original, back);
  }

  #[wasm_bindgen_test]
  fn test_primitive_values() {
    // Test Unit
    let unit_val = Value::new(ValueType::Unit, JsValue::NULL).unwrap();
    assert_eq!(unit_val.r#type(), ValueType::Unit);

    // Test Boolean
    let bool_val = Value::new(ValueType::Boolean, JsValue::from(true)).unwrap();
    assert_eq!(bool_val.r#type(), ValueType::Boolean);
    assert_eq!(bool_val.get().as_bool(), Some(true));

    // Test F64
    let f64_val = Value::new(ValueType::F64, JsValue::from(3.14)).unwrap();
    assert_eq!(f64_val.r#type(), ValueType::F64);
    assert_eq!(f64_val.get().as_f64(), Some(3.14));

    // Test String
    let str_val = Value::new(ValueType::String, JsValue::from("hello")).unwrap();
    assert_eq!(str_val.r#type(), ValueType::String);
    assert_eq!(str_val.get().as_string(), Some("hello".to_string()));
  }

  #[wasm_bindgen_test]
  fn test_integer_types() {
    // Test U8
    let u8_val = Value::new(ValueType::U8, JsValue::from(255)).unwrap();
    assert_eq!(u8_val.r#type(), ValueType::U8);

    // Test I32
    let i32_val = Value::new(ValueType::I32, JsValue::from(-42)).unwrap();
    assert_eq!(i32_val.r#type(), ValueType::I32);

    // Test out of range
    let result = Value::new(ValueType::U8, JsValue::from(256));
    assert!(result.is_err());
  }

  #[wasm_bindgen_test]
  fn test_array_values() {
    // Test boolean array
    let arr = Array::new();
    arr.push(&JsValue::from(true));
    arr.push(&JsValue::from(false));
    let bool_arr = Value::new(ValueType::ArrayBoolean, arr.into()).unwrap();
    assert_eq!(bool_arr.r#type(), ValueType::ArrayBoolean);

    // Test number array
    let arr = Array::new();
    arr.push(&JsValue::from(1.5));
    arr.push(&JsValue::from(2.5));
    let f64_arr = Value::new(ValueType::ArrayF64, arr.into()).unwrap();
    assert_eq!(f64_arr.r#type(), ValueType::ArrayF64);
  }

  #[wasm_bindgen_test]
  fn test_from_auto_detection() {
    // Test boolean
    let val = Value::from(JsValue::from(true)).unwrap();
    assert_eq!(val.r#type(), ValueType::Boolean);

    // Test number (defaults to f64)
    let val = Value::from(JsValue::from(42.0)).unwrap();
    assert_eq!(val.r#type(), ValueType::F64);

    // Test string
    let val = Value::from(JsValue::from("test")).unwrap();
    assert_eq!(val.r#type(), ValueType::String);

    // Test null
    let val = Value::from(JsValue::NULL).unwrap();
    assert_eq!(val.r#type(), ValueType::Unit);
  }

  #[wasm_bindgen_test]
  fn test_array_auto_detection() {
    // Test boolean array
    let arr = Array::new();
    arr.push(&JsValue::from(true));
    arr.push(&JsValue::from(false));
    let val = Value::from(arr.into()).unwrap();
    assert_eq!(val.r#type(), ValueType::ArrayBoolean);

    // Test number array
    let arr = Array::new();
    arr.push(&JsValue::from(1.0));
    arr.push(&JsValue::from(2.0));
    let val = Value::from(arr.into()).unwrap();
    assert_eq!(val.r#type(), ValueType::ArrayF64);

    // Test string array
    let arr = Array::new();
    arr.push(&JsValue::from("a"));
    arr.push(&JsValue::from("b"));
    let val = Value::from(arr.into()).unwrap();
    assert_eq!(val.r#type(), ValueType::ArrayString);
  }

  #[wasm_bindgen_test]
  fn test_set_method() {
    let mut val = Value::new(ValueType::F64, JsValue::from(1.0)).unwrap();
    assert_eq!(val.get().as_f64(), Some(1.0));

    val.set(JsValue::from(2.0)).unwrap();
    assert_eq!(val.get().as_f64(), Some(2.0));

    // Type mismatch should fail
    let result = val.set(JsValue::from("string"));
    assert!(result.is_err());
  }

  #[wasm_bindgen_test]
  fn test_option_value() {
    // Some value
    let some_val = Value::new(ValueType::Option, JsValue::from(42.0)).unwrap();
    assert_eq!(some_val.r#type(), ValueType::Option);

    // None value
    let none_val = Value::new(ValueType::Option, JsValue::NULL).unwrap();
    assert_eq!(none_val.r#type(), ValueType::Option);
    assert!(none_val.get().is_null());
  }
}
