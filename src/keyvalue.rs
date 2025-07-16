// These definitions may move into arora-schema later

use crate::value::Value;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::generate_bb_id;

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("{}", match self { ValueBlock::Value(v) => format!("{}", v), ValueBlock::KeyValue(kv) => format!("{}", kv) , ValueBlock::None => "ValueBlock::None".to_string() })]
pub enum ValueBlock {
  Value(Value),
  KeyValue(KeyValue),
  None,
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("KV({:?})", fields)]
pub struct KeyValue {
  pub id: String,
  pub fields: HashMap<String, KeyValueField>,
}

impl KeyValue {
  pub fn new<S: Into<String>>(id: S) -> Self {
    KeyValue {
      id: id.into(),
      fields: HashMap::new(),
    }
  }

  pub fn set_field(&mut self, field: KeyValueField) {
    self.fields.insert(field.id.clone(), field);
  }

  pub fn set_field_value<S: Into<String>>(&mut self, key: S, value: Value) {
    let key_str = key.into();
    if let Some(existing_field) = self.fields.get_mut(&key_str) {
      // Update the value of the existing field
      existing_field.value = Box::new(ValueBlock::Value(value));
    } else {
      // Create a new field if it doesn't exist
      let field = KeyValueField::new(generate_bb_id(), value);
      self.fields.insert(key_str, field);
    }
  }

  pub fn get_fields(&self) -> &HashMap<String, KeyValueField> {
    &self.fields
  }

  pub fn get_field_keys(&self) -> Vec<String> {
    self.fields.keys().cloned().collect()
  }

  pub fn get_field<S: Into<String>>(&self, key: S) -> Option<&KeyValueField> {
    self.fields.get(&key.into())
  }
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("{}: {}", id, value)]
pub struct KeyValueField {
  pub id: String,
  pub value: Box<ValueBlock>,
}

// Enum for supporting either Value or a KeyValue block as input to set()
impl From<Value> for ValueBlock {
  fn from(value: Value) -> Self {
    ValueBlock::Value(value)
  }
}

impl From<KeyValue> for ValueBlock {
  fn from(kv: KeyValue) -> Self {
    ValueBlock::KeyValue(kv)
  }
}

impl ValueBlock {
  pub fn make_kv_from_hash(id: String, fields: HashMap<String, KeyValueField>) -> Self {
    ValueBlock::KeyValue(KeyValue { id, fields })
  }

  // Convenience function to create a KeyValue block from a vector of pairs
  pub fn make_kv_from_pairs<S: Into<String> + Clone, K: Into<String> + Clone>(
    kv_id: S,
    pairs: &[(K, KeyValueField)],
  ) -> Self {
    let fields: HashMap<String, KeyValueField> = pairs
      .iter()
      .map(|(k, v)| (k.clone().into(), v.clone()))
      .collect();
    ValueBlock::KeyValue(KeyValue {
      id: kv_id.into(),
      fields,
    })
  }
}

impl KeyValueField {
  pub fn new<S: Into<String>>(id: S, value: impl Into<ValueBlock>) -> Self {
    KeyValueField {
      id: id.into(),
      value: Box::new(value.into()),
    }
  }

  pub fn new_nested_kv<S1: Into<String> + Clone, K: Into<String> + Clone>(
    kv_id: S1,
    pairs: &[(K, KeyValueField)],
  ) -> Self {
    KeyValueField {
      id: kv_id.clone().into(),
      value: Box::new(ValueBlock::make_kv_from_pairs(kv_id, pairs)),
    }
  }
}
