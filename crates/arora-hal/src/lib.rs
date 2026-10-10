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
//! directly from the synchronous step. Any real async work is the
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

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_channel::mpsc::UnboundedSender;
use futures_core::Stream;

use arora_types::data::{Key, State, StateChange};
use arora_types::value::Value;

pub mod hal_module;
mod model;

pub use model::{content_hash, ComponentGlb, ComponentModel, ModelReference, Mount, DEVICE};

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
}

impl FakeInner {
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
/// Its naming contract is the HALs' joint convention, an attribute on the
/// key's last segment ([`Key::get_component`]): a `<joint>.target_position`
/// write is a setpoint, held and sensed back as `<joint>.position`, so a
/// consumer that writes a target sees the measured position follow. Every
/// other key is ignored, the way hardware ignores what it has no actuator for.
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

    /// Give the fake a model: the [`DEVICE`] component's, servable, hashed
    /// from `glb`.
    pub fn set_model_glb(&self, glb: Vec<u8>) {
        self.inner.lock().unwrap().model_glb = Some(glb);
    }

    /// Apply a write synchronously. The fake deals only in setpoints — the keys
    /// whose first attribute is `target_position` — and ignores every other key
    /// in the change, the way hardware ignores what it has no actuator for.
    /// Each setpoint is held and sensed back under the same key with
    /// `position` as its attribute, the fake's joint actuation. Shared by
    /// [`Hal::write`] and [`Hal::try_send`].
    fn apply_write(&self, changes: &StateChange) {
        let mut setpoints = StateChange::new();
        let mut sensed = StateChange::new();
        for (key, value) in &changes.set {
            if key.get_component() == Some("target_position") {
                setpoints.set.insert(key.clone(), value.clone());
                sensed
                    .set
                    .insert(key.clone().with_component("position"), value.clone());
            }
        }
        for key in &changes.unset {
            if key.get_component() == Some("target_position") {
                setpoints.unset.insert(key.clone());
                sensed.unset.insert(key.clone().with_component("position"));
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
    async fn read_absent_is_none() {
        let hal = FakeHal::new();
        assert_eq!(hal.read(&[Key::from("nope")]).await.unwrap(), vec![None]);
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
        assert!(FakeHal::new().assets().is_some());
    }
}
