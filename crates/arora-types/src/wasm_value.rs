use js_sys::{Array, Object, Reflect};
use uuid::Uuid;
use wasm_bindgen::prelude::*;

use crate::gen_bb_uuid;
use crate::keyvalue::{KeyValue, KeyValueField};
use crate::value::{
  Enumeration, EnumerationWithoutId, Structure, StructureField, StructureWithoutId, Type, Value,
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
#[wasm_bindgen(js_name=ValueType)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WasmType {
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
impl From<Type> for WasmType {
  fn from(t: Type) -> Self {
    match t {
      Type::Unit => WasmType::Unit,
      Type::Boolean => WasmType::Boolean,
      Type::U8 => WasmType::U8,
      Type::U16 => WasmType::U16,
      Type::U32 => WasmType::U32,
      Type::U64 => WasmType::U64,
      Type::I8 => WasmType::I8,
      Type::I16 => WasmType::I16,
      Type::I32 => WasmType::I32,
      Type::I64 => WasmType::I64,
      Type::F32 => WasmType::F32,
      Type::F64 => WasmType::F64,
      Type::String => WasmType::String,
      Type::Option => WasmType::Option,
      Type::Structure => WasmType::Structure,
      Type::Enumeration => WasmType::Enumeration,
      Type::ArrayBoolean => WasmType::ArrayBoolean,
      Type::ArrayU8 => WasmType::ArrayU8,
      Type::ArrayU16 => WasmType::ArrayU16,
      Type::ArrayU32 => WasmType::ArrayU32,
      Type::ArrayU64 => WasmType::ArrayU64,
      Type::ArrayI8 => WasmType::ArrayI8,
      Type::ArrayI16 => WasmType::ArrayI16,
      Type::ArrayI32 => WasmType::ArrayI32,
      Type::ArrayI64 => WasmType::ArrayI64,
      Type::ArrayF32 => WasmType::ArrayF32,
      Type::ArrayF64 => WasmType::ArrayF64,
      Type::ArrayString => WasmType::ArrayString,
      Type::ArrayValue => WasmType::ArrayValue,
      Type::ArrayStructure => WasmType::ArrayStructure,
      Type::ArrayEnumeration => WasmType::ArrayEnumeration,
      Type::KeyValue => WasmType::KeyValue,
      Type::Uuid => WasmType::Uuid,
    }
  }
}

impl From<WasmType> for Type {
  fn from(vt: WasmType) -> Self {
    match vt {
      WasmType::Unit => Type::Unit,
      WasmType::Boolean => Type::Boolean,
      WasmType::U8 => Type::U8,
      WasmType::U16 => Type::U16,
      WasmType::U32 => Type::U32,
      WasmType::U64 => Type::U64,
      WasmType::I8 => Type::I8,
      WasmType::I16 => Type::I16,
      WasmType::I32 => Type::I32,
      WasmType::I64 => Type::I64,
      WasmType::F32 => Type::F32,
      WasmType::F64 => Type::F64,
      WasmType::String => Type::String,
      WasmType::Option => Type::Option,
      WasmType::Structure => Type::Structure,
      WasmType::Enumeration => Type::Enumeration,
      WasmType::ArrayBoolean => Type::ArrayBoolean,
      WasmType::ArrayU8 => Type::ArrayU8,
      WasmType::ArrayU16 => Type::ArrayU16,
      WasmType::ArrayU32 => Type::ArrayU32,
      WasmType::ArrayU64 => Type::ArrayU64,
      WasmType::ArrayI8 => Type::ArrayI8,
      WasmType::ArrayI16 => Type::ArrayI16,
      WasmType::ArrayI32 => Type::ArrayI32,
      WasmType::ArrayI64 => Type::ArrayI64,
      WasmType::ArrayF32 => Type::ArrayF32,
      WasmType::ArrayF64 => Type::ArrayF64,
      WasmType::ArrayString => Type::ArrayString,
      WasmType::ArrayValue => Type::ArrayValue,
      WasmType::ArrayStructure => Type::ArrayStructure,
      WasmType::ArrayEnumeration => Type::ArrayEnumeration,
      WasmType::KeyValue => Type::KeyValue,
      WasmType::Uuid => Type::Uuid,
    }
  }
}

impl From<&Value> for WasmType {
  fn from(value: &Value) -> Self {
    match value {
      Value::Unit => WasmType::Unit,
      Value::Boolean(_) => WasmType::Boolean,
      Value::U8(_) => WasmType::U8,
      Value::U16(_) => WasmType::U16,
      Value::U32(_) => WasmType::U32,
      Value::U64(_) => WasmType::U64,
      Value::I8(_) => WasmType::I8,
      Value::I16(_) => WasmType::I16,
      Value::I32(_) => WasmType::I32,
      Value::I64(_) => WasmType::I64,
      Value::F32(_) => WasmType::F32,
      Value::F64(_) => WasmType::F64,
      Value::String(_) => WasmType::String,
      Value::Option(_) => WasmType::Option,
      Value::Structure(_) => WasmType::Structure,
      Value::Enumeration(_) => WasmType::Enumeration,
      Value::ArrayBoolean(_) => WasmType::ArrayBoolean,
      Value::ArrayU8(_) => WasmType::ArrayU8,
      Value::ArrayU16(_) => WasmType::ArrayU16,
      Value::ArrayU32(_) => WasmType::ArrayU32,
      Value::ArrayU64(_) => WasmType::ArrayU64,
      Value::ArrayI8(_) => WasmType::ArrayI8,
      Value::ArrayI16(_) => WasmType::ArrayI16,
      Value::ArrayI32(_) => WasmType::ArrayI32,
      Value::ArrayI64(_) => WasmType::ArrayI64,
      Value::ArrayF32(_) => WasmType::ArrayF32,
      Value::ArrayF64(_) => WasmType::ArrayF64,
      Value::ArrayString(_) => WasmType::ArrayString,
      Value::ArrayValue(_) => WasmType::ArrayValue,
      Value::ArrayStructure { .. } => WasmType::ArrayStructure,
      Value::ArrayEnumeration { .. } => WasmType::ArrayEnumeration,
      Value::KeyValue(_) => WasmType::KeyValue,
      Value::Uuid(_) => WasmType::Uuid,
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
#[wasm_bindgen(js_name=Value)]
#[derive(Debug, Clone, PartialEq)]
pub struct WasmValue {
  inner: Value,
}

#[wasm_bindgen(js_class=Value)]
impl WasmValue {
  /// Create a Unit value (represents no value/void)
  #[wasm_bindgen]
  pub fn unit() -> WasmValue {
    WasmValue { inner: Value::Unit }
  }

  /// Create a new Value with the specified type and JavaScript value
  #[wasm_bindgen(constructor)]
  pub fn new(value_type: WasmType, value: JsValue) -> Result<WasmValue, String> {
    let inner = match value_type {
      WasmType::Unit => Value::Unit,

      WasmType::Boolean => {
        let b = value
          .as_bool()
          .ok_or_else(|| "Expected boolean value".to_string())?;
        Value::Boolean(b)
      }

      WasmType::U8 => Value::U8(parse_u8(&value)?),
      WasmType::U16 => Value::U16(parse_u16(&value)?),
      WasmType::U32 => Value::U32(parse_u32(&value)?),
      WasmType::U64 => Value::U64(parse_u64(&value)?),
      WasmType::I8 => Value::I8(parse_i8(&value)?),
      WasmType::I16 => Value::I16(parse_i16(&value)?),
      WasmType::I32 => Value::I32(parse_i32(&value)?),
      WasmType::I64 => Value::I64(parse_i64(&value)?),
      WasmType::F32 => Value::F32(parse_f32(&value)?),
      WasmType::F64 => Value::F64(parse_f64(&value)?),

      WasmType::String => {
        let s = value
          .as_string()
          .ok_or_else(|| "Expected string value".to_string())?;
        Value::String(s)
      }

      WasmType::Option => {
        if value.is_null() || value.is_undefined() {
          Value::Option(None)
        } else {
          let inner_value = WasmValue::from(value)?;
          Value::Option(Some(Box::new(inner_value.inner)))
        }
      }

      WasmType::Uuid => {
        let s = value
          .as_string()
          .ok_or_else(|| "Expected string value for UUID".to_string())?;
        let uuid = Uuid::parse_str(&s).map_err(|e| format!("Invalid UUID: {}", e))?;
        Value::Uuid(uuid)
      }

      WasmType::ArrayBoolean => {
        let vec = parse_typed_array!(
          &value,
          |v: &JsValue| {
            v.as_bool()
              .ok_or_else(|| "Expected boolean value".to_string())
          },
          "boolean"
        );
        Value::ArrayBoolean(vec)
      }

      WasmType::ArrayU8 => Value::ArrayU8(parse_typed_array!(&value, parse_u8, "u8")),
      WasmType::ArrayU16 => Value::ArrayU16(parse_typed_array!(&value, parse_u16, "u16")),
      WasmType::ArrayU32 => Value::ArrayU32(parse_typed_array!(&value, parse_u32, "u32")),
      WasmType::ArrayU64 => Value::ArrayU64(parse_typed_array!(&value, parse_u64, "u64")),
      WasmType::ArrayI8 => Value::ArrayI8(parse_typed_array!(&value, parse_i8, "i8")),
      WasmType::ArrayI16 => Value::ArrayI16(parse_typed_array!(&value, parse_i16, "i16")),
      WasmType::ArrayI32 => Value::ArrayI32(parse_typed_array!(&value, parse_i32, "i32")),
      WasmType::ArrayI64 => Value::ArrayI64(parse_typed_array!(&value, parse_i64, "i64")),
      WasmType::ArrayF32 => Value::ArrayF32(parse_typed_array!(&value, parse_f32, "f32")),
      WasmType::ArrayF64 => Value::ArrayF64(parse_typed_array!(&value, parse_f64, "f64")),

      WasmType::ArrayString => {
        let vec = parse_typed_array!(
          &value,
          |v: &JsValue| {
            v.as_string()
              .ok_or_else(|| "Expected string value".to_string())
          },
          "string"
        );
        Value::ArrayString(vec)
      }

      WasmType::ArrayValue => {
        let arr = Array::from(&value);
        let mut vec = Vec::new();
        for i in 0..arr.length() {
          let item = arr.get(i);
          let v = WasmValue::from(item)?;
          vec.push(v.inner);
        }
        Value::ArrayValue(vec)
      }

      WasmType::Structure
      | WasmType::Enumeration
      | WasmType::ArrayStructure
      | WasmType::ArrayEnumeration => {
        return Err("Structure and Enumeration types must be created from JSON".to_string());
      }

      WasmType::KeyValue => {
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
          let value = WasmValue::from(val)?;

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
        Value::KeyValue(kv)
      }
    };

    Ok(WasmValue { inner })
  }

  /// Get the type of this value
  #[wasm_bindgen(getter)]
  pub fn r#type(&self) -> WasmType {
    match &self.inner {
      Value::Unit => WasmType::Unit,
      Value::Boolean(_) => WasmType::Boolean,
      Value::U8(_) => WasmType::U8,
      Value::U16(_) => WasmType::U16,
      Value::U32(_) => WasmType::U32,
      Value::U64(_) => WasmType::U64,
      Value::I8(_) => WasmType::I8,
      Value::I16(_) => WasmType::I16,
      Value::I32(_) => WasmType::I32,
      Value::I64(_) => WasmType::I64,
      Value::F32(_) => WasmType::F32,
      Value::F64(_) => WasmType::F64,
      Value::String(_) => WasmType::String,
      Value::Option(_) => WasmType::Option,
      Value::Structure(_) => WasmType::Structure,
      Value::Enumeration(_) => WasmType::Enumeration,
      Value::ArrayBoolean(_) => WasmType::ArrayBoolean,
      Value::ArrayU8(_) => WasmType::ArrayU8,
      Value::ArrayU16(_) => WasmType::ArrayU16,
      Value::ArrayU32(_) => WasmType::ArrayU32,
      Value::ArrayU64(_) => WasmType::ArrayU64,
      Value::ArrayI8(_) => WasmType::ArrayI8,
      Value::ArrayI16(_) => WasmType::ArrayI16,
      Value::ArrayI32(_) => WasmType::ArrayI32,
      Value::ArrayI64(_) => WasmType::ArrayI64,
      Value::ArrayF32(_) => WasmType::ArrayF32,
      Value::ArrayF64(_) => WasmType::ArrayF64,
      Value::ArrayString(_) => WasmType::ArrayString,
      Value::ArrayValue(_) => WasmType::ArrayValue,
      Value::ArrayStructure { .. } => WasmType::ArrayStructure,
      Value::ArrayEnumeration { .. } => WasmType::ArrayEnumeration,
      Value::KeyValue(_) => WasmType::KeyValue,
      Value::Uuid(_) => WasmType::Uuid,
    }
  }

  /// Set the value with type checking
  pub fn set(&mut self, value: JsValue) -> Result<(), String> {
    let value_type = self.r#type();
    let new_value = WasmValue::new(value_type, value)?;
    self.inner = new_value.inner;
    Ok(())
  }

  /// Get the value as a JavaScript value
  #[wasm_bindgen]
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
  pub fn from(value: JsValue) -> Result<WasmValue, String> {
    let inner = js_to_value(&value)?;
    Ok(WasmValue { inner })
  }
}

// Conversion traits for zero-copy conversions between WASM and native types
impl From<Value> for WasmValue {
  fn from(inner: Value) -> Self {
    WasmValue { inner }
  }
}

impl From<WasmValue> for Value {
  fn from(value: WasmValue) -> Self {
    value.inner
  }
}

impl AsRef<Value> for WasmValue {
  fn as_ref(&self) -> &Value {
    &self.inner
  }
}

impl AsMut<Value> for WasmValue {
  fn as_mut(&mut self) -> &mut Value {
    &mut self.inner
  }
}

/// Convert a Value to JsValue
fn value_to_js(value: &Value, _type_registry: Option<JsValue>) -> JsValue {
  match value {
    Value::Unit => JsValue::UNDEFINED,
    Value::Boolean(b) => JsValue::from(*b),
    Value::U8(n) => JsValue::from(*n),
    Value::U16(n) => JsValue::from(*n),
    Value::U32(n) => JsValue::from(*n),
    Value::U64(n) => JsValue::from(*n as f64),
    Value::I8(n) => JsValue::from(*n),
    Value::I16(n) => JsValue::from(*n),
    Value::I32(n) => JsValue::from(*n),
    Value::I64(n) => JsValue::from(*n as f64),
    Value::F32(n) => JsValue::from(*n),
    Value::F64(n) => JsValue::from(*n),
    Value::String(s) => JsValue::from(s.as_str()),
    Value::Uuid(u) => JsValue::from(u.to_string()),

    Value::Option(opt) => match opt {
      Some(v) => value_to_js(v, _type_registry.clone()),
      None => JsValue::NULL,
    },

    Value::ArrayBoolean(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayU8(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayU16(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayU32(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayU64(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item as f64));
      }
      js_arr.into()
    }

    Value::ArrayI8(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayI16(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayI32(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayI64(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item as f64));
      }
      js_arr.into()
    }

    Value::ArrayF32(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayF64(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(*item));
      }
      js_arr.into()
    }

    Value::ArrayString(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&JsValue::from(item.as_str()));
      }
      js_arr.into()
    }

    Value::ArrayValue(arr) => {
      let js_arr = Array::new();
      for item in arr {
        js_arr.push(&value_to_js(item, _type_registry.clone()));
      }
      js_arr.into()
    }

    Value::Structure(s) => structure_to_js(s),
    Value::Enumeration(e) => enumeration_to_js(e),

    Value::ArrayStructure { id, elements } => {
      let obj = Object::new();
      Reflect::set(&obj, &JsValue::from("id"), &JsValue::from(id.to_string())).unwrap();

      let js_arr = Array::new();
      for elem in elements {
        js_arr.push(&structure_without_id_to_js(elem));
      }
      Reflect::set(&obj, &JsValue::from("elements"), &js_arr).unwrap();
      obj.into()
    }

    Value::ArrayEnumeration { id, elements } => {
      let obj = Object::new();
      Reflect::set(&obj, &JsValue::from("id"), &JsValue::from(id.to_string())).unwrap();

      let js_arr = Array::new();
      for elem in elements {
        js_arr.push(&enumeration_without_id_to_js(elem));
      }
      Reflect::set(&obj, &JsValue::from("elements"), &js_arr).unwrap();
      obj.into()
    }

    Value::KeyValue(kv) => {
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

/// Convert JsValue to Value with automatic type detection
fn js_to_value(value: &JsValue) -> Result<Value, String> {
  if value.is_null() || value.is_undefined() {
    return Ok(Value::Option(None));
  }

  if let Some(b) = value.as_bool() {
    return Ok(Value::Boolean(b));
  }

  if let Some(n) = value.as_f64() {
    // Default to f64 for JavaScript numbers
    return Ok(Value::F64(n));
  }

  if let Some(s) = value.as_string() {
    return Ok(Value::String(s));
  }

  // Check if it's an array
  if Array::is_array(value) {
    let arr = Array::from(value);
    if arr.length() == 0 {
      // Empty array defaults to ArrayValue
      return Ok(Value::ArrayValue(vec![]));
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
      return Ok(Value::ArrayBoolean(vec));
    }

    if all_number {
      // Number array - default to f64
      let mut vec = Vec::new();
      for i in 0..arr.length() {
        let item = arr.get(i);
        vec.push(item.as_f64().unwrap());
      }
      return Ok(Value::ArrayF64(vec));
    }

    if all_string {
      // String array
      let mut vec = Vec::new();
      for i in 0..arr.length() {
        let item = arr.get(i);
        vec.push(item.as_string().unwrap());
      }
      return Ok(Value::ArrayString(vec));
    }

    // Mixed or complex array - use ArrayValue
    let mut vec = Vec::new();
    for i in 0..arr.length() {
      let item = arr.get(i);
      vec.push(js_to_value(&item)?);
    }
    return Ok(Value::ArrayValue(vec));
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
                return Ok(Value::Enumeration(Enumeration {
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

              return Ok(Value::Structure(Structure { id, fields }));
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
    return Ok(Value::KeyValue(kv));
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
    assert_eq!(WasmType::Unit as u8, 0);
    assert_eq!(WasmType::Boolean as u8, 1);
    assert_eq!(WasmType::U8 as u8, 2);
    assert_eq!(WasmType::F64 as u8, 11);
    assert_eq!(WasmType::Uuid as u8, 32);
  }

  #[wasm_bindgen_test]
  fn test_type_conversions() {
    // Test conversion from Type to ValueType
    assert_eq!(WasmType::from(Type::Unit), WasmType::Unit);
    assert_eq!(WasmType::from(Type::Boolean), WasmType::Boolean);
    assert_eq!(WasmType::from(Type::F64), WasmType::F64);
    assert_eq!(WasmType::from(Type::String), WasmType::String);
    assert_eq!(WasmType::from(Type::ArrayF32), WasmType::ArrayF32);

    // Test conversion from ValueType to Type
    assert_eq!(Type::from(WasmType::Unit), Type::Unit);
    assert_eq!(Type::from(WasmType::Boolean), Type::Boolean);
    assert_eq!(Type::from(WasmType::F64), Type::F64);
    assert_eq!(Type::from(WasmType::String), Type::String);
    assert_eq!(Type::from(WasmType::ArrayF32), Type::ArrayF32);

    // Test round-trip conversion
    let original = Type::Structure;
    let wasm_type: WasmType = original.clone().into();
    let back: Type = wasm_type.into();
    assert_eq!(original, back);
  }

  #[wasm_bindgen_test]
  fn test_unit_constructor() {
    // Test the unit() static constructor
    let unit_val = WasmValue::unit();
    assert_eq!(unit_val.r#type(), WasmType::Unit);

    // Verify it returns UNDEFINED when extracted
    let extracted = unit_val.get();
    assert!(extracted.is_undefined());

    // Verify it's identical to creating via new()
    let unit_via_new = WasmValue::new(WasmType::Unit, JsValue::NULL).unwrap();
    assert_eq!(unit_val.r#type(), unit_via_new.r#type());
  }

  #[wasm_bindgen_test]
  fn test_primitive_values() {
    // Test Unit
    let unit_val = WasmValue::new(WasmType::Unit, JsValue::NULL).unwrap();
    assert_eq!(unit_val.r#type(), WasmType::Unit);

    // Test Boolean
    let bool_val = WasmValue::new(WasmType::Boolean, JsValue::from(true)).unwrap();
    assert_eq!(bool_val.r#type(), WasmType::Boolean);
    assert_eq!(bool_val.get().as_bool(), Some(true));

    // Test F64
    let f64_val = WasmValue::new(WasmType::F64, JsValue::from(3.14)).unwrap();
    assert_eq!(f64_val.r#type(), WasmType::F64);
    assert_eq!(f64_val.get().as_f64(), Some(3.14));

    // Test String
    let str_val = WasmValue::new(WasmType::String, JsValue::from("hello")).unwrap();
    assert_eq!(str_val.r#type(), WasmType::String);
    assert_eq!(str_val.get().as_string(), Some("hello".to_string()));
  }

  #[wasm_bindgen_test]
  fn test_integer_types() {
    // Test U8
    let u8_val = WasmValue::new(WasmType::U8, JsValue::from(255)).unwrap();
    assert_eq!(u8_val.r#type(), WasmType::U8);

    // Test I32
    let i32_val = WasmValue::new(WasmType::I32, JsValue::from(-42)).unwrap();
    assert_eq!(i32_val.r#type(), WasmType::I32);

    // Test out of range
    let result = WasmValue::new(WasmType::U8, JsValue::from(256));
    assert!(result.is_err());
  }

  #[wasm_bindgen_test]
  fn test_array_values() {
    // Test boolean array
    let arr = Array::new();
    arr.push(&JsValue::from(true));
    arr.push(&JsValue::from(false));
    let bool_arr = WasmValue::new(WasmType::ArrayBoolean, arr.into()).unwrap();
    assert_eq!(bool_arr.r#type(), WasmType::ArrayBoolean);

    // Test number array
    let arr = Array::new();
    arr.push(&JsValue::from(1.5));
    arr.push(&JsValue::from(2.5));
    let f64_arr = WasmValue::new(WasmType::ArrayF64, arr.into()).unwrap();
    assert_eq!(f64_arr.r#type(), WasmType::ArrayF64);
  }

  #[wasm_bindgen_test]
  fn test_from_auto_detection() {
    // Test boolean
    let val = WasmValue::from(JsValue::from(true)).unwrap();
    assert_eq!(val.r#type(), WasmType::Boolean);

    // Test number (defaults to f64)
    let val = WasmValue::from(JsValue::from(42.0)).unwrap();
    assert_eq!(val.r#type(), WasmType::F64);

    // Test string
    let val = WasmValue::from(JsValue::from("test")).unwrap();
    assert_eq!(val.r#type(), WasmType::String);

    // Test null - converts to Option(None) per js_to_value implementation
    let val = WasmValue::from(JsValue::NULL).unwrap();
    assert_eq!(val.r#type(), WasmType::Option);
  }

  #[wasm_bindgen_test]
  fn test_array_auto_detection() {
    // Test boolean array
    let arr = Array::new();
    arr.push(&JsValue::from(true));
    arr.push(&JsValue::from(false));
    let val = WasmValue::from(arr.into()).unwrap();
    assert_eq!(val.r#type(), WasmType::ArrayBoolean);

    // Test number array
    let arr = Array::new();
    arr.push(&JsValue::from(1.0));
    arr.push(&JsValue::from(2.0));
    let val = WasmValue::from(arr.into()).unwrap();
    assert_eq!(val.r#type(), WasmType::ArrayF64);

    // Test string array
    let arr = Array::new();
    arr.push(&JsValue::from("a"));
    arr.push(&JsValue::from("b"));
    let val = WasmValue::from(arr.into()).unwrap();
    assert_eq!(val.r#type(), WasmType::ArrayString);
  }

  #[wasm_bindgen_test]
  fn test_set_method() {
    let mut val = WasmValue::new(WasmType::F64, JsValue::from(1.0)).unwrap();
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
    let some_val = WasmValue::new(WasmType::Option, JsValue::from(42.0)).unwrap();
    assert_eq!(some_val.r#type(), WasmType::Option);

    // None value
    let none_val = WasmValue::new(WasmType::Option, JsValue::NULL).unwrap();
    assert_eq!(none_val.r#type(), WasmType::Option);
    assert!(none_val.get().is_null());
  }
}
