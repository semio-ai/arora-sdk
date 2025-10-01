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
  /// Create a KeyValue from a collection (slice, Vec, KeyValueSet) of KeyValueField where each
  /// field's own name is used as the key.
  pub fn make_kv_from_fields<F: AsRef<[KeyValueField]>>(fields: F) -> Value {
    Self::make_kv_from_fields_with_id(gen_bb_uuid(), fields)
  }

  /// Same as `make_kv_from_fields` but allows specifying the KeyValue id explicitly.
  pub fn make_kv_from_fields_with_id<F: AsRef<[KeyValueField]>>(kv_id: Uuid, fields: F) -> Value {
    let slice = fields.as_ref();
    let map: HashMap<String, KeyValueField> =
      slice.iter().cloned().map(|f| (f.name.clone(), f)).collect();
    Value::KeyValue(KeyValue {
      id: kv_id,
      fields: map,
    })
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

/// Wrapper type representing a collection of `KeyValueField`s. This allows us to
/// extend functionality (validation, ordering rules, etc.) without changing all
/// call sites that currently use `Vec<KeyValueField>` or slices.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KeyValueSet(pub Vec<KeyValueField>);

impl std::ops::Deref for KeyValueSet {
  type Target = [KeyValueField];
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl IntoIterator for KeyValueSet {
  type Item = KeyValueField;
  type IntoIter = std::vec::IntoIter<KeyValueField>;
  fn into_iter(self) -> Self::IntoIter {
    self.0.into_iter()
  }
}

impl From<Vec<KeyValueField>> for KeyValueSet {
  fn from(v: Vec<KeyValueField>) -> Self {
    KeyValueSet(v)
  }
}

impl<'a> From<&'a [KeyValueField]> for KeyValueSet {
  fn from(slice: &'a [KeyValueField]) -> Self {
    KeyValueSet(slice.to_vec())
  }
}

impl From<KeyValueSet> for Vec<KeyValueField> {
  fn from(kvs: KeyValueSet) -> Self {
    kvs.0
  }
}

impl KeyValueSet {
  pub fn new() -> Self {
    Self(Vec::new())
  }
  pub fn push(&mut self, field: KeyValueField) {
    self.0.push(field);
  }
  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }
  pub fn len(&self) -> usize {
    self.0.len()
  }
  pub fn into_inner(self) -> Vec<KeyValueField> {
    self.0
  }
  pub fn iter(&self) -> std::slice::Iter<'_, KeyValueField> {
    self.0.iter()
  }
  pub fn get(&self, name: &str) -> Option<&KeyValueField> {
    self.0.iter().find(|f| f.name == name)
  }

  pub fn from_hash<K>(pairs: impl IntoIterator<Item = (K, KeyValueField)>) -> KeyValueSet
  where
    K: Into<String>,
  {
    use std::collections::HashMap; // temporary map to de-duplicate by name
    let mut map: HashMap<String, KeyValueField> = HashMap::new();
    for (k, v) in pairs.into_iter() {
      map.insert(k.into(), v); // last wins
    }
    KeyValueSet(map.into_iter().map(|(_, v)| v).collect())
  }
}

impl AsRef<[KeyValueField]> for KeyValueSet {
  fn as_ref(&self) -> &[KeyValueField] {
    &self.0
  }
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

  pub fn new_nested_kv<S: Into<String>, F: AsRef<[KeyValueField]>>(kv_name: S, fields: &F) -> Self {
    Self::new_nested_kv_with_kv_id(kv_name, gen_bb_uuid(), fields)
  }

  pub fn new_nested_kv_with_kv_id<S: Into<String>, F: AsRef<[KeyValueField]>>(
    kv_name: S,
    kv_id: Uuid,
    fields: &F,
  ) -> Self {
    Self::new_nested_kv_with_both_ids(kv_name, gen_bb_uuid(), kv_id, fields)
  }

