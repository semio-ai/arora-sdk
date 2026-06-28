//! The Arora data vocabulary: [`Key`], [`State`], [`StateChange`].
//!
//! This is the shared blackboard vocabulary that the HAL, the bridge, and
//! execution engines (behavior tree, modules) all agree on. It was lifted from
//! `studio-bridge`'s `msgs::state` so that those consumers can depend on it
//! without pulling the bridge in. Keep it additive-only — `Key`'s serde
//! representation is also the on-the-wire format (see the migration plan, D6).

use std::{
  borrow::Borrow,
  collections::{HashMap, HashSet},
  hash::Hash,
  str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::value::Value;

impl From<(String, Option<Value>)> for StateChange {
  fn from((key, value): (String, Option<Value>)) -> Self {
    Self {
      set: HashMap::from([(Key { path: key }, value)]),
      unset: HashSet::new(),
    }
  }
}

/// A collection of keys with their value associated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
  pub storage: HashMap<Key, Option<Value>>,
}

impl Default for State {
  fn default() -> Self {
    Self::new()
  }
}

impl State {
  pub fn new() -> Self {
    State {
      storage: HashMap::new(),
    }
  }

  /// Sets some value to a given key.
  pub fn set<K: Into<Key>>(&mut self, key: K, value: Option<Value>) {
    self.storage.insert(key.into(), value);
  }

  /// Unsets value at the given key.
  pub fn unset(&mut self, key: &Key) {
    self.storage.remove(key);
  }

  pub fn get(&self, key: &Key) -> Option<&Option<Value>> {
    self.storage.get(key)
  }

  pub fn iter(&self) -> impl Iterator<Item = (&Key, &Option<Value>)> {
    self.storage.iter()
  }

  pub fn evaluate_as_bool(&self, key: &Key) -> bool {
    self
      .get(key)
      .map(|v: &Option<Value>| match v {
        Some(Value::Boolean(b)) => *b,
        _ => false,
      })
      .unwrap_or(false)
  }

  pub fn is_empty(&self) -> bool {
    self.storage.is_empty()
  }

  /// Applies the given changes to the state.
  pub fn apply<C>(&mut self, changes: C)
  where
    C: Into<StateChange>,
  {
    let changes: StateChange = changes.into();
    for (key, value) in changes.set {
      self.storage.insert(key, value);
    }
    for key in changes.unset {
      self.storage.remove(&key);
    }
  }
}

impl IntoIterator for State {
  type Item = (Key, Option<Value>);
  type IntoIter = <HashMap<Key, Option<Value>> as IntoIterator>::IntoIter;

  fn into_iter(self) -> Self::IntoIter {
    self.storage.into_iter()
  }
}

/// Path to a variable in a state.
///
/// It is composed of a first set of segments separated by slashes ('/'),
/// determining the namespaces and the entity identifier, followed by a second
/// set of segments separated by dots ('.'), determining the attribute to access
/// on the entity. The entity identifier is the only mandatory segment.
///
/// Only alphanumeric characters, underscores and emojis are allowed.
///
/// Examples:
/// - `"robot1/joint1.position"` → namespace `["robot1"]`, entity `"joint1"`, attributes `["position"]`
/// - `"self/battery_level"` → namespace `["self"]`, entity `"battery_level"`, attributes `[]`
/// - `"camera_front.resolution.width"` → namespace `[]`, entity `"camera_front"`, attributes `["resolution", "width"]`
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct Key {
  pub path: String,
}

impl Key {
  pub fn new<S: Into<String>>(path: S) -> Self {
    Key { path: path.into() }
  }

  pub fn get_path(&self) -> &str {
    &self.path
  }

  pub fn get_namespace(&self) -> Vec<&str> {
    let parts: Vec<&str> = self.path.split('/').collect();
    if parts.len() <= 1 {
      return vec![];
    }
    parts[..parts.len() - 1].to_vec()
  }

