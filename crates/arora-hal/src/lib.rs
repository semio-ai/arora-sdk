//! The Arora Hardware Abstraction Layer.
//!
//! [`Hal`] is the boundary to a real (or fake) device — the thing studio-bridge
//! called a `Controller`. A HAL reads sensors and reported state, accepts
//! actuator/state writes, and pushes a feed of changes the hardware makes.
//!
//! The Arora runtime mirrors a HAL against a
//! [`DataStore`](arora_types::data::DataStore): HAL updates flow into the store,
//! store writes flow to the HAL. The HAL trait depends only on `arora-types`, so
//! any execution engine can drive it without pulling in the bridge.
//!
//! # The I/O seam
//!
//! Consistent with the bridge, the HAL's data plane is an owned inbound stream
//! and a non-blocking outbound push: [`updates`](Hal::updates) hands out the
//! sensor feed as a [`Stream`] (each call an independent subscription, owned by
//! its consumer — the runtime's loop is its one poller), and
//! [`try_send`](Hal::try_send) pushes actuator writes immediately, called
//! directly from the synchronous step. What the keys are — which ones a remote
//! may write, of what type — the HAL describes as a feed too
//! ([`describe_keys`](Hal::describe_keys)), which the runtime states to the
//! store. Any real async work is the
//! implementation's own responsibility (its own task/queue), the same way a
//! bridge owns its socket; this crate depends on no async runtime.
//!
//! The device owns its HAL (`Box<dyn Hal>` at the builder). An implementation
//! that also serves an observer (a simulator UI, a test double) shares its
//! internals and hands sibling handles out itself — [`FakeHal`] clones onto
//! the same state.
//!
//! Pick an implementation per robot: [`FakeHal`] here (also the test double),
//! and the real ones (ros2, restful, nao) in their own sibling crates.

use std::collections::{BTreeSet, HashMap};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_channel::mpsc::UnboundedSender;
use futures_core::Stream;

use arora_types::data::{Key, KeyMeta, State, StateChange};
use arora_types::value::{Type, Value};

pub mod hal_module;
mod model;
mod target;

pub use model::{content_hash, ComponentGlb, ComponentModel, ModelReference, Mount, DEVICE};
pub use target::{fold_newest_wins, set_targets, target_keys, SetTarget};

/// What device a HAL drives — or, in a [`ComponentModel`], what one of its
/// components is.
#[derive(
    arora_types::AroraType,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
)]
#[arora(id = "0c048a95-3fd8-46dd-875a-9f7f203923bd")]
pub struct HalDescription {
    #[arora(id = "caa29d5f-9760-45d8-ab08-06975713337a")]
    pub model_family: Option<String>,
    #[arora(id = "d7f27d34-759d-4d8f-825c-256207a3301b")]
    pub hardware_version: Option<String>,
    #[arora(id = "c50367c8-0f85-4a3a-ae94-61b3ad5c2124")]
    pub software_version: Option<String>,
}

/// Something went wrong talking to the hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HalError {
    /// The hardware link is broken / unavailable.
    Broken(String),
    /// A key could not be resolved.
    NoSuchKey(String),
    /// Anything else, with a message.
    Other(String),
}

impl std::fmt::Display for HalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HalError::Broken(m) => write!(f, "hardware link broken: {m}"),
            HalError::NoSuchKey(k) => write!(f, "no such key: {k}"),
            HalError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for HalError {}

pub type HalResult<T> = Result<T, HalError>;

/// The Hardware Abstraction Layer: the boundary to a device.
///
/// Interior-mutable (`&self`): the methods take shared references, so an
/// implementation that hands sibling handles to an observer keeps working —
/// the runtime still owns its HAL by value.
#[async_trait]
pub trait Hal: Send + Sync {
    /// Describe the device (model family, versions).
    async fn describe(&self) -> HalDescription;

    /// Read the current values for the given keys. Each entry is `None` if the
    /// key is unset/absent (further nesting lives inside [`Value`]).
    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>>;

    /// Read everything the HAL currently exposes.
    async fn read_all(&self) -> HalResult<State>;

    /// Apply actuator/state changes. Observers of [`updates`](Hal::updates) see
    /// the resulting changes.
    async fn write(&self, changes: StateChange) -> HalResult<()>;

