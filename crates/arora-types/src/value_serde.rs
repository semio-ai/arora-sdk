//! Serde over [`Value`]: any `Serialize`/`Deserialize` type converts to and
//! from an arora [`Value`], no type declaration or code generation involved —
//! the [`serde_json::to_value`]-style bridge, with `Value` as the data model.
//!
//! - **structs** map to [`Structure`] — the type and field ids derive from
//!   their names via [`gen_uuid_from_str`]. The names are not stored:
//!   deserialization matches the stored ids against the hashes of the
//!   candidate field names serde provides, so the one-way hash round-trips;
//! - **enums** map to [`Enumeration`] — same scheme for the variant ids;
//! - **maps** (dynamic string keys, no declaration to hash against) map to
//!   [`KeyValue`], which carries the names;
//! - **sequences and tuples** map to [`Value::ArrayValue`], primitives to
//!   their `Value` twins, `Option` to [`Value::Option`], unit to
//!   [`Value::Unit`].
//!
//! [`to_value`]/[`from_value`] are the entry points. The bridge is host-side
//! convenience: the module ABI's declared type specs (code generation) are a
//! separate concern and unaffected.

use std::collections::HashMap;
use std::fmt::{self, Display};

use serde::de::{
  self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
  VariantAccess, Visitor,
};
use serde::ser::{self, Serialize};
use uuid::Uuid;

use crate::gen_uuid_from_str;
use crate::keyvalue::{KeyValue, KeyValueField};
use crate::value::{Enumeration, Structure, StructureField, Value};

/// A conversion between a Rust type and a [`Value`] failed.
#[derive(Debug)]
pub struct Error(String);

impl Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
  fn custom<T: Display>(msg: T) -> Self {
    Error(msg.to_string())
  }
}

impl de::Error for Error {
  fn custom<T: Display>(msg: T) -> Self {
    Error(msg.to_string())
  }
}

/// Convert any `Serialize` type into a [`Value`].
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, Error> {
  value.serialize(ValueSerializer)
}

/// Convert a [`Value`] back into any `Deserialize` type.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
  T::deserialize(ValueDeserializer(value))
}

// ---- serialization ---------------------------------------------------------

struct ValueSerializer;

fn structure_from(id: Uuid, fields: Vec<(String, Value)>) -> Value {
  Value::Structure(Structure {
    id,
    fields: fields
      .into_iter()
      .map(|(name, value)| StructureField {
        id: gen_uuid_from_str(&name),
        value: Box::new(value),
      })
      .collect(),
  })
}

fn keyvalue_from(id: Uuid, fields: Vec<(String, Value)>) -> Value {
  let mut kv = KeyValue::new_with_id(id);
  for (name, value) in fields {
    kv.fields.insert(
      name.clone(),
      KeyValueField {
        id: gen_uuid_from_str(&name),
        name,
        value: Some(Box::new(value)),
      },
    );
  }
  Value::KeyValue(kv)
}

/// The well-known id every serde-converted map travels under: maps carry
/// their keys by name, so no per-type id applies.
pub fn map_id() -> Uuid {
  gen_uuid_from_str("map")
}

fn enumeration_from(type_name: &str, variant: &str, value: Value) -> Value {
  Value::Enumeration(Enumeration {
    id: gen_uuid_from_str(type_name),
    variant_id: gen_uuid_from_str(variant),
    value: Box::new(value),
  })
}

impl ser::Serializer for ValueSerializer {
  type Ok = Value;
  type Error = Error;
  type SerializeSeq = SeqSerializer;
  type SerializeTuple = SeqSerializer;
  type SerializeTupleStruct = SeqSerializer;
  type SerializeTupleVariant = VariantSeqSerializer;
  type SerializeMap = MapSerializer;
  type SerializeStruct = StructSerializer;
  type SerializeStructVariant = VariantStructSerializer;

