//! A worked example of a **device-specific Arora**, built entirely from the
//! outside.
//!
//! Run it with `cargo run -p arora --example device`. There is no fork of
//! `arora` and no per-device feature flag inside it: this example just uses the
//! `arora` crate, provides its own [`Hal`] implementation, and hands it to
//! [`arora::run_with`]. Swap [`ExampleHal`] for a real robot HAL (and
//! [`FakeBridge`] for the studio-bridge connector) and you have a real device
//! build — same `arora` runtime, your hardware.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use arora_bridge::FakeBridge;
use arora_hal::{Hal, HalDescription, HalResult};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{Key, State, StateChange, Subscription};
use arora_types::value::Value;
use async_trait::async_trait;

/// A minimal custom HAL — the one thing a device-specific Arora must supply.
///
/// This one just keeps an in-memory blackboard: it stores writes and echoes them
/// to subscribers. A real HAL would instead talk to the device (read sensors,
/// command actuators) in these same five methods.
#[derive(Default)]
struct ExampleHal {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    state: State,
    subscribers: Vec<Sender<StateChange>>,
}

#[async_trait]
impl Hal for ExampleHal {
    async fn describe(&self) -> HalDescription {
        HalDescription {
            model_family: Some("example-device".to_string()),
            hardware_version: Some("0.1".to_string()),
            software_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }
    }

    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
        let inner = self.inner.lock().unwrap();
        Ok(keys
            .iter()
            .map(|key| inner.state.get(key).cloned().flatten())
            .collect())
    }

    async fn read_all(&self) -> HalResult<State> {
        Ok(self.inner.lock().unwrap().state.clone())
    }

    async fn write(&self, changes: StateChange) -> HalResult<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut inner = self.inner.lock().unwrap();
        inner.state.apply(changes.clone());
        // Tell observers what the "hardware" now reports.
        inner
            .subscribers
            .retain(|tx| tx.send(changes.clone()).is_ok());
        Ok(())
    }

    fn updates(&self) -> Subscription {
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.lock().unwrap().subscribers.push(tx);
        Subscription::new(rx)
    }
}

fn main() -> Result<()> {
    // The whole device-specific build: run the standard arora runtime with our
    // HAL and a bridge, injected from here, over a fresh private data store.
    // (Use the studio-bridge connector's `ZenohDeviceClient` in place of
    // `FakeBridge` to reach Semio Studio.)
    arora::run_with(
        Arc::new(ExampleHal::default()),
        Arc::new(FakeBridge::new()),
        Arc::new(SimpleDataStore::new()),
    )
}
