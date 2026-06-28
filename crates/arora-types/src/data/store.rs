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

use std::sync::mpsc::Receiver;

use crate::value::Value;

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

/// A feed of changes applied to a [`DataStore`], obtained from
/// [`DataStore::subscribe`]. Each subscription receives every change applied
/// after it was created.
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

  /// Subscribe to changes. Each call yields an independent [`Subscription`].
  fn subscribe(&self) -> Subscription;
}