    /// Push actuator/state changes toward the hardware immediately, without
    /// blocking — the outbound counterpart to the [`updates`](Hal::updates)
    /// sensor feed, and the shape the synchronous runtime step calls directly
    /// (mirroring `Bridge::try_send`).
    ///
    /// **Must not block.** A HAL whose apply is truly immediate (an in-memory
    /// fake, a cache write) applies here directly; a HAL that performs real
    /// async I/O (HTTP, DDS) enqueues onto its own internal task and returns.
    /// There is deliberately no default: every implementation decides how its
    /// hardware absorbs a non-blocking push.
    fn try_send(&self, changes: &StateChange);

    /// A feed of changes the hardware reports (sensors, mirrored actuation, …).
    /// Each call yields an independent, owned stream; the consumer that takes
    /// it is its one poller (natively the runtime's `run` select, on the web
    /// the per-frame sweep).
    fn updates(&self) -> UpdatesStream;

    /// What this HAL's keys are. Its inputs — the keys it reads from a write,
    /// such as actuator targets — are open to remote writers
    /// ([`KeyMeta::editable`]); its outputs — what it reports, such as sensed
    /// values — are closed. Each states its type, and its unit, range and rest
    /// value where the HAL knows them.
    ///
    /// The description is a feed, because a HAL may learn its keys after it
    /// starts: a topic discovered, a configuration fetched. Each call yields an
    /// independent stream. Its first item, ready at once, describes the keys
    /// the HAL knows then; a later item describes keys it has learnt, or keys
    /// anew. An item adds to the description and replaces what an earlier item
    /// said of the same key; no item retracts a key. A HAL whose keys are fixed
    /// yields one item.
    ///
    /// The runtime states each item to the device's store
    /// ([`DataStore::set_meta`](arora_types::data::DataStore::set_meta)) — the
    /// items ready when the device is built, then each later one at the next
    /// step — so every bridge lists the keys with their meta and accepts a
    /// remote write to an input, with no declaration of its own. A key the HAL
    /// does not describe keeps whatever the store says of it, and so does a key
    /// whose own meta the device's host stated: the host has the last word on
    /// what its device opens.
    ///
    /// Default: no item, a HAL that describes no key.
    fn describe_keys(&self) -> KeyDescriptions {
        Box::pin(futures_util::stream::empty())
    }

    /// The device's assets — its 3D model — when this HAL supplies them. The
    /// runtime serves them as the HAL module's functions
    /// ([`hal_module`]), so a HAL that implements [`HalAssets`] returns
    /// `Some(self)` here. Default: `None`, a device with no model.
    fn assets(&self) -> Option<&dyn HalAssets> {
        None
    }
}

/// The sensor feed of a [`Hal`]: an owned stream of the changes the hardware
/// reports. The stream ending means the hardware feed is gone.
pub type UpdatesStream = Pin<Box<dyn Stream<Item = StateChange> + Send>>;

/// The key descriptions of a [`Hal`] ([`describe_keys`](Hal::describe_keys)):
/// an owned stream of statements, each the meta of some of its keys, in the
/// shape [`DataStore::set_meta`](arora_types::data::DataStore::set_meta)
/// takes.
pub type KeyDescriptions = Pin<Box<dyn Stream<Item = HashMap<Key, KeyMeta>> + Send>>;

/// Optional extension: the 3D models of a HAL's components. A HAL exposes it to
/// the runtime through [`Hal::assets`], and the runtime serves it as the HAL
/// module's functions ([`hal_module`]).
///
/// A device's models are fixed for its life: a different model means a
/// restart. So both methods answer from what the HAL was built with, at once —
/// the bytes it holds or a quick local read — and are called from the
/// runtime's synchronous step.
#[async_trait]
pub trait HalAssets: Send + Sync {
    /// The model of each component that has one. A HAL that is not composed
    /// states at most one, under [`DEVICE`]. Default: none.
    fn models(&self) -> Vec<ComponentModel> {
        Vec::new()
    }

    /// The GLB of `component`'s model, when [`models`](HalAssets::models)
    /// states it [`servable`](ComponentModel::servable); `None` otherwise.
    /// The runtime asks only for a servable model. Default: none.
    fn servable_glb(&self, component: &str) -> HalResult<Option<Vec<u8>>> {
        let _ = component;
        Ok(None)
    }