  fn serialize_bool(self, v: bool) -> Result<Value, Error> {
    Ok(Value::Boolean(v))
  }
  fn serialize_i8(self, v: i8) -> Result<Value, Error> {
    Ok(Value::I8(v))
  }
  fn serialize_i16(self, v: i16) -> Result<Value, Error> {
    Ok(Value::I16(v))
  }
  fn serialize_i32(self, v: i32) -> Result<Value, Error> {
    Ok(Value::I32(v))
  }
  fn serialize_i64(self, v: i64) -> Result<Value, Error> {
    Ok(Value::I64(v))
  }
  fn serialize_u8(self, v: u8) -> Result<Value, Error> {
    Ok(Value::U8(v))
  }
  fn serialize_u16(self, v: u16) -> Result<Value, Error> {
    Ok(Value::U16(v))
  }
  fn serialize_u32(self, v: u32) -> Result<Value, Error> {
    Ok(Value::U32(v))
  }
  fn serialize_u64(self, v: u64) -> Result<Value, Error> {
    Ok(Value::U64(v))
  }
  fn serialize_f32(self, v: f32) -> Result<Value, Error> {
    Ok(Value::F32(v))
  }
  fn serialize_f64(self, v: f64) -> Result<Value, Error> {
    Ok(Value::F64(v))
  }
  fn serialize_char(self, v: char) -> Result<Value, Error> {
    Ok(Value::String(v.to_string()))
  }
  fn serialize_str(self, v: &str) -> Result<Value, Error> {
    Ok(Value::String(v.to_string()))
  }
  fn serialize_bytes(self, v: &[u8]) -> Result<Value, Error> {
    Ok(Value::ArrayU8(v.to_vec()))
  }
  fn serialize_none(self) -> Result<Value, Error> {
    Ok(Value::Option(None))
  }
  fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, Error> {
    Ok(Value::Option(Some(Box::new(
      value.serialize(ValueSerializer)?,
    ))))
  }
  fn serialize_unit(self) -> Result<Value, Error> {
    Ok(Value::Unit)
  }
  fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, Error> {
    Ok(Value::Unit)
  }
  fn serialize_unit_variant(
    self,
    name: &'static str,
    _index: u32,
    variant: &'static str,
  ) -> Result<Value, Error> {
    Ok(enumeration_from(name, variant, Value::Unit))
  }
  fn serialize_newtype_struct<T: Serialize + ?Sized>(
    self,
    _name: &'static str,
    value: &T,
  ) -> Result<Value, Error> {
    value.serialize(ValueSerializer)
  }
  fn serialize_newtype_variant<T: Serialize + ?Sized>(
    self,
    name: &'static str,
    _index: u32,
    variant: &'static str,
    value: &T,
  ) -> Result<Value, Error> {
    Ok(enumeration_from(
      name,
      variant,
      value.serialize(ValueSerializer)?,
    ))
  }
  fn serialize_seq(self, len: Option<usize>) -> Result<SeqSerializer, Error> {
    Ok(SeqSerializer {
      items: Vec::with_capacity(len.unwrap_or(0)),
    })
  }
  fn serialize_tuple(self, len: usize) -> Result<SeqSerializer, Error> {
    self.serialize_seq(Some(len))
  }
  fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqSerializer, Error> {
    self.serialize_seq(Some(len))
  }
  fn serialize_tuple_variant(
    self,
    name: &'static str,
    _index: u32,
    variant: &'static str,
    len: usize,
  ) -> Result<VariantSeqSerializer, Error> {
    Ok(VariantSeqSerializer {
      type_name: name,
      variant,
      items: Vec::with_capacity(len),
    })
  }
  fn serialize_map(self, _len: Option<usize>) -> Result<MapSerializer, Error> {
    Ok(MapSerializer {
      key: None,
      fields: Vec::new(),
    })
  }
  fn serialize_struct(self, name: &'static str, len: usize) -> Result<StructSerializer, Error> {
    Ok(StructSerializer {
      type_name: name,
      fields: Vec::with_capacity(len),
    })
  }
  fn serialize_struct_variant(
    self,
    name: &'static str,
    _index: u32,
    variant: &'static str,
    len: usize,
  ) -> Result<VariantStructSerializer, Error> {
    Ok(VariantStructSerializer {
      type_name: name,
      variant,
      fields: Vec::with_capacity(len),
    })
  }
}

struct SeqSerializer {
  items: Vec<Value>,
}

