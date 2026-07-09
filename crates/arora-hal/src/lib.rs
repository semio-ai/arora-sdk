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
//! # Synchronous I/O seam
//!
//! Consistent with the bridge, the runtime drives the HAL synchronously: the
//! inbound sensor drain is [`updates`](Hal::updates) (a sync-pollable
//! [`Subscription`] the step loop `try_recv`s), and the outbound actuator push
//! is [`try_send`](Hal::try_send) — non-blocking, called directly from the
//! synchronous step. Any real async work is the implementation's own
//! responsibility (its own task/queue), the same way a bridge owns its socket.
//!
//! Pick an implementation per robot: [`FakeHal`] here (also the test double),
//! and the real ones (ros2, restful, nao) in their own sibling crates.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use arora_types::data::{Key, State, StateChange, Subscription};
use arora_types::value::Value;

/// What device a HAL drives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HalDescription {
    pub model_family: Option<String>,
    pub hardware_version: Option<String>,
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
/// Interior-mutable (`&self`) so the runtime can share one HAL across tasks,
/// the same way a [`DataStore`](arora_types::data::DataStore) is shared.
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
    /// sensor drain, and the shape the synchronous runtime step calls directly
    /// (mirroring `Bridge::try_send`).
    ///
    /// The default forwards to [`write`](Hal::write), which suits HALs whose
    /// write does not truly block (in-memory fakes, cache-only writes). A HAL
    /// that performs real async I/O (HTTP, DDS) should override this to enqueue
    /// onto its own task so the caller's synchronous step loop never blocks on
    /// the hardware.
    fn try_send(&self, changes: &StateChange) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = futures::executor::block_on(self.write(changes.clone()));
        }
        // On wasm there are no threads to park on; a wasm HAL (an in-process
        // fake) overrides this with a synchronous apply.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = changes;
        }
    }

    /// A feed of changes the hardware reports (sensors, mirrored actuation, …).
    fn updates(&self) -> Subscription;
}

/// Optional extension: HALs that can supply a 3D model (GLB) of the device.
#[async_trait]
pub trait HalAssets: Send + Sync {
    async fn model_glb(&self) -> HalResult<Option<Vec<u8>>>;
}

#[derive(Default)]
struct FakeInner {
    description: HalDescription,
    model_glb: Option<Vec<u8>>,
    state: State,
    subscribers: Vec<Sender<StateChange>>,
}

impl FakeInner {
    fn notify(&mut self, change: &StateChange) {
        if change.is_empty() {
            return;
        }
        self.subscribers
            .retain(|tx| tx.send(change.clone()).is_ok());
    }
}

/// An in-memory fake [`Hal`] for tests and simulators.
///
/// Echoes writes back as state, and fakes joint actuation by mirroring any
/// `*.target_position` write to the corresponding `*.position` (so a consumer
/// that writes a target sees the measured position follow). Cheaply cloneable;
/// clones share the same state. (Backed by [`State`](arora_types::data::State),
/// the trivial owned state type.)
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

    pub fn set_model_glb(&self, glb: Vec<u8>) {
        self.inner.lock().unwrap().model_glb = Some(glb);
    }

    /// Apply a write synchronously: store it, echo it to subscribers, and fake
    /// joint actuation by mirroring any `*.target_position` to `*.position`.
    /// Shared by [`Hal::write`] and [`Hal::try_send`].
    fn apply_write(&self, changes: &StateChange) {
        if changes.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.state.apply(changes.clone());
        inner.notify(changes);

        // Fake joint actuation: mirror "*.target_position" to "*.position".
        let mut mirrored = StateChange::new();
        for (key, value) in &changes.set {
            if key.get_component() == Some("target_position") {
                mirrored
                    .set
                    .insert(key.clone().with_component("position"), value.clone());
            }
        }
        if !mirrored.is_empty() {
            inner.state.apply(mirrored.clone());
            inner.notify(&mirrored);
        }
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

    fn updates(&self) -> Subscription {
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.lock().unwrap().subscribers.push(tx);
        Subscription::new(rx)
    }
}

#[async_trait]
impl HalAssets for FakeHal {
    async fn model_glb(&self) -> HalResult<Option<Vec<u8>>> {
        Ok(self.inner.lock().unwrap().model_glb.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_mirrors_target_position() {
        let hal = FakeHal::new();
        let sub = hal.updates();
        hal.write(StateChange::set("joint1.target_position", Value::from(1.0)))
            .await
            .unwrap();
        // measured position mirrors the target
        assert_eq!(
            hal.read(&[Key::from("joint1.position")]).await.unwrap(),
            vec![Some(Value::from(1.0))]
        );
        // a subscriber saw the mirrored position change
        let saw_position = sub
            .try_iter()
            .any(|c| c.contains(&Key::from("joint1.position")));
        assert!(saw_position);
    }

    #[test]
    fn try_send_applies_synchronously_and_mirrors() {
        // The synchronous seam the runtime's step calls: no async, immediate.
        let hal = FakeHal::new();
        let sub = hal.updates();
        hal.try_send(&StateChange::set("joint1.target_position", Value::from(1.0)));
        let saw_position = sub
            .try_iter()
            .any(|c| c.contains(&Key::from("joint1.position")));
        assert!(saw_position, "try_send should mirror target to measured position");
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
        hal.set_model_glb(vec![1, 2, 3]);
        assert_eq!(hal.model_glb().await.unwrap(), Some(vec![1, 2, 3]));
    }
}