    /// The device's one model, as GLB bytes. The runtime serves
    /// [`models`](HalAssets::models) and
    /// [`servable_glb`](HalAssets::servable_glb) instead; it never calls this.
    #[deprecated(note = "state the components' models with `models` and `servable_glb`")]
    async fn model_glb(&self) -> HalResult<Option<Vec<u8>>> {
        Ok(None)
    }
}

#[derive(Default)]
struct FakeInner {
    description: HalDescription,
    model_glb: Option<Vec<u8>>,
    state: State,
    subscribers: Vec<UnboundedSender<StateChange>>,
    joints: BTreeSet<String>,
    key_subscribers: Vec<UnboundedSender<HashMap<Key, KeyMeta>>>,
}

impl FakeInner {
    /// The meta of the keys of `joints`: for each, its position target in
    /// both forms, an `F64` input, and its sensed position, an `F64` output.
    fn describe<'j>(joints: impl IntoIterator<Item = &'j String>) -> HashMap<Key, KeyMeta> {
        let target = KeyMeta::new().editable().of_type(Type::F64);
        let sensed = KeyMeta::new().of_type(Type::F64);
        joints
            .into_iter()
            .flat_map(|joint| {
                let [entity_form, prefixed_form] = target_keys(joint, "position");
                [
                    (entity_form, target.clone()),
                    (prefixed_form, target.clone()),
                    (Key::new(format!("{joint}.position")), sensed.clone()),
                ]
            })
            .collect()
    }

    fn notify(&mut self, change: &StateChange) {
        if change.is_empty() {
            return;
        }
        self.subscribers
            .retain(|tx| tx.unbounded_send(change.clone()).is_ok());
    }
}

/// An in-memory fake [`Hal`] for tests and simulators.
///
/// Its naming contract is the HALs' target keys ([`set_targets`]): a
/// `<joint>/target.position` or `<joint>.target_position` write is a setpoint,
/// held and sensed back as `<joint>.position`, so a consumer that writes a
/// target sees the measured position follow. Every other key is ignored, the
/// way hardware ignores what it has no actuator for.
///
/// It describes the keys of the joints it is given
/// ([`add_joints`](FakeHal::add_joints)): their position targets as inputs,
/// their sensed positions as outputs. A joint it was not given still follows
/// its setpoint, undescribed.
///
/// Cheaply cloneable; clones share the same state. (Backed by [`State`], the
/// trivial owned state type.)
#[derive(Clone, Default)]
pub struct FakeHal {
    inner: Arc<Mutex<FakeInner>>,
}

