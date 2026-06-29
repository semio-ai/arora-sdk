//! The Arora Bridge interface.
//!
//! A [`Bridge`] connects an Arora runtime to a remote — in practice Semio Studio
//! over `studio-bridge`. It is modelled on studio-bridge's `device-client`
//! trait: push local state changes out, receive device-info updates and
//! commands in, and learn when a client is asking for data.
//!
//! The trait lives here (lean: `arora-types` + async primitives) so the runtime
//! can depend on the *interface* without depending on `studio-bridge`.
//! studio-bridge keeps its device-client implementations and provides a
//! connector that implements this trait.

use std::pin::Pin;

use async_trait::async_trait;
use futures::channel::oneshot;
use futures::Stream;

use arora_types::call::{Call, CallResult};
use arora_types::data::{Key, StateChange};

/// Neutral device metadata the bridge syncs with the remote. The bridge-flavored
/// wire form (studio-bridge's `PartialDeviceInfo`) is converted to/from this by
/// the connector.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: Option<String>,
    pub description: Option<String>,
    pub model_family: Option<String>,
    pub hardware_version: Option<String>,
    pub software_version: Option<String>,
    pub owners: Vec<String>,
}

/// An operation a remote client asks the device to perform. Mirrors
/// studio-bridge's `AroraOp`.
#[derive(Debug, Clone)]
pub enum BridgeOp {
    /// Read the given keys.
    Get(Vec<Key>),
    /// Apply a state change.
    Update(StateChange),
    /// Call a function.
    Call(Call),
    /// Enumerate store keys under an optional path prefix — introspection for
    /// the live-edit surface. Replies with a [`CallResult`] whose `ret` is an
    /// `ArrayValue` of the matching key paths as `String`s.
    ListKeys {
        /// Only keys whose path starts with this prefix; `None` lists all.
        prefix: Option<String>,
    },
    /// Enumerate callable module methods under an optional name prefix. Replies
    /// with a [`CallResult`] whose `ret` is an `ArrayValue` of method names as
    /// `String`s.
    ListMethods {
        /// Only methods whose name starts with this prefix; `None` lists all.
        prefix: Option<String>,
    },
}

/// Something went wrong on the bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// The link to the remote dropped.
    Disconnected(String),
    /// The device was unregistered from the remote — the runtime should stop.
    Unregistered,
    /// Anything else, with a message.
    Other(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Disconnected(m) => write!(f, "bridge disconnected: {m}"),
            BridgeError::Unregistered => write!(f, "device unregistered from the remote"),
            BridgeError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for BridgeError {}

pub type BridgeResult<T> = Result<T, BridgeError>;

/// A command received from the remote, carrying a one-shot reply channel.
///
/// Process [`op`](BridgeCommand::op), then call [`reply`](BridgeCommand::reply)
/// exactly once with the result (mirrors device-client's
/// `(AroraOp, oneshot::Sender<Result<AroraCallResult, String>>)`).
pub struct BridgeCommand {
    pub op: BridgeOp,
    reply: oneshot::Sender<Result<CallResult, String>>,
}

impl BridgeCommand {
    /// Build a command from an op and its reply channel (for `Bridge` impls).
    pub fn new(op: BridgeOp, reply: oneshot::Sender<Result<CallResult, String>>) -> Self {
        Self { op, reply }
    }

    /// Send the result back to the remote. Ignores a dropped receiver.
    pub fn reply(self, result: Result<CallResult, String>) {
        let _ = self.reply.send(result);
    }
}

/// Stream of device-info updates. `Ok(None)` means the device was unregistered.
pub type DeviceInfoStream = Pin<Box<dyn Stream<Item = BridgeResult<Option<DeviceInfo>>> + Send>>;
/// Stream of the "a client is asking for data" (claim) toggle.
pub type DataRequestedStream = Pin<Box<dyn Stream<Item = bool> + Send>>;
/// Stream of commands from the remote.
pub type CommandStream = Pin<Box<dyn Stream<Item = BridgeCommand> + Send>>;

/// The connection between an Arora runtime and a remote (e.g. Semio Studio).
///
/// Modelled on studio-bridge's `device-client`. Interior-mutable (`&self`) so
/// the runtime can share it across the tasks of its run loop.
#[async_trait]
pub trait Bridge: Send + Sync {
    /// The device's current info, if registered.
    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>>;

    /// A stream of device-info updates from the remote (`Ok(None)` = unregistered).
    async fn device_info_updated(&self) -> BridgeResult<DeviceInfoStream>;

    /// Push updated device info to the remote; returns the merged result.
    async fn update_device_info(
        &self,
        info: Option<DeviceInfo>,
    ) -> BridgeResult<Option<DeviceInfo>>;

    /// A stream that toggles as a client claims/releases interest in the data.
    async fn data_requested(&self) -> DataRequestedStream;

    /// Push a state change out to the remote.
    async fn send_data(&self, data: StateChange) -> BridgeResult<()>;

    /// A stream of commands the remote issues to the device.
    async fn commands(&self) -> CommandStream;
}

/// A no-op [`Bridge`] for tests and offline runs: never registers, never emits
/// updates, commands, or claims, and accepts (drops) any data sent.
#[derive(Clone, Default)]
pub struct FakeBridge;

impl FakeBridge {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Bridge for FakeBridge {
    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
        Ok(None)
    }

    async fn device_info_updated(&self) -> BridgeResult<DeviceInfoStream> {
        Ok(Box::pin(futures::stream::empty()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn fake_bridge_is_usable_as_trait_object() {
        let bridge: Box<dyn Bridge> = Box::new(FakeBridge::new());
        assert_eq!(bridge.get_device_info().await.unwrap(), None);
        bridge.send_data(StateChange::new()).await.unwrap();
        assert!(bridge
            .device_info_updated()
            .await
            .unwrap()
            .next()
            .await
            .is_none());
        assert!(bridge.commands().await.next().await.is_none());
    }

    #[tokio::test]
    async fn command_reply_round_trips() {
        let (tx, rx) = oneshot::channel();
        let cmd = BridgeCommand::new(BridgeOp::Get(vec![Key::from("a")]), tx);
        match &cmd.op {
            BridgeOp::Get(keys) => assert_eq!(keys[0], Key::from("a")),
            _ => panic!("wrong op"),
        }
        cmd.reply(Err("not implemented".to_string()));
        assert!(rx.await.unwrap().is_err());
    }
}
