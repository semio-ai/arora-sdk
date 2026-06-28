//! The Arora runtime loop — studio-bridge's `engine`, library side.
//!
//! A [`Runtime`] wires the three Arora interfaces together around a shared
//! [`DataStore`] blackboard:
//!
//! - the **HAL** ([`arora_hal::Hal`]) pushes sensor/state changes, which the
//!   loop mirrors into the store;
//! - local store changes are mirrored out to the **bridge**
//!   ([`arora_bridge::Bridge`]) and on to the remote (Studio);
//! - the bridge's device-info stream drives the lifecycle (a `None` means the
//!   device was unregistered → the runtime stops).
//!
//! The store is `Send + Sync`, so it is the hub between this async world and the
//! (single-threaded, `!Send`) engine/behavior-tree world. Wiring the engine and
//! BT to read/write the store, and handling inbound commands (Get/Update/Call),
//! is the next slice; for now inbound commands are rejected.
//!
//! The store's change feed is a synchronous [`Subscription`](arora_types::data::Subscription)
//! (so `arora-types` stays runtime-free); the loop bridges it to the async world
//! with a `spawn_blocking` forwarder.

use std::sync::Arc;

use arora_bridge::{Bridge, BridgeError, FakeBridge};
use arora_hal::{FakeHal, Hal};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, StateChange};
use futures::StreamExt;

/// Something went wrong running the loop.
#[derive(Debug)]
pub enum RuntimeError {
    /// The device was unregistered from the remote — the runtime stopped.
    Unregistered,
    /// A write to the data store failed.
    Store(String),
    /// The bridge failed.
    Bridge(BridgeError),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::Unregistered => write!(f, "device unregistered from the remote"),
            RuntimeError::Store(m) => write!(f, "data store error: {m}"),
            RuntimeError::Bridge(e) => write!(f, "bridge error: {e}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// An Arora runtime: a [`DataStore`] blackboard wired to a [`Hal`] and a
/// [`Bridge`]. Build one with [`Runtime::builder`].
pub struct Runtime {
    store: Arc<dyn DataStore>,
    hal: Box<dyn Hal>,
    bridge: Box<dyn Bridge>,
}

/// Builder for [`Runtime`]. Unset pieces default to the in-process fakes
/// ([`SimpleDataStore`], [`FakeHal`], [`FakeBridge`]).
#[derive(Default)]
pub struct RuntimeBuilder {
    store: Option<Arc<dyn DataStore>>,
    hal: Option<Box<dyn Hal>>,
    bridge: Option<Box<dyn Bridge>>,
}

impl RuntimeBuilder {
    pub fn with_data_store(mut self, store: Arc<dyn DataStore>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn with_hal(mut self, hal: Box<dyn Hal>) -> Self {
        self.hal = Some(hal);
        self
    }

    pub fn with_bridge(mut self, bridge: Box<dyn Bridge>) -> Self {
        self.bridge = Some(bridge);
        self
    }

    pub fn build(self) -> Runtime {
        Runtime {
            store: self
                .store
                .unwrap_or_else(|| Arc::new(SimpleDataStore::new())),
            hal: self.hal.unwrap_or_else(|| Box::new(FakeHal::new())),
            bridge: self.bridge.unwrap_or_else(|| Box::new(FakeBridge::new())),
        }
    }
}

impl Runtime {
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }

    /// Run the loop until the device is unregistered or a part fails.
    pub async fn run(self) -> Result<(), RuntimeError> {
        let Runtime { store, hal, bridge } = self;

        // HAL change feed (sync channel) -> async, mirrored into the store.
        let hal_updates = hal.updates();
        let (hal_tx, mut hal_rx) = tokio::sync::mpsc::channel::<StateChange>(64);
        tokio::task::spawn_blocking(move || {
            while let Some(change) = hal_updates.recv() {
                if hal_tx.blocking_send(change).is_err() {
                    break;
                }
            }
        });

        // Store change feed (sync channel) -> async, mirrored out to the remote.
        let store_sub = store.subscribe();
        let (store_tx, mut store_rx) = tokio::sync::mpsc::channel::<StateChange>(64);
        tokio::task::spawn_blocking(move || {
            while let Some(change) = store_sub.recv() {
                if store_tx.blocking_send(change).is_err() {
                    break;
                }
            }
        });

        let mut commands = bridge.commands().await;
        let mut device_info = bridge
            .device_info_updated()
            .await
            .map_err(RuntimeError::Bridge)?;

        loop {
            tokio::select! {
                // sensor/state changes from the HAL -> the blackboard
                Some(change) = hal_rx.recv() => {
                    store.write(change).map_err(|e| RuntimeError::Store(e.to_string()))?;
                }
                // local blackboard changes -> the remote (Studio)
                Some(change) = store_rx.recv() => {
                    bridge.send_data(change).await.map_err(RuntimeError::Bridge)?;
                }
                // commands from the remote
                Some(cmd) = commands.next() => {
                    // TODO(next slice): Get/Update via the store, Call via the engine.
                    cmd.reply(Err("command handling not yet wired".to_string()));
                }
                // device-info lifecycle: None means unregistered → stop
                Some(info) = device_info.next() => {
                    match info {
                        Ok(None) => return Err(RuntimeError::Unregistered),
                        Ok(Some(_info)) => { /* TODO(next slice): apply device info */ }
                        Err(e) => return Err(RuntimeError::Bridge(e)),
                    }
                }
                else => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_bridge::{
        BridgeResult, CommandStream, DataRequestedStream, DeviceInfo, DeviceInfoStream,
    };
    use async_trait::async_trait;

    /// A bridge that immediately reports the device as unregistered.
    struct UnregisterBridge;

    #[async_trait]
    impl Bridge for UnregisterBridge {
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn device_info_updated(&self) -> BridgeResult<DeviceInfoStream> {
            Ok(Box::pin(futures::stream::once(async { Ok(None) })))
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
        async fn data_requested(&self) -> DataRequestedStream {
            Box::pin(futures::stream::empty())
        }
        async fn send_data(&self, _data: StateChange) -> BridgeResult<()> {
            Ok(())
        }
        async fn commands(&self) -> CommandStream {
            Box::pin(futures::stream::empty())
        }
    }

    #[tokio::test]
    async fn stops_when_unregistered() {
        let runtime = Runtime::builder()
            .with_bridge(Box::new(UnregisterBridge))
            .build();
        let err = runtime.run().await.unwrap_err();
        assert!(matches!(err, RuntimeError::Unregistered));
    }

    #[test]
    fn builder_defaults_to_fakes() {
        // Builds with all defaults (SimpleDataStore + FakeHal + FakeBridge).
        let _ = Runtime::builder().build();
    }
}