impl ser::SerializeSeq for SeqSerializer {
  type Ok = Value;
  type Error = Error;
  fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
    self.items.push(value.serialize(ValueSerializer)?);
    Ok(())
  }
  fn end(self) -> Result<Value, Error> {
    Ok(Value::ArrayValue(self.items))
  }
}

impl ser::SerializeTuple for SeqSerializer {
  type Ok = Value;
  type Error = Error;
  fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
    ser::SerializeSeq::serialize_element(self, value)
  }
  fn end(self) -> Result<Value, Error> {
    ser::SerializeSeq::end(self)
  }
}

impl ser::SerializeTupleStruct for SeqSerializer {
  type Ok = Value;
  type Error = Error;
  fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
    ser::SerializeSeq::serialize_element(self, value)
  }
  fn end(self) -> Result<Value, Error> {
    ser::SerializeSeq::end(self)
  }
}

struct VariantSeqSerializer {
  type_name: &'static str,
  variant: &'static str,
  items: Vec<Value>,
}

impl ser::SerializeTupleVariant for VariantSeqSerializer {
  type Ok = Value;
  type Error = Error;
  fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
    self.items.push(value.serialize(ValueSerializer)?);
    Ok(())
  }
  fn end(self) -> Result<Value, Error> {
    Ok(enumeration_from(
      self.type_name,
      self.variant,
      Value::ArrayValue(self.items),
    ))
  }
}

struct MapSerializer {
  key: Option<String>,
  fields: Vec<(String, Value)>,
}

impl ser::SerializeMap for MapSerializer {
  type Ok = Value;
  type Error = Error;
  fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
    match key.serialize(ValueSerializer)? {
      Value::String(name) => {
        self.key = Some(name);
        Ok(())
      }
      other => Err(ser::Error::custom(format!(
        "map keys must be strings to become a KeyValue, got {other:?}"
      ))),
    }
  }
  fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
    let key = self
      .key
      .take()
      .ok_or_else(|| ser::Error::custom("map value without a key"))?;
    self.fields.push((key, value.serialize(ValueSerializer)?));
    Ok(())
  }
  fn end(mut self) -> Result<Value, Error> {
    // Deterministic encoding: entries sort by key, and the KeyValue takes the
    // well-known map id — so the same map always converts to the same Value
    // (and the same bytes, whichever serde backend produced them).
    self.fields.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(keyvalue_from(map_id(), self.fields))
  }
}

struct StructSerializer {
  type_name: &'static str,
  fields: Vec<(String, Value)>,
}

impl ser::SerializeStruct for StructSerializer {
  type Ok = Value;
  type Error = Error;
  fn serialize_field<T: Serialize + ?Sized>(
    &mut self,
    key: &'static str,
    value: &T,
  ) -> Result<(), Error> {
    self
      .fields
      .push((key.to_string(), value.serialize(ValueSerializer)?));
    Ok(())
  }
  fn end(self) -> Result<Value, Error> {
    Ok(structure_from(
      gen_uuid_from_str(self.type_name),
      self.fields,
    ))
  }
}

struct VariantStructSerializer {
  type_name: &'static str,
  variant: &'static str,
  fields: Vec<(String, Value)>,
}

impl ser::SerializeStructVariant for VariantStructSerializer {
  type Ok = Value;
  type Error = Error;
  fn serialize_field<T: Serialize + ?Sized>(
    &mut self,
    key: &'static str,
    value: &T,
  ) -> Result<(), Error> {
    self
      .fields
      .push((key.to_string(), value.serialize(ValueSerializer)?));
    Ok(())
  }
  fn end(self) -> Result<Value, Error> {
    let inner = structure_from(gen_uuid_from_str(self.variant), self.fields);
    Ok(enumeration_from(self.type_name, self.variant, inner))
  }
}

// ---- deserialization -------------------------------------------------------

struct ValueDeserializer(Value);

impl<'de> de::Deserializer<'de> for ValueDeserializer {
  type Error = Error;

  fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
    match self.0 {
      Value::Unit => visitor.visit_unit(),
      Value::Boolean(v) => visitor.visit_bool(v),
      Value::U8(v) => visitor.visit_u8(v),
      Value::U16(v) => visitor.visit_u16(v),
      Value::U32(v) => visitor.visit_u32(v),
      Value::U64(v) => visitor.visit_u64(v),
      Value::I8(v) => visitor.visit_i8(v),
      Value::I16(v) => visitor.visit_i16(v),
      Value::I32(v) => visitor.visit_i32(v),
      Value::I64(v) => visitor.visit_i64(v),
      Value::F32(v) => visitor.visit_f32(v),
      Value::F64(v) => visitor.visit_f64(v),
      Value::String(v) => visitor.visit_string(v),
      Value::Option(None) => visitor.visit_none(),
      Value::Option(Some(v)) => visitor.visit_some(ValueDeserializer(*v)),
      Value::ArrayValue(items) => visit_seq(items, visitor),
      Value::ArrayBoolean(items) => {
        visit_seq(items.into_iter().map(Value::Boolean).collect(), visitor)
      }
      Value::ArrayU8(items) => visit_seq(items.into_iter().map(Value::U8).collect(), visitor),
      Value::ArrayU16(items) => visit_seq(items.into_iter().map(Value::U16).collect(), visitor),
      Value::ArrayU32(items) => visit_seq(items.into_iter().map(Value::U32).collect(), visitor),
      Value::ArrayU64(items) => visit_seq(items.into_iter().map(Value::U64).collect(), visitor),
      Value::ArrayI8(items) => visit_seq(items.into_iter().map(Value::I8).collect(), visitor),
      Value::ArrayI16(items) => visit_seq(items.into_iter().map(Value::I16).collect(), visitor),
      Value::ArrayI32(items) => visit_seq(items.into_iter().map(Value::I32).collect(), visitor),
      Value::ArrayI64(items) => visit_seq(items.into_iter().map(Value::I64).collect(), visitor),
      Value::ArrayF32(items) => visit_seq(items.into_iter().map(Value::F32).collect(), visitor),
      Value::ArrayF64(items) => visit_seq(items.into_iter().map(Value::F64).collect(), visitor),
      Value::ArrayString(items) => {
        visit_seq(items.into_iter().map(Value::String).collect(), visitor)
      }
      Value::KeyValue(kv) => visit_keyvalue(kv, visitor),
      Value::Uuid(v) => visitor.visit_string(v.to_string()),
      other @ (Value::Structure(_)
      | Value::Enumeration(_)
      | Value::ArrayStructure { .. }
      | Value::ArrayEnumeration { .. }) => Err(de::Error::custom(format!(
        "cannot deserialize from {other:?} without its type declaration"
      ))),
    }
  }

  fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
    match self.0 {
      Value::Option(None) | Value::Unit => visitor.visit_none(),
      Value::Option(Some(v)) => visitor.visit_some(ValueDeserializer(*v)),
      other => visitor.visit_some(ValueDeserializer(other)),
    }
  }

  fn deserialize_enum<V: Visitor<'de>>(
    self,
    _name: &'static str,
    variants: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error> {
    match self.0 {
      Value::Enumeration(e) => {
        let variant = variants
          .iter()
          .find(|candidate| gen_uuid_from_str(candidate) == e.variant_id)
          .ok_or_else(|| {
            de::Error::custom(format!(
              "variant id {} matches none of {variants:?}",
              e.variant_id
            ))
          })?;
        visitor.visit_enum(EnumDeserializer {
          variant,
          value: *e.value,
        })
      }
      // A bare string names a unit variant, as in serde's self-describing
      // formats.
      Value::String(name) => visitor.visit_enum(name.into_deserializer()),
      other => Err(de::Error::custom(format!(
        "expected an enumeration, got {other:?}"
      ))),
    }
  }

  fn deserialize_newtype_struct<V: Visitor<'de>>(
    self,
    _name: &'static str,
    visitor: V,
  ) -> Result<V::Value, Error> {
    visitor.visit_newtype_struct(self)
  }

  fn deserialize_struct<V: Visitor<'de>>(
    self,
    _name: &'static str,
    fields: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error> {
    match self.0 {
      Value::Structure(structure) => visit_structure(structure, fields, visitor),
      // A KeyValue carries its names directly; accept it for a struct too.
      Value::KeyValue(kv) => visit_keyvalue(kv, visitor),
      other => Err(de::Error::custom(format!(
        "expected a structure, got {other:?}"
      ))),
    }
  }

  serde::forward_to_deserialize_any! {
      bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
      bytes byte_buf unit unit_struct seq tuple tuple_struct map
      identifier ignored_any
  }
}