  pub fn get_entity(&self) -> &str {
    if self.path.is_empty() {
      return "";
    }
    let parts: Vec<&str> = self.path.split('/').collect();
    parts[parts.len() - 1]
      .split('.')
      .next()
      .expect("entity should be present")
  }

  pub fn get_component(&self) -> Option<&str> {
    self.get_attributes().into_iter().next()
  }

  pub fn get_attributes(&self) -> Vec<&str> {
    let entity_attrs_parts: Vec<&str> = self.path.split('.').collect();
    if entity_attrs_parts.len() <= 1 {
      return vec![];
    }
    entity_attrs_parts[1..].to_vec()
  }

  pub fn from_parts<N, E, A>(namespace: N, entity: E, attributes: A) -> Self
  where
    N: IntoIterator,
    N::Item: Into<String>,
    E: Into<String>,
    A: IntoIterator,
    A::Item: Into<String>,
  {
    let namespace_str = namespace
      .into_iter()
      .map(|s| s.into())
      .collect::<Vec<String>>()
      .join("/");
    let entity_str = entity.into();
    let attributes_str = attributes
      .into_iter()
      .map(|s| s.into())
      .collect::<Vec<String>>()
      .join(".");

    let path = if !namespace_str.is_empty() {
      if !attributes_str.is_empty() {
        format!("{namespace_str}/{entity_str}.{attributes_str}")
      } else {
        format!("{namespace_str}/{entity_str}")
      }
    } else if !attributes_str.is_empty() {
      format!("{entity_str}.{attributes_str}")
    } else {
      entity_str
    };

    Key { path }
  }

  pub fn with_component<C: Into<String>>(self, component: C) -> Self {
    let mut attributes: Vec<String> = self
      .get_attributes()
      .into_iter()
      .map(str::to_owned)
      .collect();
    if let Some(current_component) = attributes.first_mut() {
      *current_component = component.into();
    } else {
      attributes.push(component.into());
    }
    Self::from_parts(self.get_namespace(), self.get_entity(), attributes)
  }
}

impl From<String> for Key {
  fn from(path: String) -> Self {
    Key { path }
  }
}

impl From<&str> for Key {
  fn from(path: &str) -> Self {
    Key {
      path: path.to_string(),
    }
  }
}

impl From<Key> for String {
  fn from(val: Key) -> Self {
    val.path
  }
}

impl std::fmt::Display for Key {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.path)
  }
}

impl FromStr for Key {
  type Err = <String as FromStr>::Err;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(Key {
      path: String::from_str(s)?,
    })
  }
}

impl AsRef<Key> for Key {
  fn as_ref(&self) -> &Key {
    self
  }
}

impl AsRef<str> for Key {
  fn as_ref(&self) -> &str {
    &self.path
  }
}

impl Borrow<str> for Key {
  fn borrow(&self) -> &str {
    &self.path
  }
}

impl Borrow<String> for Key {
  fn borrow(&self) -> &String {
    &self.path
  }
}

/// A change in the state: keys to set (to a value or to `None`) and keys to unset.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct StateChange {
  pub set: HashMap<Key, Option<Value>>,
  pub unset: HashSet<Key>,
}

impl Default for StateChange {
  fn default() -> Self {
    Self::new()
  }
}

impl StateChange {
  pub fn new() -> Self {
    StateChange {
      set: HashMap::new(),
      unset: HashSet::new(),
    }
  }

