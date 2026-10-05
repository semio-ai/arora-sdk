//! A [`DataStore`] wrapper that namespaces every key under a common prefix.
//!
//! One mutualized store can be shared across many runtimes (a device's HAL,
//! bridge, and behavior writes), each runtime seeing a *device-relative* view of
//! the keys while the storage lives under a per-device prefix in the single
//! shared store. [`NamespacedStore`] is that view: it rewrites a device-relative
//! key `joint1.position` to `<namespace>/joint1.position` before delegating to
//! the inner store, and shows the inner store's `<namespace>/…` keys, and those
//! alone, under their device-relative names — so two namespaces over one shared
//! inner store never collide, and neither sees the other.
//!
//! It does NOT own the storage: it wraps a shared inner store (e.g. a cloned
//! [`SimpleDataStore`], whose clones share storage) and is `Send + Sync`, so it
//! can be handed to a device via `Arora::builder().with_data_store(..)` as
//! `Arc<dyn DataStore>`.

use std::collections::HashMap;
use std::sync::Arc;

use arora_types::data::{
    DataError, DataStore, Key, KeyMeta, Slot, State, StateChange, Subscription,
};
use arora_types::value::Value;

/// A [`DataStore`] view confined to one namespace of an inner, shared store.
///
/// A device-relative key goes in as `<namespace>/<key>`; what comes out — a
/// snapshot, the meta listing, the change feed — holds the keys under
/// `<namespace>/` and no other, stripped of it. It is what a device holds when
/// several share one store: each sees its own keys and none of its neighbours'.
///
/// The inner store is held as `Arc<dyn DataStore>`, so the same underlying
/// storage can back several differently-namespaced views (and other,
/// un-namespaced holders) at once.
#[derive(Clone)]
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

    /// The device-relative form of an inner key: `None` for a key outside this
    /// namespace.
    fn relative(&self, key: &Key) -> Option<Key> {
        relative(&self.namespace, key)
    }

    /// The inner form of a subtree of the device: the empty prefix is the whole
    /// device — its namespace — and never a neighbour's.
    fn prefixed_subtree(&self, prefix: &str) -> String {
        if prefix.is_empty() {
            self.namespace.clone()
        } else {
            format!("{}/{prefix}", self.namespace)
        }
    }
}

/// The device-relative form of `key` under `namespace`: its path past
/// `<namespace>/`, or `None` for a key that is not under it — a neighbour's,
/// one whose first segment merely starts with the namespace, or the bare
/// `<namespace>` itself, which names no key of the device.
fn relative(namespace: &str, key: &Key) -> Option<Key> {
    key.path
        .strip_prefix(namespace)?
        .strip_prefix('/')
        .map(Key::from)
}

/// `change` as the device sees it: the keys under `namespace`, stripped of it;
/// a key outside it is not the device's and is left out.
fn confine(namespace: &str, change: StateChange) -> StateChange {
    StateChange {
        set: change
            .set
            .into_iter()
            .filter_map(|(key, value)| Some((relative(namespace, &key)?, value)))
            .collect(),
        unset: change
            .unset
            .into_iter()
            .filter_map(|key| relative(namespace, &key))
            .collect(),
    }
}

impl DataStore for NamespacedStore {
    fn read(&self, keys: &[Key]) -> Vec<Option<Value>> {
        let prefixed: Vec<Key> = keys.iter().map(|k| self.prefixed(k)).collect();
        // The inner store preserves order, so values line up with `keys`.
        self.inner.read(&prefixed)
    }

    fn meta(&self, keys: &[Key]) -> Vec<Option<KeyMeta>> {
        let prefixed: Vec<Key> = keys.iter().map(|k| self.prefixed(k)).collect();
        self.inner.meta(&prefixed)
    }

    /// The meta of this device's keys, under their device-relative names; a
    /// neighbour's keys are not listed.
    fn all_meta(&self) -> HashMap<Key, KeyMeta> {
        self.inner
            .all_meta()
            .into_iter()
            .filter_map(|(key, meta)| Some((self.relative(&key)?, meta)))
            .collect()
    }

    fn set_meta(&self, meta: HashMap<Key, KeyMeta>) -> Result<(), DataError> {
        self.inner.set_meta(
            meta.into_iter()
                .map(|(key, meta)| (self.prefixed(&key), meta))
                .collect(),
        )
    }