fn visit_seq<'de, V: Visitor<'de>>(items: Vec<Value>, visitor: V) -> Result<V::Value, Error> {
  visitor.visit_seq(SeqDeserializer {
    iter: items.into_iter(),
  })
}

/// Drive a visitor over a [`Structure`], resolving each stored field id back
/// to its declared name by hashing the candidates serde provides. Fields with
/// ids outside the declaration are skipped, like unknown fields elsewhere.
fn visit_structure<'de, V: Visitor<'de>>(
  structure: Structure,
  fields: &'static [&'static str],
  visitor: V,
) -> Result<V::Value, Error> {
  let mut named = Vec::with_capacity(structure.fields.len());
  for field in structure.fields {
    if let Some(name) = fields
      .iter()
      .find(|candidate| gen_uuid_from_str(candidate) == field.id)
    {
      named.push((*name, *field.value));
    }
  }
  visitor.visit_map(StructDeserializer {
    iter: named.into_iter(),
    value: None,
  })
}

fn visit_keyvalue<'de, V: Visitor<'de>>(kv: KeyValue, visitor: V) -> Result<V::Value, Error> {
  let fields: HashMap<String, Option<Box<Value>>> = kv
    .fields
    .into_iter()
    .map(|(name, field)| (name, field.value))
    .collect();
  visitor.visit_map(MapDeserializer {
    iter: fields.into_iter(),
    value: None,
  })
}

struct SeqDeserializer {
  iter: std::vec::IntoIter<Value>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer {
  type Error = Error;
  fn next_element_seed<T: DeserializeSeed<'de>>(
    &mut self,
    seed: T,
  ) -> Result<Option<T::Value>, Error> {
    match self.iter.next() {
      Some(value) => seed.deserialize(ValueDeserializer(value)).map(Some),
      None => Ok(None),
    }
  }
  fn size_hint(&self) -> Option<usize> {
    Some(self.iter.len())
  }
}

struct MapDeserializer {
  iter: std::collections::hash_map::IntoIter<String, Option<Box<Value>>>,
  value: Option<Value>,
}

impl<'de> MapAccess<'de> for MapDeserializer {
  type Error = Error;
  fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Error> {
    match self.iter.next() {
      Some((name, value)) => {
        self.value = Some(match value {
          Some(v) => *v,
          None => Value::Option(None),
        });
        seed.deserialize(name.into_deserializer()).map(Some)
      }
      None => Ok(None),
    }
  }
  fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
    let value = self
      .value
      .take()
      .ok_or_else(|| de::Error::custom("map value without a key"))?;
    seed.deserialize(ValueDeserializer(value))
  }
}

struct StructDeserializer {
  iter: std::vec::IntoIter<(&'static str, Value)>,
  value: Option<Value>,
}

impl<'de> MapAccess<'de> for StructDeserializer {
  type Error = Error;
  fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Error> {
    match self.iter.next() {
      Some((name, value)) => {
        self.value = Some(value);
        seed.deserialize(name.into_deserializer()).map(Some)
      }
      None => Ok(None),
    }
  }
  fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
    let value = self
      .value
      .take()
      .ok_or_else(|| de::Error::custom("struct value without a field"))?;
    seed.deserialize(ValueDeserializer(value))
  }
}

struct EnumDeserializer {
  variant: &'static str,
  value: Value,
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
  type Error = Error;
  type Variant = VariantDeserializer;
  fn variant_seed<V: DeserializeSeed<'de>>(
    self,
    seed: V,
  ) -> Result<(V::Value, VariantDeserializer), Error> {
    let variant = seed.deserialize(self.variant.into_deserializer())?;
    Ok((variant, VariantDeserializer { value: self.value }))
  }
}

