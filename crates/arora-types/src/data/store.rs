//! The data-store interface: a shared, path-keyed blackboard that the HAL, the
//! bridge, and execution engines (behavior tree, modules) all read and write.
//!
//! Design notes (see `docs/plan-bring-studio-bridge-in.md`):
//! - `read` returns `Vec<Option<Value>>`; any further nesting lives inside
//!   [`Value`] itself, for whoever needs it.
//! - The store uses interior mutability (`&self`), so one store can be handed to
//!   the HAL, the bridge, the BT, and the engine at once.
//! - [`DataStore::slot`] hands out a [`Slot`]: resolve a key once, then read and
//!   write that exact storage cell without further lookups. Reads and writes
//!   through the slot coincide with `read`/`write` on the same key.
//! - [`DataStore::subscribe`] is intentionally lean (a std channel), so this
//!   crate stays free of an async runtime. A `futures::Stream` adapter is an
//!   opt-in extension (a future `stream` feature), not the primary API.

use std::collections::HashMap;
use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};

use crate::value::{Type, Value};

use super::state::{Key, State, StateChange};

/// Something went wrong reading from or writing to a [`DataStore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
  /// The key could not be resolved (e.g. an alias with no target).
  NoSuchKey(String),
  /// Anything else, with a message.
  Other(String),
}

impl std::fmt::Display for DataError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      DataError::NoSuchKey(k) => write!(f, "no such key: {k}"),
      DataError::Other(msg) => write!(f, "{msg}"),
    }
  }
}

impl std::error::Error for DataError {}

/// A direct handle to one key's storage cell.
///
/// Obtained from [`DataStore::slot`]. The key is resolved once; afterwards
/// [`get`](Slot::get) and [`set`](Slot::set) act on the same cell without
/// repeating the lookup. This mirrors the behavior-tree blackboard's habit of
/// holding a direct reference to a value rather than re-resolving a path every
/// tick — here made `Send + Sync` so it can be shared across tasks.
pub trait Slot: Send + Sync {
  /// Read the current value of the cell.
  fn get(&self) -> Option<Value>;
  /// Write the cell; observers of the store see the corresponding change.
  fn set(&self, value: Option<Value>) -> Result<(), DataError>;
}

/// A feed of changes from a [`DataStore`], obtained from
/// [`DataStore::subscribe`]. A subscription's first change is the store's
/// whole current state; every change applied after it was created follows.
///
/// This is deliberately a plain synchronous channel so `arora-types` needs no
/// async runtime. Async consumers can poll [`try_recv`](Subscription::try_recv)
/// from their own loop, or adapt it to a `futures::Stream` (an opt-in extension).
pub struct Subscription {
  rx: Receiver<StateChange>,
}

impl Subscription {
  /// Wrap a receiver. `DataStore` implementations build the channel and keep
  /// the sender side.
  pub fn new(rx: Receiver<StateChange>) -> Self {
    Self { rx }
  }

  /// Block until the next change (or `None` if the store was dropped).
  pub fn recv(&self) -> Option<StateChange> {
    self.rx.recv().ok()
  }

  /// Take the next change if one is already available, without blocking.
  pub fn try_recv(&self) -> Option<StateChange> {
    self.rx.try_recv().ok()
  }

  /// Drain all currently-available changes without blocking.
  pub fn try_iter(&self) -> impl Iterator<Item = StateChange> + '_ {
    self.rx.try_iter()
  }
}

