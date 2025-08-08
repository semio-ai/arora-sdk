use crate::value::Value;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::gen_bb_uuid;

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("KV({:?})", fields)]
pub struct KeyValue {
  pub id: Uuid,
  pub fields: HashMap<String, KeyValueField>,
}

impl KeyValue {
  pub fn new() -> Self {
    KeyValue::new_with_id(gen_bb_uuid())
  }

  pub fn new_with_id(id: Uuid) -> Self {
    Self {
      id,
      fields: HashMap::new(),
    }
  }

  pub fn set_field(&mut self, field: KeyValueField) {
    self.fields.insert(field.name.clone(), field);
  }

  pub fn set_field_value(&mut self, key: &str, value: Value) {
    let key_str = key.to_string();
    if let Some(existing_field) = self.fields.get_mut(&key_str) {
      // Update the value of the existing field
      existing_field.value = Some(Box::new(value));
    } else {
      // Create a new field if it doesn't exist
      let field = KeyValueField::new(key_str.clone(), value);
      self.fields.insert(key_str, field);
    }
  }

  pub fn get_fields(&self) -> &HashMap<String, KeyValueField> {
    &self.fields
  }

  pub fn get_field_keys(&self) -> Vec<String> {
    self.fields.keys().cloned().collect()
  }

  pub fn get_field(&self, key: &str) -> Option<&KeyValueField> {
    self.fields.get(&key.to_string())
  }

  pub fn make_kv_from_pairs<K: Into<String> + Clone>(
    kv_id: Uuid,
    pairs: &[(K, KeyValueField)],
  ) -> Value {
    let fields: HashMap<String, KeyValueField> = pairs
      .iter()
      .map(|(k, v)| (k.clone().into(), v.clone()))
      .collect();
    Value::KeyValue(KeyValue { id: kv_id, fields })
  }

  pub fn make_kv_from_hash(fields: HashMap<String, KeyValueField>) -> Value {
    Self::make_kv_from_hash_with_id(gen_bb_uuid(), fields)
  }

  pub fn make_kv_from_hash_with_id(id: Uuid, fields: HashMap<String, KeyValueField>) -> Value {
    KeyValue { id, fields }.as_value()
  }

  pub fn as_value(self) -> Value {
    Value::KeyValue(self)
  }
}

#[derive(Debug, Clone, Display, Serialize, Deserialize, PartialEq)]
#[display("{} ({}): {:?}", name, id, value)]
pub struct KeyValueField {
  pub id: Uuid,
  pub name: String,
  pub value: Option<Box<Value>>,
}

impl KeyValueField {
  pub fn new<S: Into<String>>(name: S, value: Value) -> Self {
    Self::new_with_option(name, Some(value))
  }

  pub fn new_with_option<S: Into<String>>(name: S, value: Option<Value>) -> Self {
    Self::new_with_id_and_option(name, gen_bb_uuid(), value)
  }

  pub fn new_with_id<S: Into<String>>(name: S, id: Uuid, value: Value) -> Self {
    Self {
      name: name.into(),
      id,
      value: Some(Box::new(value)),
    }
  }

  pub fn new_with_id_and_option<S: Into<String>>(name: S, id: Uuid, value: Option<Value>) -> Self {
    Self {
      name: name.into(),
      id,
      value: value.map(Box::new),
    }
  }

  pub fn new_nested_kv<S: Into<String>, K: Into<String> + Clone>(
    kv_name: S,
    kv_id: Uuid,
    pairs: &[(K, KeyValueField)],
  ) -> Self {
    KeyValueField::new(kv_name, KeyValue::make_kv_from_pairs(kv_id, pairs))
  }