struct VariantDeserializer {
  value: Value,
}

impl<'de> VariantAccess<'de> for VariantDeserializer {
  type Error = Error;
  fn unit_variant(self) -> Result<(), Error> {
    match self.value {
      Value::Unit => Ok(()),
      other => Err(de::Error::custom(format!(
        "expected a unit variant payload, got {other:?}"
      ))),
    }
  }
  fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
    seed.deserialize(ValueDeserializer(self.value))
  }
  fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
    match self.value {
      Value::ArrayValue(items) => visit_seq(items, visitor),
      other => Err(de::Error::custom(format!(
        "expected a tuple variant payload, got {other:?}"
      ))),
    }
  }
  fn struct_variant<V: Visitor<'de>>(
    self,
    fields: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error> {
    match self.value {
      Value::Structure(structure) => visit_structure(structure, fields, visitor),
      Value::KeyValue(kv) => visit_keyvalue(kv, visitor),
      other => Err(de::Error::custom(format!(
        "expected a struct variant payload, got {other:?}"
      ))),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde::{Deserialize, Serialize};

  #[derive(Serialize, Deserialize, Debug, PartialEq)]
  enum Shape {
    Empty,
    Circle(f32),
    Segment(f32, f32),
    Box { width: f32, height: f32 },
  }

  #[derive(Serialize, Deserialize, Debug, PartialEq)]
  struct Sample {
    name: String,
    weight: f64,
    tags: Vec<String>,
    lucky: Option<u32>,
    unlucky: Option<u32>,
    shape: Shape,
    pairs: HashMap<String, i32>,
    ratio: (u8, u8),
  }

  fn sample() -> Sample {
    Sample {
      name: "arora".to_string(),
      weight: 0.5,
      tags: vec!["a".to_string(), "b".to_string()],
      lucky: Some(7),
      unlucky: None,
      shape: Shape::Box {
        width: 2.0,
        height: 3.0,
      },
      pairs: HashMap::from([("x".to_string(), 1), ("y".to_string(), -1)]),
      ratio: (3, 4),
    }
  }

  #[test]
  fn round_trips_a_nested_struct() {
    let value = to_value(&sample()).unwrap();
    // Structs travel as Structure — ids derive from the declared names, and
    // deserialization resolves them against the candidates serde provides.
    assert!(matches!(value, Value::Structure(_)));
    let back: Sample = from_value(value).unwrap();
    assert_eq!(back, sample());
  }

  #[test]
  fn maps_travel_as_keyvalue() {
    let pairs = HashMap::from([("x".to_string(), 1i32), ("y".to_string(), -1i32)]);
    let value = to_value(&pairs).unwrap();
    // Dynamic keys have no declaration to hash against, so the names ride
    // along in a KeyValue.
    assert!(matches!(value, Value::KeyValue(_)));
    let back: HashMap<String, i32> = from_value(value).unwrap();
    assert_eq!(back, pairs);
  }

  #[test]
  fn round_trips_every_enum_shape() {
    for shape in [
      Shape::Empty,
      Shape::Circle(1.5),
      Shape::Segment(0.0, 2.0),
      Shape::Box {
        width: 1.0,
        height: 2.0,
      },
    ] {
      let value = to_value(&shape).unwrap();
      // Enums travel as Enumeration; the variant id derives from the name and
      // deserialization matches it against the candidates serde provides.
      assert!(matches!(value, Value::Enumeration(_)));
      let back: Shape = from_value(value).unwrap();
      assert_eq!(back, shape);
    }
  }

  #[test]
  fn typed_arrays_deserialize_as_sequences() {
    let back: Vec<f32> = from_value(Value::ArrayF32(vec![1.0, 2.0])).unwrap();
    assert_eq!(back, vec![1.0, 2.0]);
  }

  #[test]
  fn an_unknown_variant_id_is_an_error() {
    let value = Value::Enumeration(Enumeration {
      id: gen_uuid_from_str("Shape"),
      variant_id: gen_uuid_from_str("NotAVariant"),
      value: Box::new(Value::Unit),
    });
    assert!(from_value::<Shape>(value).is_err());
  }
}