/// What a key is, beyond the value it currently holds: the range it runs over
/// and the unit it is counted in, where it rests, what it is for, and whether
/// anything outside the device may write it.
///
/// A value shows its own shape ([`Value::kind`]), so this is what the value
/// cannot say. It is the store's to keep, because it is a property of the key
/// rather than of any one reader: every bridge relays the same answer, and a
/// backend that already knows a key's range (a schema-backed store) surfaces it
/// without anyone restating it.
///
/// **A key is closed to remote writers unless its meta says otherwise.** The
/// device's own writers — its HAL, its modules, its behavior — are never asked;
/// `editable` is what every bridge's inbound write is checked against, and a key
/// nobody described is not a network peer's to set. A device opens its inputs
/// by saying so, per key or per subtree
/// ([`set_prefix_meta`](DataStore::set_prefix_meta)).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyMeta {
  /// The shape the key holds, when it is fixed — an unset key has no value to
  /// read it from, and a key that is written by one producer has one shape.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub ty: Option<Type>,
  /// The lowest value it takes, for a numeric key.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub min: Option<f64>,
  /// The highest value it takes, for a numeric key.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub max: Option<f64>,
  /// What a numeric key's values are counted in — radians, metres, a
  /// fraction — named freely for whoever displays or converts them. It is a
  /// name, not an algebra: Arora relays it as written and computes nothing
  /// from it.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub unit: Option<String>,
  /// Where it rests: what a reset puts back.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub default: Option<Value>,
  /// What the key is for, in a sentence, for whoever drives it.
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub description: Option<String>,
  /// Whether a writer outside the device may set it — the device's inputs.
  /// Closed unless said: an unauthenticated peer on a bridge does not get to set
  /// a key the device never offered.
  #[serde(default)]
  pub editable: bool,
}

impl KeyMeta {
  /// Nothing said yet: a key closed to remote writers, of whatever shape its
  /// value has.
  pub fn new() -> Self {
    Self::default()
  }

  /// A remote writer may set it: one of the device's inputs.
  pub fn editable(mut self) -> Self {
    self.editable = true;
    self
  }

  /// The range it runs over.
  pub fn range(mut self, min: f64, max: f64) -> Self {
    self.min = Some(min);
    self.max = Some(max);
    self
  }

  /// What its values are counted in.
  pub fn in_unit(mut self, unit: impl Into<String>) -> Self {
    self.unit = Some(unit.into());
    self
  }

  /// Where it rests.
  pub fn resting_at(mut self, value: Value) -> Self {
    self.default = Some(value);
    self
  }

  /// What it is for.
  pub fn described(mut self, description: impl Into<String>) -> Self {
    self.description = Some(description.into());
    self
  }

  /// The shape it holds.
  pub fn of_type(mut self, ty: Type) -> Self {
    self.ty = Some(ty);
    self
  }
}

/// Whether `prefix` covers `path`: the whole subtree under it, on a segment
/// boundary — `face` covers `face/mouth` but not `faceplate` — and the empty
/// prefix covers every key.
pub fn prefix_covers(prefix: &str, path: &str) -> bool {
  prefix.is_empty()
    || path == prefix
    || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'))
}

/// A shared, path-keyed store of [`Value`]s, observable through change
/// subscriptions. The canonical lean implementation is
/// [`arora-simple-data-store`](https://docs.rs/arora-simple-data-store); richer
/// backends (e.g. arora-ecbs) can implement the same trait.
pub trait DataStore: Send + Sync {
  /// Read several keys at once. Each entry is the key's current value, or
  /// `None` if the key is unset/absent.
  fn read(&self, keys: &[Key]) -> Vec<Option<Value>>;

  /// Apply a batch of changes. Observers receive the same [`StateChange`].
  fn write(&self, changes: StateChange) -> Result<(), DataError>;

  /// A snapshot of the entire store.
  fn snapshot(&self) -> State;

  /// Resolve a key to a direct [`Slot`] handle (read + write the same cell
  /// without repeating the lookup).
  fn slot(&self, key: &Key) -> Box<dyn Slot>;

  /// Subscribe to changes. Each call yields an independent [`Subscription`],
  /// whose first change is the store's whole current state: a subscriber
  /// starts from the full picture and stays current from the changes that
  /// follow, without a separate snapshot read that could race them.
  fn subscribe(&self) -> Subscription;