  pub fn new_nested_kv_with_id<S: Into<String>, K: Into<String> + Clone>(
    kv_name: S,
    field_id: Uuid,
    kv_id: Uuid,
    pairs: &[(K, KeyValueField)],
  ) -> Self {
    KeyValueField::new_with_id(
      kv_name,
      field_id,
      KeyValue::make_kv_from_pairs(kv_id, pairs),
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::value::Value;

  #[test]
  fn test_keyvalue_new() {
    let uuid = gen_bb_uuid();
    let kv = KeyValue::new_with_id(uuid);
    assert_eq!(kv.id, uuid);
    assert!(kv.fields.is_empty());
  }

  #[test]
  fn test_keyvalue_set_field_value_new() {
    let mut kv = KeyValue::new();
    let health_value = Value::I32(100);

    kv.set_field_value("health", health_value.clone());

    assert_eq!(kv.fields.len(), 1);
    assert!(kv.fields.contains_key("health"));

    let field = kv.get_field("health").unwrap();
    match field.value.as_ref().unwrap().as_ref() {
      Value::I32(value) => assert_eq!(*value, 100),
      _ => panic!("Expected I32 value"),
    }
  }

  #[test]
  fn test_keyvalue_set_field_value_update_existing() {
    let mut kv = KeyValue::new();

    // Set initial value
    kv.set_field_value("health", Value::I32(100));

    // Update existing field
    kv.set_field_value("health", Value::I32(50));

    assert_eq!(kv.fields.len(), 1);
    let field = kv.get_field("health").unwrap();
    match field.value.as_ref().unwrap().as_ref() {
      Value::I32(value) => assert_eq!(*value, 50),
      _ => panic!("Expected I32 value"),
    }
  }

  #[test]
  fn test_keyvalue_set_field() {
    let mut kv = KeyValue::new();
    let field = KeyValueField::new("health_id", Value::I32(100));

    kv.set_field(field.clone());

    assert_eq!(kv.fields.len(), 1);
    assert!(kv.fields.contains_key("health_id"));
    assert_eq!(kv.get_field("health_id"), Some(&field));
  }

  #[test]
  fn test_keyvalue_get_field_keys() {
    let mut kv = KeyValue::new();
    kv.set_field_value("health", Value::I32(100));
    kv.set_field_value("mana", Value::I32(50));
    kv.set_field_value("level", Value::I32(5));

    let keys = kv.get_field_keys();
    assert_eq!(keys.len(), 3);
    assert!(keys.contains(&"health".to_string()));
    assert!(keys.contains(&"mana".to_string()));
    assert!(keys.contains(&"level".to_string()));
  }

  #[test]
  fn test_keyvalue_get_field_nonexistent() {
    let kv = KeyValue::new();
    assert_eq!(kv.get_field("nonexistent"), None);
  }

  #[test]
  fn test_keyvalue_field_new() {
    let field = KeyValueField::new("test_id", Value::String("test_value".to_string()));
    assert_eq!(field.name, "test_id");
    match field.value.as_ref().unwrap().as_ref() {
      Value::String(value) => assert_eq!(value, "test_value"),
      _ => panic!("Expected String value"),
    }
  }

  #[test]
  fn test_keyvalue_field_new_with_simple_nested_keyvalue() {
    let id = gen_bb_uuid();
    let inner_kv = KeyValue::new_with_id(id);
    let field = KeyValueField::new("test_id", inner_kv.as_value());

    assert_eq!(field.name, "test_id");
    match field.value.as_ref().unwrap().as_ref() {
      Value::KeyValue(kv) => assert_eq!(kv.id, id),
      _ => panic!("Expected KeyValue variant"),
    }
  }

  #[test]
  fn test_keyvalue_field_new_nested_kv() {
    let nested_id = gen_bb_uuid();
    let stats_field = KeyValueField::new_nested_kv(
      "stats",
      nested_id,
      &[
        ("strength", KeyValueField::new("str_id", Value::I32(50))),
        ("agility", KeyValueField::new("agi_id", Value::I32(75))),
      ],
    );

    assert_eq!(stats_field.name, "stats");
    match stats_field.value.as_ref().unwrap().as_ref() {
      Value::KeyValue(kv) => {
        assert_eq!(kv.id, nested_id);
        assert_eq!(kv.fields.len(), 2);
        assert!(kv.fields.contains_key("strength"));
        assert!(kv.fields.contains_key("agility"));
      }
      _ => panic!("Expected KeyValue variant"),
    }
  }

  #[test]
  fn test_valueblock_make_kv_from_hash() {
    let mut fields = HashMap::new();
    fields.insert(
      "test_key".to_string(),
      KeyValueField::new("field_id", Value::I32(100)),
    );
    let id = gen_bb_uuid();
    let kv = KeyValue::make_kv_from_hash_with_id(id, fields.clone());

    match kv {
      Value::KeyValue(kv) => {
        assert_eq!(kv.id, id);
        assert_eq!(kv.fields, fields);
      }
      _ => panic!("Expected KeyValue variant"),
    }
  }

  #[test]
  fn test_valueblock_make_kv_from_pairs() {
    let pairs = &[
      ("health", KeyValueField::new("health_id", Value::I32(100))),
      ("mana", KeyValueField::new("mana_id", Value::I32(50))),
    ];

    let id = gen_bb_uuid();
    let kv = KeyValue::make_kv_from_pairs(id, pairs);

    match kv {
      Value::KeyValue(kv) => {
        assert_eq!(kv.id, id);
        assert_eq!(kv.fields.len(), 2);
        assert!(kv.fields.contains_key("health"));
        assert!(kv.fields.contains_key("mana"));
      }
      _ => panic!("Expected KeyValue variant"),
    }
  }

  #[test]
  fn test_keyvalue_complex_nested_structure() {
    // Create a complex nested structure similar to the blackboard test
    let outer_id = gen_bb_uuid();
    let health_id = gen_bb_uuid();
    let inner_id = gen_bb_uuid();
    let inner_kv_id = gen_bb_uuid();
    let strength_id = gen_bb_uuid();
    let agility_id = gen_bb_uuid();
    let player_kv = KeyValue::make_kv_from_pairs(
      outer_id,
      &[
        (
          "health",
          KeyValueField::new_with_id("health", health_id, Value::I32(100)),
        ),
        (
          "stats",
          KeyValueField::new_nested_kv_with_id(
            "stats",
            inner_id,
            inner_kv_id,
            &[
              (
                "strength",
                KeyValueField::new_with_id("strength", strength_id, Value::I32(50)),
              ),
              (
                "agility",
                KeyValueField::new_with_id("agility", agility_id, Value::I32(75)),
              ),
            ],
          ),
        ),
      ],
    );

    match player_kv {
      Value::KeyValue(player) => {
        assert_eq!(player.id, outer_id);
        assert_eq!(player.fields.len(), 2);

        // Test health field
        let health_field = player.get_field("health").unwrap();
        assert_eq!(health_field.id, health_id);
        assert_eq!(health_field.name, "health");
        match health_field.value.as_ref().unwrap().as_ref() {
          Value::I32(100) => {}
          _ => panic!("Expected I32(100) for health"),
        }

        // Test nested stats structure
        let stats_field = player.get_field("stats").unwrap();
        assert_eq!(stats_field.name, "stats");
        assert_eq!(stats_field.id, inner_id);
        match stats_field.value.as_ref().unwrap().as_ref() {
          Value::KeyValue(stats_kv) => {
            assert_eq!(stats_kv.id, inner_kv_id);
            assert_eq!(stats_kv.fields.len(), 2);

            // Test strength
            let strength_field = stats_kv.get_field("strength").unwrap();
            assert_eq!(strength_field.id, strength_id);
            assert_eq!(strength_field.name, "strength");
            match strength_field.value.as_ref().unwrap().as_ref() {
              Value::I32(50) => {}
              _ => panic!("Expected I32(50) for strength"),
            }

            // Test agility
            let agility_field = stats_kv.get_field("agility").unwrap();
            assert_eq!(agility_field.id, agility_id);
            assert_eq!(agility_field.name, "agility");
            match agility_field.value.as_ref().unwrap().as_ref() {
              Value::I32(75) => {}
              _ => panic!("Expected I32(75) for agility"),
            }
          }
          _ => panic!("Expected KeyValue for stats"),
        }
      }
      _ => panic!("Expected KeyValue variant for player"),
    }
  }

  #[test]
  fn test_keyvalue_display() {
    let mut kv = KeyValue::new();
    kv.set_field_value("key1", Value::String("value1".to_string()));
    kv.set_field_value("key2", Value::I32(42));

    let display_str = format!("{}", kv);
    assert!(display_str.contains("KV("));
    assert!(display_str.contains("key1"));
    assert!(display_str.contains("key2"));
  }

  #[test]
  fn test_keyvalue_field_display() {
    let field = KeyValueField::new("test_field", Value::String("test_value".to_string()));
    let display_str = format!("{}", field);
    assert!(display_str.contains("test_field"));
    assert!(display_str.contains("test_value"));
  }

  #[test]
  fn test_keyvalue_clone_and_equality() {
    let mut original = KeyValue::new();
    original.set_field_value("health", Value::I32(100));
    original.set_field_value("level", Value::I32(5));

    let cloned = original.clone();

    assert_eq!(original, cloned);
    assert_eq!(original.id, cloned.id);
    assert_eq!(original.fields.len(), cloned.fields.len());

    // Verify they're actually separate instances
    let mut modified = cloned;
    modified.set_field_value("health", Value::I32(200));

    assert_ne!(original, modified);
  }

  #[test]
  fn test_keyvalue_serialization() {
    use json5;

    let id = gen_bb_uuid();
    let mut kv = KeyValue::new_with_id(id);
    kv.set_field_value("health", Value::I32(100));
    kv.set_field_value("name", Value::String("Hero".to_string()));

    // Test serialization
    let json = json5::to_string(&kv).expect("Serialization should succeed");
    assert!(json.contains(&id.to_string()));
    assert!(json.contains("health"));
    assert!(json.contains("name"));

    // Test deserialization
    let deserialized: KeyValue = json5::from_str(&json).expect("Deserialization should succeed");
    assert_eq!(kv, deserialized);
  }

  #[test]
  fn test_keyvalue_empty_operations() {
    let kv = KeyValue::new();

    assert!(kv.get_field_keys().is_empty());
    assert!(kv.get_fields().is_empty());
    assert_eq!(kv.get_field("any_key"), None);
  }

  #[test]
  fn test_valueblock_with_different_value_types() {
    // Test with different primitive types
    let test_cases = vec![
      ("bool", Value::Boolean(true)),
      ("u8", Value::U8(255)),
      ("u16", Value::U16(65535)),
      ("u32", Value::U32(4294967295)),
      ("u64", Value::U64(18446744073709551615)),
      ("i8", Value::I8(-128)),
      ("i16", Value::I16(-32768)),
      ("i32", Value::I32(-2147483648)),
      ("i64", Value::I64(-9223372036854775808)),
      ("f32", Value::F32(3.14159)),
      ("f64", Value::F64(2.718281828)),
      ("string", Value::String("test string".to_string())),
      ("unit", Value::Unit),
    ];

    for (name, value) in test_cases {
      let mut kv = KeyValue::new();
      kv.set_field_value(name, value.clone());

      let retrieved_field = kv.get_field(name).unwrap();

      assert_eq!(
        retrieved_field.value.as_ref().unwrap().as_ref(),
        &value,
        "Failed for type: {}",
        name
      );
    }
  }
}