impl FakeHal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_description(description: HalDescription) -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeInner {
                description,
                ..Default::default()
            })),
        }
    }

    /// Give the fake these joints: it describes their keys
    /// ([`Hal::describe_keys`]) — for each joint, `<joint>/target.position` and
    /// `<joint>.target_position` as `F64` inputs, and `<joint>.position` as an
    /// `F64` output. Joints added once the fake is described reach every
    /// description feed as one more item, the way a HAL that discovers its
    /// joints describes them. A name no target can name — one with a `.`, or
    /// an empty last segment ([`target_keys`]) — is no joint, and is skipped.
    pub fn add_joints(&self, joints: impl IntoIterator<Item = impl Into<String>>) {
        let mut inner = self.inner.lock().unwrap();
        let added: Vec<String> = joints
            .into_iter()
            .map(Into::into)
            .filter(|joint| target::is_target_entity(joint))
            .filter(|joint| inner.joints.insert(joint.clone()))
            .collect();
        if added.is_empty() {
            return;
        }
        let description = FakeInner::describe(&added);
        inner
            .key_subscribers
            .retain(|tx| tx.unbounded_send(description.clone()).is_ok());
    }

    /// Give the fake a model: the [`DEVICE`] component's, servable, hashed
    /// from `glb`.
    pub fn set_model_glb(&self, glb: Vec<u8>) {
        self.inner.lock().unwrap().model_glb = Some(glb);
    }

    /// Apply a write synchronously. The fake deals only in position setpoints
    /// — the keys `<joint>/target.position` and `<joint>.target_position` —
    /// and ignores every other key in the change, the way hardware ignores
    /// what it has no actuator for. Each setpoint is held as written and
    /// sensed back as `<joint>.position`, the fake's joint actuation; a change
    /// that sets both keys of one joint applies `<joint>/target.position`
    /// ([`set_targets`]). An unset setpoint unsets the sensed position, unless
    /// the same change sets the joint's other key. Shared by [`Hal::write`]
    /// and [`Hal::try_send`].
    fn apply_write(&self, changes: &StateChange) {
        let mut setpoints = StateChange::new();
        let mut sensed = StateChange::new();
        // The setpoint key each sensed position follows in this change.
        let mut sensed_from: HashMap<Key, Key> = HashMap::new();
        for target in set_targets(changes) {
            if target.component() == "position" {
                setpoints
                    .set
                    .insert(target.key.clone(), target.value.clone());
                sensed_from.insert(target.measured.clone(), target.key.clone());
                sensed.set.insert(target.measured, target.value.clone());
            }
        }
        for key in &changes.unset {
            let Some(target) = target::Target::parse(key) else {
                continue;
            };
            if target.measured().get_component() == Some("position") {
                setpoints.unset.insert(key.clone());
                let set_by_other_key = sensed_from
                    .get(target.measured())
                    .is_some_and(|from| from != key);
                if !set_by_other_key {
                    sensed.set.remove(target.measured());
                    sensed.unset.insert(target.measured().clone());
                }
            }
        }
        if setpoints.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.state.apply(setpoints);
        inner.state.apply(sensed.clone());
        inner.notify(&sensed);
    }
}

#[async_trait]
impl Hal for FakeHal {
    async fn describe(&self) -> HalDescription {
        self.inner.lock().unwrap().description.clone()
    }

    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
        let inner = self.inner.lock().unwrap();
        Ok(keys
            .iter()
            .map(|k| inner.state.get(k).cloned().flatten())
            .collect())
    }

    async fn read_all(&self) -> HalResult<State> {
        Ok(self.inner.lock().unwrap().state.clone())
    }

    async fn write(&self, changes: StateChange) -> HalResult<()> {
        self.apply_write(&changes);
        Ok(())
    }

    /// Synchronous, immediate apply — the fake never blocks, so it needs no task
    /// of its own (unlike a real HTTP/DDS HAL).
    fn try_send(&self, changes: &StateChange) {
        self.apply_write(changes);
    }

    fn updates(&self) -> UpdatesStream {
        let (tx, rx) = futures_channel::mpsc::unbounded();
        self.inner.lock().unwrap().subscribers.push(tx);
        Box::pin(rx)
    }

    fn describe_keys(&self) -> KeyDescriptions {
        let (tx, rx) = futures_channel::mpsc::unbounded();
        let mut inner = self.inner.lock().unwrap();
        // The opening item, even when empty: the keys the fake knows now. The
        // receiver is held here, so the send cannot fail.
        let _ = tx.unbounded_send(FakeInner::describe(&inner.joints));
        inner.key_subscribers.retain(|tx| !tx.is_closed());
        inner.key_subscribers.push(tx);
        Box::pin(rx)
    }

    fn assets(&self) -> Option<&dyn HalAssets> {
        Some(self)
    }
}

#[async_trait]
impl HalAssets for FakeHal {
    fn models(&self) -> Vec<ComponentModel> {
        let inner = self.inner.lock().unwrap();
        inner
            .model_glb
            .as_deref()
            .map(|glb| ComponentModel {
                component: DEVICE.to_string(),
                description: Some(inner.description.clone()),
                reference: None,
                content_hash: Some(content_hash(glb)),
                servable: true,
                mount: None,
            })
            .into_iter()
            .collect()
    }

    fn servable_glb(&self, component: &str) -> HalResult<Option<Vec<u8>>> {
        Ok(match component {
            DEVICE => self.inner.lock().unwrap().model_glb.clone(),
            _ => None,
        })
    }

