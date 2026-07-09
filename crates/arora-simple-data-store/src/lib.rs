//! A simple hashmap-backed [`DataStore`].
//!
//! The lean, dependency-free reference implementation of
//! [`arora_types::data::DataStore`]: a `HashMap` of path-keyed cells, std
//! channels for change subscriptions. Cheaply cloneable — clones share the same
//! storage, so the same blackboard can be handed to the HAL, the bridge, the
//! behavior tree, and the engine at once. Richer backends (e.g. arora-ecbs) can
//! implement the same trait.

mod namespaced;
pub use namespaced::NamespacedStore;

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, RwLock};

use arora_types::data::{DataError, DataStore, Key, Slot, State, StateChange, Subscription};
use arora_types::value::Value;

/// One key's storage cell, shared so a [`Slot`] keeps a direct reference to it.
type Cell = Arc<RwLock<Option<Value>>>;

#[derive(Default)]
struct Inner {
    cells: RwLock<HashMap<Key, Cell>>,
    subscribers: Mutex<Vec<Sender<StateChange>>>,
}

impl Inner {
    /// Broadcast a change to live subscribers, pruning ones whose receiver was
    /// dropped.
    fn notify(&self, change: StateChange) {
        if change.is_empty() {
            return;
        }
        let mut subs = self.subscribers.lock().unwrap();
        subs.retain(|tx| tx.send(change.clone()).is_ok());
    }
}

/// A simple hashmap-backed [`DataStore`]. Clone to share the same storage.
#[derive(Clone, Default)]
pub struct SimpleDataStore {
    inner: Arc<Inner>,
}

impl SimpleDataStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get (or create) the storage cell for a key.
    fn cell(&self, key: &Key) -> Cell {
        if let Some(cell) = self.inner.cells.read().unwrap().get(key) {
            return cell.clone();
        }
        self.inner
            .cells
            .write()
            .unwrap()
            .entry(key.clone())
            .or_insert_with(|| Arc::new(RwLock::new(None)))
            .clone()
    }
}

impl DataStore for SimpleDataStore {
    fn read(&self, keys: &[Key]) -> Vec<Option<Value>> {
        let cells = self.inner.cells.read().unwrap();
        keys.iter()
            .map(|k| cells.get(k).and_then(|c| c.read().unwrap().clone()))
            .collect()
    }

    fn write(&self, changes: StateChange) -> Result<(), DataError> {
        // Observers see value CHANGES: a write that leaves a key at the value
        // it already holds is dropped from the notification (and an unset of an
        // absent key likewise). This is what keeps echo cycles damped — e.g. a
        // HAL that mirrors actuation back as state produces one change, not a
        // feedback loop. (`f32`/`f64` NaNs compare unequal, so NaN writes
        // always notify.)
        let mut effective = StateChange::new();
        for (key, value) in &changes.set {
            let cell = self.cell(key);
            let mut current = cell.write().unwrap();
            if current.as_ref() != value.as_ref() {
                *current = value.clone();
                effective.set.insert(key.clone(), value.clone());
            }
        }
        for key in &changes.unset {
            // Keep the cell (so any outstanding Slot stays valid); clear its value.
            let cell = self.cell(key);
            let mut current = cell.write().unwrap();
            if current.is_some() {
                *current = None;
                effective.unset.insert(key.clone());
            }
        }
        if !effective.is_empty() {
            self.inner.notify(effective);
        }
        Ok(())
    }

    fn snapshot(&self) -> State {
        let cells = self.inner.cells.read().unwrap();
        let storage = cells
            .iter()
            .map(|(k, c)| (k.clone(), c.read().unwrap().clone()))
            .collect();
        State { storage }
    }

    fn slot(&self, key: &Key) -> Box<dyn Slot> {
        Box::new(SimpleSlot {
            cell: self.cell(key),
            key: key.clone(),
            inner: self.inner.clone(),
        })
    }

    fn subscribe(&self) -> Subscription {
        let (tx, rx) = channel();
        self.inner.subscribers.lock().unwrap().push(tx);
        Subscription::new(rx)
    }
}

/// A direct handle to one key's cell — reads and writes hit the same storage as
/// the store's `read`/`write` for that key, without re-resolving the path.
struct SimpleSlot {
    cell: Cell,
    key: Key,
    inner: Arc<Inner>,
}

impl Slot for SimpleSlot {
    fn get(&self) -> Option<Value> {
        self.cell.read().unwrap().clone()
    }

    fn set(&self, value: Option<Value>) -> Result<(), DataError> {
        // Same change-only notification as `DataStore::write`: setting the
        // value the cell already holds is a no-op for observers.
        {
            let mut current = self.cell.write().unwrap();
            if *current == value {
                return Ok(());
            }
            *current = value.clone();
        }
        self.inner.notify(StateChange {
            set: HashMap::from([(self.key.clone(), value)]),
            unset: Default::default(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read() {
        let store = SimpleDataStore::new();
        store
            .write(StateChange::set("a/b", Value::Boolean(true)))
            .unwrap();
        assert_eq!(
            store.read(&[Key::from("a/b")]),
            vec![Some(Value::Boolean(true))]
        );
        assert_eq!(store.read(&[Key::from("missing")]), vec![None]);
    }

    #[test]
    fn slot_and_store_coincide() {
        let store = SimpleDataStore::new();
        let slot = store.slot(&Key::from("x"));
        // write through the slot, read through the store
        slot.set(Some(Value::Boolean(true))).unwrap();
        assert_eq!(
            store.read(&[Key::from("x")]),
            vec![Some(Value::Boolean(true))]
        );
        // write through the store, read through the slot (same cell)
        store
            .write(StateChange::set("x", Value::Boolean(false)))
            .unwrap();
        assert_eq!(slot.get(), Some(Value::Boolean(false)));
    }

    #[test]
    fn subscribe_delivers_changes_to_all() {
        let store = SimpleDataStore::new();
        let s1 = store.subscribe();
        let s2 = store.subscribe();
        store
            .write(StateChange::set("k", Value::Boolean(true)))
            .unwrap();
        assert!(s1.try_recv().expect("s1 change").contains(&Key::from("k")));
        assert!(s2.try_recv().expect("s2 change").contains(&Key::from("k")));
    }

    #[test]
    fn slot_set_notifies_subscribers() {
        let store = SimpleDataStore::new();
        let sub = store.subscribe();
        store
            .slot(&Key::from("y"))
            .set(Some(Value::Boolean(true)))
            .unwrap();
        assert!(sub.try_recv().expect("change").contains(&Key::from("y")));
    }

    #[test]
    fn snapshot_returns_all() {
        let store = SimpleDataStore::new();
        store
            .write(StateChange::set("a", Value::Boolean(true)))
            .unwrap();
        store
            .write(StateChange::set("b", Value::Boolean(false)))
            .unwrap();
        assert_eq!(store.snapshot().storage.len(), 2);
    }

    #[test]
    fn clones_share_storage() {
        let store = SimpleDataStore::new();
        let other = store.clone();
        store
            .write(StateChange::set("shared", Value::Boolean(true)))
            .unwrap();
        assert_eq!(
            other.read(&[Key::from("shared")]),
            vec![Some(Value::Boolean(true))]
        );
    }
}