  /// What these keys are, beyond the values they hold: for each, the most
  /// specific statement the store has — the key's own meta, else the meta of the
  /// deepest subtree covering it ([`set_prefix_meta`](Self::set_prefix_meta)),
  /// else `None`. A statement replaces a broader one whole; it does not merge
  /// with it.
  ///
  /// The default answers `None` for every key: a store that keeps no meta is a
  /// store whose keys are plain and closed to remote writers.
  fn meta(&self, keys: &[Key]) -> Vec<Option<KeyMeta>> {
    vec![None; keys.len()]
  }

  /// Every key the store holds meta for, whether or not it holds a value yet —
  /// a key can be described before anything writes it.
  ///
  /// The default is empty, for the same reason as [`meta`](Self::meta).
  fn all_meta(&self) -> HashMap<Key, KeyMeta> {
    HashMap::new()
  }

  /// Say what these keys are. A device does this as it composes, and again
  /// whenever its composition changes — a module loaded mid-run says what it
  /// brought.
  ///
  /// The default refuses: a store that cannot keep meta says so, rather than
  /// accepting it and losing it.
  fn set_meta(&self, _meta: HashMap<Key, KeyMeta>) -> Result<(), DataError> {
    Err(DataError::Other("this store keeps no key meta".to_string()))
  }

  /// Say what every key under these prefixes is, until a more specific statement
  /// says otherwise — how a device opens a subtree of inputs at once, and, with
  /// the empty prefix, how a sandbox opens everything. See [`prefix_covers`] for
  /// what a prefix covers.
  ///
  /// The default refuses, as [`set_meta`](Self::set_meta) does.
  fn set_prefix_meta(&self, _meta: HashMap<String, KeyMeta>) -> Result<(), DataError> {
    Err(DataError::Other("this store keeps no key meta".to_string()))
  }

  /// A sibling handle onto the **same** storage: reads and writes through the
  /// clone coincide with the original's. Stores share their storage across
  /// clones by design (interior mutability, `&self` everywhere), and this
  /// puts that fact on the trait, so a holder of `dyn DataStore` can keep an
  /// independent live handle — e.g. one kept aside before handing a store to
  /// a device that takes it by value.
  fn clone_box(&self) -> Box<dyn DataStore>;
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::value::Type;
  use crate::value_serde;

  /// The wire shape of a key's meta: a field the store did not set is absent,
  /// not null, so a reader tells "no unit" from the absence of `unit`.
  #[test]
  fn key_meta_serializes_its_unit_only_when_set() {
    let in_fractions = KeyMeta::new()
      .of_type(Type::F64)
      .range(0.0, 1.0)
      .in_unit("fraction");
    let json = serde_json::to_value(&in_fractions).unwrap();
    assert_eq!(
      json,
      serde_json::json!({
        "ty": "f64", "min": 0.0, "max": 1.0, "unit": "fraction", "editable": false
      })
    );
    assert_eq!(
      serde_json::from_value::<KeyMeta>(json).unwrap(),
      in_fractions
    );

    let unitless = KeyMeta::new().of_type(Type::F64).range(0.0, 1.0);
    let json = serde_json::to_value(&unitless).unwrap();
    assert_eq!(
      json,
      serde_json::json!({"ty": "f64", "min": 0.0, "max": 1.0, "editable": false})
    );
    assert_eq!(serde_json::from_value::<KeyMeta>(json).unwrap(), unitless);
  }

  /// The value plane, which carries a `ListKeys` answer from the runtime to its
  /// bridges, keeps the unit as it keeps every other field.
  #[test]
  fn key_meta_round_trips_through_the_value_plane() {
    for meta in [KeyMeta::new().in_unit("rad"), KeyMeta::new()] {
      let value = value_serde::to_value(&meta).unwrap();
      assert_eq!(value_serde::from_value::<KeyMeta>(value).unwrap(), meta);
    }
  }
}