    /// A subtree of this device's keys: the empty prefix is the whole device —
    /// its namespace — and never a neighbour's.
    fn set_prefix_meta(&self, meta: HashMap<String, KeyMeta>) -> Result<(), DataError> {
        self.inner.set_prefix_meta(
            meta.into_iter()
                .map(|(prefix, meta)| (self.prefixed_subtree(&prefix), meta))
                .collect(),
        )
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

    /// This device's keys, under their device-relative names: the inner
    /// store's `<namespace>/…` keys and no other.
    fn snapshot(&self) -> State {
        State {
            storage: self
                .inner
                .snapshot()
                .storage
                .into_iter()
                .filter_map(|(key, value)| Some((self.relative(&key)?, value)))
                .collect(),
        }
    }

    /// The inner store's feed confined to this namespace: each change holds the
    /// keys under `<namespace>/`, stripped of it, the opening state included.
    /// A change that touches no key of the device is not delivered, so a
    /// neighbour's writes neither reach nor wake this device.
    ///
    /// The feed is the inner store's subscription to the namespace's subtree
    /// ([`DataStore::subscribe_prefix`]), so a store that routes by prefix, as
    /// [`SimpleDataStore`](crate::SimpleDataStore) does, never sends this view a
    /// neighbour's keys.
    fn subscribe(&self) -> Subscription {
        self.subscribe_prefix("")
    }

    /// The device's keys under `prefix`, the empty prefix being the whole
    /// device: the inner store's subscription to `<namespace>/<prefix>`, its
    /// keys stripped of `<namespace>/`.
    fn subscribe_prefix(&self, prefix: &str) -> Subscription {
        let namespace = self.namespace.clone();
        self.inner
            .subscribe_prefix(&self.prefixed_subtree(prefix))
            .map(move |change| confine(&namespace, change))
    }

    fn clone_box(&self) -> Box<dyn DataStore> {
        Box::new(self.clone())
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

    /// Two devices over one store: what one writes, the other's subscription,
    /// snapshot and meta listing never show — and what they do show is under
    /// device-relative names.
    #[test]
    fn a_view_shows_its_namespace_and_no_other() {
        let shared = SimpleDataStore::new();
        let a = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let b = NamespacedStore::new(Arc::new(shared.clone()), "robotB");
        let a_changes = a.subscribe();
        assert_eq!(
            a_changes.try_recv(),
            Some(StateChange::new()),
            "an empty namespace opens on an empty state"
        );

        b.write(StateChange::set("joint1.position", Value::Boolean(false)))
            .unwrap();
        b.set_meta(HashMap::from([(
            Key::from("joint1.position"),
            KeyMeta::new().editable(),
        )]))
        .unwrap();
        assert_eq!(
            a_changes.try_recv(),
            None,
            "a neighbour's write is not a change of this device"
        );
        assert!(a.snapshot().is_empty(), "a neighbour's keys are not listed");
        assert!(a.all_meta().is_empty(), "a neighbour's meta is not listed");

        a.write(StateChange::set("joint1.position", Value::Boolean(true)))
            .unwrap();
        a.set_meta(HashMap::from([(
            Key::from("joint1.position"),
            KeyMeta::new().described("this device's"),
        )]))
        .unwrap();
        assert_eq!(
            a_changes.try_recv(),
            Some(StateChange::set("joint1.position", Value::Boolean(true))),
            "the device's own write, under its device-relative name"
        );
        assert_eq!(
            a.snapshot().storage,
            HashMap::from([(Key::from("joint1.position"), Some(Value::Boolean(true)))])
        );
        assert_eq!(
            a.all_meta(),
            HashMap::from([(
                Key::from("joint1.position"),
                KeyMeta::new().described("this device's")
            )])
        );

        a.write(StateChange {
            set: HashMap::new(),
            unset: [Key::from("joint1.position")].into_iter().collect(),
        })
        .unwrap();
        let unset = a_changes.try_recv().expect("the device's own unset");
        assert!(unset.set.is_empty());
        assert!(unset.unset.contains(&Key::from("joint1.position")));
    }

    /// A subscription opens on the device's own state alone, under
    /// device-relative names: neither a neighbour's keys, nor a key whose first
    /// segment merely starts with the namespace, nor the bare namespace.
    #[test]
    fn a_subscription_opens_on_the_namespace_alone() {
        let shared = SimpleDataStore::new();
        let a = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let b = NamespacedStore::new(Arc::new(shared.clone()), "robotB");
        a.write(StateChange::set("battery_level", Value::Boolean(true)))
            .unwrap();
        b.write(StateChange::set("battery_level", Value::Boolean(false)))
            .unwrap();
        shared
            .write(StateChange::set(
                "robotAlpha/battery_level",
                Value::Boolean(false),
            ))
            .unwrap();
        shared
            .write(StateChange::set("robotA", Value::Boolean(false)))
            .unwrap();

        let opening = a.subscribe().try_recv().expect("opening state");
        assert_eq!(
            opening,
            StateChange::set("battery_level", Value::Boolean(true))
        );
        assert_eq!(
            a.snapshot().storage,
            HashMap::from([(Key::from("battery_level"), Some(Value::Boolean(true)))])
        );
    }

    /// The prefixes the shared store keeps for its subscribers.
    fn subscriber_prefixes(shared: &SimpleDataStore) -> Vec<String> {
        let subscribers = shared.inner.subscribers.lock().unwrap();
        subscribers.iter().map(|sub| sub.prefix.clone()).collect()
    }

    /// A view subscribes the store to its namespace's subtree, and a view of a
    /// view to the subtree of both: the store sends each view its own keys and
    /// never a neighbour's.
    #[test]
    fn a_view_subscribes_the_store_to_its_namespace() {
        let shared = SimpleDataStore::new();
        let a = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let arm = NamespacedStore::new(Arc::new(a.clone()), "arm");
        let _a_changes = a.subscribe();
        let arm_changes = arm.subscribe();
        let arm_joints = arm.subscribe_prefix("joints");
        assert_eq!(
            subscriber_prefixes(&shared),
            ["robotA", "robotA/arm", "robotA/arm/joints"]
        );

        arm_changes.try_recv().expect("opening state");
        a.write(StateChange::from(vec![
            ("arm/joints/elbow", Value::Boolean(true)),
            ("head", Value::Boolean(true)),
        ]))
        .unwrap();
        assert_eq!(
            arm_changes.try_recv(),
            Some(StateChange::set("joints/elbow", Value::Boolean(true))),
            "the arm's keys, relative to the arm"
        );
        assert_eq!(
            arm_joints.try_recv(),
            Some(StateChange::new()),
            "the subtree's opening state: empty"
        );
        assert_eq!(
            arm_joints.try_recv(),
            Some(StateChange::set("joints/elbow", Value::Boolean(true))),
            "a subtree of the view, under the view's names"
        );
        arm.write(StateChange::set("wrist", Value::Boolean(true)))
            .unwrap();
        assert_eq!(arm_joints.try_recv(), None, "outside the subtree");
    }

    /// A store that keeps no subscriber prefix: everything forwarded to a
    /// [`SimpleDataStore`] but `subscribe_prefix`, which is left to the
    /// trait's default.
    #[derive(Clone)]
    struct Unrouted(SimpleDataStore);

    impl DataStore for Unrouted {
        fn read(&self, keys: &[Key]) -> Vec<Option<Value>> {
            self.0.read(keys)
        }
        fn write(&self, changes: StateChange) -> Result<(), DataError> {
            self.0.write(changes)
        }
        fn snapshot(&self) -> State {
            self.0.snapshot()
        }
        fn slot(&self, key: &Key) -> Box<dyn Slot> {
            self.0.slot(key)
        }
        fn subscribe(&self) -> Subscription {
            self.0.subscribe()
        }
        fn clone_box(&self) -> Box<dyn DataStore> {
            Box::new(self.clone())
        }
    }

    /// Over a store that keeps the trait's default `subscribe_prefix`, a view
    /// shows the same feed: the store sends it every change and the view keeps
    /// its own keys.
    #[test]
    fn a_view_over_an_unrouted_store_shows_its_namespace_alone() {
        let shared = Unrouted(SimpleDataStore::new());
        shared
            .write(StateChange::from(vec![
                ("robotA", Value::Boolean(true)),
                ("robotA/battery_level", Value::Boolean(true)),
                ("robotAlpha/battery_level", Value::Boolean(true)),
            ]))
            .unwrap();
        let a = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let b = NamespacedStore::new(Arc::new(shared.clone()), "robotB");
        let a_changes = a.subscribe();
        assert_eq!(
            subscriber_prefixes(&shared.0),
            [""],
            "the store is asked for everything"
        );
        assert_eq!(
            a_changes.try_recv(),
            Some(StateChange::set("battery_level", Value::Boolean(true)))
        );

        b.write(StateChange::set("battery_level", Value::Boolean(false)))
            .unwrap();
        assert_eq!(a_changes.try_recv(), None, "a neighbour's write");
        a.write(StateChange::set("battery_level", Value::Boolean(false)))
            .unwrap();
        assert_eq!(
            a_changes.try_recv(),
            Some(StateChange::set("battery_level", Value::Boolean(false)))
        );

        let under_a = shared.subscribe_prefix("robotA");
        assert_eq!(
            under_a.try_recv(),
            Some(StateChange::from(vec![
                ("robotA", Value::Boolean(true)),
                ("robotA/battery_level", Value::Boolean(false)),
            ])),
            "the default keeps the subtree, under full paths"
        );
        b.write(StateChange::set("battery_level", Value::Boolean(true)))
            .unwrap();
        assert_eq!(under_a.try_recv(), None);
    }

    /// The empty prefix on a view is the device, not the store: a neighbour's
    /// keys stay closed.
    #[test]
    fn the_empty_prefix_scopes_to_the_namespace() {
        let shared = SimpleDataStore::new();
        let a = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let b = NamespacedStore::new(Arc::new(shared.clone()), "robotB");

        a.set_prefix_meta(HashMap::from([(String::new(), KeyMeta::new().editable())]))
            .unwrap();

        assert!(
            a.meta(&[Key::from("joint1.position")])[0]
                .as_ref()
                .unwrap()
                .editable,
            "the whole device is open"
        );
        assert_eq!(
            b.meta(&[Key::from("joint1.position")]),
            vec![None],
            "the neighbour is untouched"
        );
        assert_eq!(
            shared.meta(&[Key::from("robotAlpha/joint1.position")]),
            vec![None],
            "a namespace covers segments, not letters"
        );
    }
}