  pub fn new_nested_kv_with_both_ids<S: Into<String>, F: AsRef<[KeyValueField]>>(
    kv_name: S,
    field_id: Uuid,
    kv_id: Uuid,
    fields: &F,
  ) -> Self {
    KeyValueField::new_with_id(
      kv_name,
      field_id,
      KeyValue::make_kv_from_fields_with_id(kv_id, fields.as_ref()),
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

    // Directly set a field and value entry into the KV

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

    // Set an entry into the KV using a prebuilt key-value field

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
  fn test_keyvalue_field_simple_nested_keyvalue() {
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
  fn test_valueblock_make_kv_from_fields_simple() {
    let fields = vec![
      KeyValueField::new("health", Value::I32(100)),
      KeyValueField::new("mana", Value::I32(50)),
    ];
    let id = gen_bb_uuid();
    let kv = KeyValue::make_kv_from_fields_with_id(id, &fields);
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
  fn test_keyvalue_nested_structure_without_ids() {
    // Create a complex nested structure without caring for the ids, showing a clean and simple example
    let player_kv = KeyValue::make_kv_from_fields(&[
      KeyValueField::new("health", Value::I32(100)),
      KeyValueField::new_nested_kv(
        "stats",
        &KeyValueSet::from(vec![
          KeyValueField::new("strength", Value::I32(50)),
          KeyValueField::new("agility", Value::I32(75)),
        ]),
      ),
      KeyValueField::new_nested_kv(
        "position",
        &KeyValueSet::from(vec![
          KeyValueField::new("x", Value::F32(10.0)),
          KeyValueField::new("y", Value::F32(20.0)),
        ]),
      ),
    ]);

    match player_kv {
      Value::KeyValue(player) => {
        // Now expect three top-level fields: health, stats, position
        assert_eq!(player.fields.len(), 3);

        // health
        let health_field = player.get_field("health").expect("health field");
        match health_field.value.as_ref().unwrap().as_ref() {
          Value::I32(100) => {}
          other => panic!("Expected I32(100) got {:?}", other),
        }

        // stats nested kv
        let stats_field = player.get_field("stats").expect("stats field");
        match stats_field.value.as_ref().unwrap().as_ref() {
          Value::KeyValue(stats_kv) => {
            assert_eq!(stats_kv.fields.len(), 2);
            // strength
            match stats_kv
              .get_field("strength")
              .unwrap()
              .value
              .as_ref()
              .unwrap()
              .as_ref()
            {
              Value::I32(50) => {}
              other => panic!("Expected strength=50 got {:?}", other),
            }
            // agility
            match stats_kv
              .get_field("agility")
              .unwrap()
              .value
              .as_ref()
              .unwrap()
              .as_ref()
            {
              Value::I32(75) => {}
              other => panic!("Expected agility=75 got {:?}", other),
            }
          }
          other => panic!("Expected KeyValue for stats got {:?}", other),
        }

        // position nested kv
        let position_field = player.get_field("position").expect("position field");
        match position_field.value.as_ref().unwrap().as_ref() {
          Value::KeyValue(pos_kv) => {
            assert_eq!(pos_kv.fields.len(), 2);
            match pos_kv
              .get_field("x")
              .unwrap()
              .value
              .as_ref()
              .unwrap()
              .as_ref()
            {
              Value::F32(f) if (*f - 10.0).abs() < f32::EPSILON => {}
              other => panic!("Expected x=10.0 got {:?}", other),
            }
            match pos_kv
              .get_field("y")
              .unwrap()
              .value
              .as_ref()
              .unwrap()
              .as_ref()
            {
              Value::F32(f) if (*f - 20.0).abs() < f32::EPSILON => {}
              other => panic!("Expected y=20.0 got {:?}", other),
            }
          }
          other => panic!("Expected KeyValue for position got {:?}", other),
        }
      }
      _ => panic!("Expected KeyValue variant for player"),
    }
  }

  #[test]
  fn test_valueblock_make_kv_from_fields_duplicate_names_last_wins() {
    let fields = vec![
      KeyValueField::new("health", Value::I32(100)),
      KeyValueField::new("health", Value::I32(150)), // duplicate name
    ];
    let kv_value = KeyValue::make_kv_from_fields(&fields);
    match kv_value {
      Value::KeyValue(kv) => {
        assert_eq!(kv.fields.len(), 1); // last wins
        let health_field = kv.get_field("health").unwrap();
        match health_field.value.as_ref().unwrap().as_ref() {
          Value::I32(150) => {}
          other => panic!("Expected 150 got {:?}", other),
        }
      }
      _ => panic!("Expected KeyValue variant"),
    }
  }

  #[test]
  fn test_keyvalue_nested_structure_with_ids() {
    let outer_id = gen_bb_uuid();
    let health_id = gen_bb_uuid();
    let inner_field_id = gen_bb_uuid();
    let inner_kv_id = gen_bb_uuid();
    let strength_id = gen_bb_uuid();
    let agility_id = gen_bb_uuid();

    let stats_set = KeyValueSet::from(vec![
      KeyValueField::new_with_id("strength", strength_id, Value::I32(50)),
      KeyValueField::new_with_id("agility", agility_id, Value::I32(75)),
    ]);

    let player_value = KeyValue::make_kv_from_fields_with_id(
      outer_id,
      &[
        KeyValueField::new_with_id("health", health_id, Value::I32(100)),
        KeyValueField::new_nested_kv_with_both_ids(
          "stats",
          inner_field_id,
          inner_kv_id,
          &stats_set,
        ),
      ],
    );

    match player_value {
      Value::KeyValue(player) => {
        assert_eq!(player.id, outer_id);
        assert_eq!(player.fields.len(), 2);
        let health_field = player.get_field("health").unwrap();
        assert_eq!(health_field.id, health_id);
        match health_field.value.as_ref().unwrap().as_ref() {
          Value::I32(100) => {}
          _ => panic!("Expected I32(100)"),
        }
        let stats_field = player.get_field("stats").unwrap();
        assert_eq!(stats_field.id, inner_field_id);
        match stats_field.value.as_ref().unwrap().as_ref() {
          Value::KeyValue(stats_kv) => {
            assert_eq!(stats_kv.id, inner_kv_id);
            assert_eq!(stats_kv.fields.len(), 2);
            let strength_field = stats_kv.get_field("strength").unwrap();
            assert_eq!(strength_field.id, strength_id);
            match strength_field.value.as_ref().unwrap().as_ref() {
              Value::I32(50) => {}
              _ => panic!("Expected I32(50)"),
            }
            let agility_field = stats_kv.get_field("agility").unwrap();
            assert_eq!(agility_field.id, agility_id);
            match agility_field.value.as_ref().unwrap().as_ref() {
              Value::I32(75) => {}
              _ => panic!("Expected I32(75)"),
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

  #[test]
  fn test_keyvalue_field_map_from_hash() {
    let set = KeyValueSet::from_hash([
      ("a", KeyValueField::new("a", Value::I32(1))),
      ("b", KeyValueField::new("b", Value::I32(2))),
      ("a", KeyValueField::new("a", Value::I32(10))), // overwrite
    ]);
    assert_eq!(set.len(), 2);
    let a_field = set.get("a").expect("a field");
    match a_field.value.as_ref().unwrap().as_ref() {
      Value::I32(10) => {}
      other => panic!("expected 10 got {:?}", other),
    }
    let b_field = set.get("b").expect("b field");
    match b_field.value.as_ref().unwrap().as_ref() {
      Value::I32(2) => {}
      other => panic!("expected 2 got {:?}", other),
    }
  }
}