  /// A change that sets a single key to a value.
  pub fn set<K: Into<Key>>(key: K, value: Value) -> Self {
    StateChange {
      set: HashMap::from([(key.into(), Some(value))]),
      unset: HashSet::new(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.set.is_empty() && self.unset.is_empty()
  }

  pub fn len(&self) -> usize {
    self.set.len() + self.unset.len()
  }

  pub fn contains(&self, key: &Key) -> bool {
    self.set.contains_key(key) || self.unset.contains(key)
  }
}

impl<K, V> From<Vec<(K, V)>> for StateChange
where
  K: Into<Key>,
  V: Into<Value>,
{
  fn from(v: Vec<(K, V)>) -> Self {
    let set = v
      .into_iter()
      .map(|(k, v)| (k.into(), Some(v.into())))
      .collect();
    StateChange {
      set,
      unset: HashSet::new(),
    }
  }
}

/// Generic change type, used in various places.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum Change<T>
where
  T: PartialEq + Clone + Serialize + Eq + Hash,
{
  Add(T),
  Remove(T),
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn test_state_set_get_unset() {
    let mut state = State::new();
    let key = Key::from("a.b".to_string());

    assert_eq!(key.get_namespace(), Vec::<&str>::new());
    assert_eq!(key.get_entity(), "a");
    assert_eq!(key.get_attributes(), vec!["b"]);

    assert!(state.is_empty());
    assert!(state.get(&key).is_none());
    assert!(!state.evaluate_as_bool(&key));

    state.set(key.clone(), Some(Value::Boolean(true)));
    assert!(!state.is_empty());
    assert_eq!(state.get(&key), Some(&Some(Value::Boolean(true))));
    assert!(state.evaluate_as_bool(&key));

    state.unset(&key);
    assert!(state.get(&key).is_none());
    assert!(state.is_empty());

    state.set(key.clone(), None);
    assert_eq!(state.get(&key), Some(&None));
  }

  #[test]
  fn test_apply_state_changes() {
    let mut state = State::new();
    let change1: StateChange = ("x".to_string(), Some(Value::Boolean(true))).into();
    let mut change2 = StateChange::new();
    change2.unset.insert(Key::from("x".to_string()));

    state.apply(change1);
    assert_eq!(
      state.get(&Key::from("x".to_string())),
      Some(&Some(Value::Boolean(true)))
    );

    state.apply(change2);
    assert!(state.get(&Key::from("x".to_string())).is_none());
  }

  #[test]
  fn test_state_change_set_helper() {
    let sc = StateChange::set("battery/level", Value::Boolean(true));
    assert_eq!(sc.len(), 1);
    assert!(sc.contains(&Key::from("battery/level")));
  }

  #[test]
  fn test_key_from_str_and_borrow() {
    let path = "emoji-😊_123";
    let key: Key = path.parse().expect("failed to parse key");
    assert_eq!(key.path, path);
    let s: &str = key.as_ref();
    assert_eq!(s, path);
    let b: &str = key.borrow();
    assert_eq!(b, path);
  }

  #[test]
  fn test_hashmap_lookup_with_string() {
    let mut map: HashMap<Key, i32> = HashMap::new();
    map.insert(Key::from("test_key".to_string()), 42);
    assert_eq!(map.get("test_key"), Some(&42));
    assert_eq!(map.get(&"test_key".to_string()), Some(&42));
  }

  #[test]
  fn test_key_parts() {
    let key = Key::from("factory/robot1/arm/gripper.status".to_string());
    assert_eq!(key.get_namespace(), vec!["factory", "robot1", "arm"]);
    assert_eq!(key.get_entity(), "gripper");
    assert_eq!(key.get_attributes(), vec!["status"]);

    let key = Key::from("camera_front.resolution.width".to_string());
    assert_eq!(key.get_namespace(), Vec::<&str>::new());
    assert_eq!(key.get_entity(), "camera_front");
    assert_eq!(key.get_attributes(), vec!["resolution", "width"]);

    let uuid = "a2bfec-1234-5678-f90e-abcdef123456";
    let key = Key::from(uuid.to_string());
    assert_eq!(key.get_entity(), uuid);
    assert_eq!(key.get_attributes(), Vec::<&str>::new());
  }

  #[test]
  fn test_from_parts_and_with_component() {
    let key = Key::from_parts(["robot1"], "joint1", ["position"]);
    assert_eq!(key.get_path(), "robot1/joint1.position");
    let key = key.with_component("velocity");
    assert_eq!(key.get_path(), "robot1/joint1.velocity");
  }
}
