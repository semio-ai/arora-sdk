//! A [`DataStore`] wrapper that namespaces every key under a common prefix.
//!
//! One mutualized store can be shared across many runtimes (a device's HAL,
//! bridge, and behavior writes), each runtime seeing a *device-relative* view of
//! the keys while the storage lives under a per-device prefix in the single
//! shared store. [`NamespacedStore`] is that view: it transparently rewrites a
//! device-relative key `joint1.position` to `<namespace>/joint1.position` before
//! delegating to the inner store, so two namespaces over one shared inner store
//! never collide.
//!
//! It does NOT own the storage: it wraps a shared inner store (e.g. a cloned
//! [`SimpleDataStore`], whose clones share storage) and is `Send + Sync`, so it
//! can be handed to [`Runtime::with_io_in`] as `Arc<dyn DataStore>`.

use std::sync::Arc;

use arora_types::data::{DataError, DataStore, Key, Slot, State, StateChange, Subscription};
use arora_types::value::Value;

/// A [`DataStore`] view that prefixes every key with `<namespace>/` before
/// delegating to an inner, shared store.
///
/// The inner store is held as `Arc<dyn DataStore>`, so the same underlying
/// storage can back several differently-namespaced views (and other,
/// un-namespaced holders) at once.
pub struct NamespacedStore {
    inner: Arc<dyn DataStore>,
    namespace: String,
}

impl NamespacedStore {
    /// Wrap `inner`, prefixing every key with `<namespace>/`.
    pub fn new(inner: Arc<dyn DataStore>, namespace: impl Into<String>) -> Self {
        Self {
            inner,
            namespace: namespace.into(),
        }
    }

    /// The namespace this view prefixes keys with.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Rewrite a device-relative key into its namespaced form
    /// (`<namespace>/<key_path>`).
    fn prefixed(&self, key: &Key) -> Key {
        Key::from(format!("{}/{}", self.namespace, key.path))
    }
}

impl DataStore for NamespacedStore {
    fn read(&self, keys: &[Key]) -> Vec<Option<Value>> {
        let prefixed: Vec<Key> = keys.iter().map(|k| self.prefixed(k)).collect();
        // The inner store preserves order, so values line up with `keys`.
        self.inner.read(&prefixed)
    }

    fn write(&self, changes: StateChange) -> Result<(), DataError> {
        let set = changes
            .set
            .iter()
            .map(|(k, v)| (self.prefixed(k), v.clone()))
            .collect();
        let unset = changes.unset.iter().map(|k| self.prefixed(k)).collect();
        self.inner.write(StateChange { set, unset })
    }

    fn slot(&self, key: &Key) -> Box<dyn Slot> {
        self.inner.slot(&self.prefixed(key))
    }

    /// Delegates to the inner store as-is — the snapshot carries the **full,
    /// namespaced** keys, not the device-relative ones.
    ///
    /// NOTE: not yet prefix-filtered or stripped. A namespaced view's snapshot
    /// returns the entire shared store (every namespace), with full keys. This
    /// is a later refinement; for now it is fine because the runtime only drains
    /// [`subscribe`](DataStore::subscribe) to flush changes outward and the
    /// device's HAL/bridge are fakes.
    fn snapshot(&self) -> State {
        self.inner.snapshot()
    }

    /// Delegates to the inner store as-is — the feed carries the **full,
    /// namespaced** keys, not the device-relative ones, and is **not**
    /// filtered to this namespace.
    ///
    /// NOTE: not yet prefix-filtered or stripped (see [`snapshot`](Self::snapshot)).
    /// A later refinement; fine for now because the runtime only drains this to
    /// flush changes outward (already-namespaced keys are what the shared feed
    /// wants) and the device's HAL/bridge are fakes.
    fn subscribe(&self) -> Subscription {
        self.inner.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimpleDataStore;

    #[test]
    fn write_lands_under_namespace_in_inner_store() {
        let shared = SimpleDataStore::new();
        let view = NamespacedStore::new(Arc::new(shared.clone()), "robotA");

        // A device-relative write through the view…
        view.write(StateChange::set("joint1.position", Value::Boolean(true)))
            .unwrap();

        // …lands prefixed in the inner store.
        assert_eq!(
            shared.read(&[Key::from("robotA/joint1.position")]),
            vec![Some(Value::Boolean(true))]
        );
        // The un-prefixed key is NOT present in the inner store.
        assert_eq!(shared.read(&[Key::from("joint1.position")]), vec![None]);
    }

    #[test]
    fn read_round_trips_device_relative_keys() {
        let shared = SimpleDataStore::new();
        let view = NamespacedStore::new(Arc::new(shared.clone()), "robotA");

        view.write(StateChange::set("battery_level", Value::Boolean(false)))
            .unwrap();

        // Reading the device-relative key through the view returns the value.
        assert_eq!(
            view.read(&[Key::from("battery_level")]),
            vec![Some(Value::Boolean(false))]
        );
    }

    #[test]
    fn slot_round_trips_and_coincides_with_inner() {
        let shared = SimpleDataStore::new();
        let view = NamespacedStore::new(Arc::new(shared.clone()), "robotA");

        // Write through a device-relative slot…
        let slot = view.slot(&Key::from("joint1.position"));
        slot.set(Some(Value::Boolean(true))).unwrap();

        // …reads back through the view's slot…
        assert_eq!(slot.get(), Some(Value::Boolean(true)));
        // …and lands prefixed in the inner store.
        assert_eq!(
            shared.read(&[Key::from("robotA/joint1.position")]),
            vec![Some(Value::Boolean(true))]
        );
    }

    #[test]
    fn two_namespaces_over_one_inner_store_do_not_collide() {
        let shared = SimpleDataStore::new();
        let a = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let b = NamespacedStore::new(Arc::new(shared.clone()), "robotB");

        // Both write the SAME device-relative key…
        a.write(StateChange::set("joint1.position", Value::Boolean(true)))
            .unwrap();
        b.write(StateChange::set("joint1.position", Value::Boolean(false)))
            .unwrap();

        // …but each lands under its own prefix, so they don't clobber.
        assert_eq!(
            a.read(&[Key::from("joint1.position")]),
            vec![Some(Value::Boolean(true))]
        );
        assert_eq!(
            b.read(&[Key::from("joint1.position")]),
            vec![Some(Value::Boolean(false))]
        );
        // And both prefixed keys coexist in the shared inner store.
        assert_eq!(
            shared.read(&[
                Key::from("robotA/joint1.position"),
                Key::from("robotB/joint1.position"),
            ]),
            vec![Some(Value::Boolean(true)), Some(Value::Boolean(false))]
        );
    }
}