    async fn model_glb(&self) -> HalResult<Option<Vec<u8>>> {
        Ok(self.inner.lock().unwrap().model_glb.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, StreamExt};

    /// Drain everything the feed has already buffered, without blocking.
    fn drain(feed: &mut UpdatesStream) -> Vec<StateChange> {
        let mut out = Vec::new();
        while let Some(Some(change)) = feed.next().now_or_never() {
            out.push(change);
        }
        out
    }

    #[tokio::test]
    async fn fake_mirrors_target_position() {
        let hal = FakeHal::new();
        let mut sub = hal.updates();
        hal.write(StateChange::set("joint1.target_position", Value::from(1.0)))
            .await
            .unwrap();
        // measured position mirrors the target
        assert_eq!(
            hal.read(&[Key::from("joint1.position")]).await.unwrap(),
            vec![Some(Value::from(1.0))]
        );
        // a subscriber saw the mirrored position change
        let saw_position = drain(&mut sub)
            .iter()
            .any(|c| c.contains(&Key::from("joint1.position")));
        assert!(saw_position);
    }

    #[tokio::test]
    async fn fake_mirrors_the_entity_form_target() {
        let hal = FakeHal::new();
        let mut sub = hal.updates();
        hal.write(StateChange::set("joint1/target.position", Value::from(1.0)))
            .await
            .unwrap();
        assert_eq!(
            hal.read(&[
                Key::from("joint1.position"),
                Key::from("joint1/target.position")
            ])
            .await
            .unwrap(),
            vec![Some(Value::from(1.0)), Some(Value::from(1.0))]
        );
        let changes = drain(&mut sub);
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0].set,
            HashMap::from([(Key::from("joint1.position"), Some(Value::from(1.0)))])
        );
    }

    /// A change that sets both forms of one target applies the entity form's
    /// value, whatever order the change holds them in (each fresh `HashMap`
    /// has its own).
    #[tokio::test]
    async fn fake_applies_the_entity_form_when_a_change_sets_both() {
        for _ in 0..32 {
            let hal = FakeHal::new();
            let mut change = StateChange::new();
            change
                .set
                .insert(Key::from("joint1.target_position"), Some(Value::from(1.0)));
            change
                .set
                .insert(Key::from("joint1/target.position"), Some(Value::from(2.0)));
            hal.write(change).await.unwrap();
            assert_eq!(
                hal.read(&[
                    Key::from("joint1.position"),
                    Key::from("joint1/target.position"),
                    Key::from("joint1.target_position"),
                ])
                .await
                .unwrap(),
                vec![Some(Value::from(2.0)), Some(Value::from(2.0)), None]
            );
        }
    }

    /// A change that sets and unsets the same setpoint applies the unset, as
    /// applying a change does: sets first, then unsets.
    #[tokio::test]
    async fn fake_set_and_unset_of_one_key_unsets() {
        let hal = FakeHal::new();
        let mut change = StateChange::set("joint1.target_position", Value::from(1.0));
        change.unset.insert(Key::from("joint1.target_position"));
        hal.write(change).await.unwrap();
        assert_eq!(
            hal.read(&[
                Key::from("joint1.position"),
                Key::from("joint1.target_position")
            ])
            .await
            .unwrap(),
            vec![None, None]
        );
    }

    #[tokio::test]
    async fn fake_unsets_the_sensed_position_with_either_form() {
        let hal = FakeHal::new();
        for (target, other) in [
            ("joint1/target.position", "joint1.target_position"),
            ("joint1.target_position", "joint1/target.position"),
        ] {
            hal.write(StateChange::set(target, Value::from(1.0)))
                .await
                .unwrap();
            let mut unset = StateChange::new();
            unset.unset.insert(Key::from(target));
            hal.write(unset).await.unwrap();
            assert_eq!(
                hal.read(&[Key::from("joint1.position"), Key::from(target)])
                    .await
                    .unwrap(),
                vec![None, None]
            );

            // An unset beside a set of the joint's other key keeps the
            // sensed position the set writes.
            let mut both = StateChange::set(other, Value::from(3.0));
            both.unset.insert(Key::from(target));
            hal.write(both).await.unwrap();
            assert_eq!(
                hal.read(&[Key::from("joint1.position")]).await.unwrap(),
                vec![Some(Value::from(3.0))]
            );
            let mut clear = StateChange::new();
            clear.unset.insert(Key::from(other));
            hal.write(clear).await.unwrap();
        }
    }

    #[test]
    fn try_send_applies_synchronously_and_mirrors() {
        // The synchronous seam the runtime's step calls: no async, immediate.
        let hal = FakeHal::new();
        let mut sub = hal.updates();
        hal.try_send(&StateChange::set(
            "joint1.target_position",
            Value::from(1.0),
        ));
        let saw_position = drain(&mut sub)
            .iter()
            .any(|c| c.contains(&Key::from("joint1.position")));
        assert!(
            saw_position,
            "try_send should mirror target to measured position"
        );
    }

    #[tokio::test]
    async fn describe_and_glb() {
        let hal = FakeHal::with_description(HalDescription {
            model_family: Some("test".into()),
            ..Default::default()
        });
        assert_eq!(hal.describe().await.model_family.as_deref(), Some("test"));
        assert!(hal.models().is_empty());
        hal.set_model_glb(vec![1, 2, 3]);
        let models = hal.models();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].component, DEVICE);
        assert_eq!(models[0].content_hash, Some(content_hash(&[1, 2, 3])));
        assert!(models[0].servable);
        assert_eq!(hal.servable_glb(DEVICE).unwrap(), Some(vec![1, 2, 3]));
        assert_eq!(hal.servable_glb("arm").unwrap(), None);
    }

    /// Drain every description the feed has already buffered, and whether it
    /// has ended.
    fn drain_keys(feed: &mut KeyDescriptions) -> (Vec<HashMap<Key, KeyMeta>>, bool) {
        let mut out = Vec::new();
        loop {
            match feed.next().now_or_never() {
                Some(Some(description)) => out.push(description),
                Some(None) => return (out, true),
                None => return (out, false),
            }
        }
    }

    #[test]
    fn the_fake_describes_its_joints_targets_as_inputs_and_positions_as_outputs() {
        let hal = FakeHal::new();
        hal.add_joints(["head_yaw", "arm/elbow"]);
        let (descriptions, ended) = drain_keys(&mut hal.describe_keys());
        assert!(!ended, "the fake can describe more joints later");
        assert_eq!(descriptions.len(), 1, "one opening item");
        let input = KeyMeta::new().editable().of_type(Type::F64);
        let output = KeyMeta::new().of_type(Type::F64);
        assert_eq!(
            descriptions[0],
            HashMap::from([
                (Key::from("head_yaw/target.position"), input.clone()),
                (Key::from("head_yaw.target_position"), input.clone()),
                (Key::from("head_yaw.position"), output.clone()),
                (Key::from("arm/elbow/target.position"), input.clone()),
                (Key::from("arm/elbow.target_position"), input),
                (Key::from("arm/elbow.position"), output),
            ])
        );
    }

    #[test]
    fn joints_added_later_reach_an_open_feed_as_one_more_item() {
        let hal = FakeHal::new();
        let mut feed = hal.describe_keys();
        assert_eq!(drain_keys(&mut feed).0, vec![HashMap::new()]);
        hal.add_joints(["joint1"]);
        hal.add_joints(["joint1", "not.a.joint", ""]);
        let (later, _) = drain_keys(&mut feed);
        assert_eq!(
            later.len(),
            1,
            "a joint already described, or no joint, sends nothing"
        );
        assert_eq!(
            later[0].keys().map(Key::get_path).collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "joint1/target.position",
                "joint1.target_position",
                "joint1.position"
            ])
        );
    }

    #[test]
    fn a_hal_without_assets_has_none() {
        struct Bare;
        #[async_trait]
        impl Hal for Bare {
            async fn describe(&self) -> HalDescription {
                HalDescription::default()
            }
            async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
                Ok(vec![None; keys.len()])
            }
            async fn read_all(&self) -> HalResult<State> {
                Ok(State::default())
            }
            async fn write(&self, _changes: StateChange) -> HalResult<()> {
                Ok(())
            }
            fn try_send(&self, _changes: &StateChange) {}
            fn updates(&self) -> UpdatesStream {
                Box::pin(futures::stream::empty())
            }
        }
        assert!(Bare.assets().is_none());
        assert_eq!(drain_keys(&mut Bare.describe_keys()), (Vec::new(), true));
        assert!(FakeHal::new().assets().is_some());
    }
}
