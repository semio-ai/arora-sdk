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

/// The device operator's answer to an [`AccessRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    Allowed,
    Rejected,
}

/// A remote client asking permission to interact with the device — e.g. a
/// Studio client wanting to join the device's session (Studio's "session
/// joining access control").
///
/// The bridge surfaces these through [`Bridge::access_requests`]; whoever
/// consumes the stream (an operator UI, a headless auto-approval policy)
/// answers with [`respond`](AccessRequest::respond). Dropping the request
/// unanswered leaves the decision to the bridge, which typically treats it as
/// a rejection.
pub struct AccessRequest {
    /// The requesting client's session id (e.g. the Studio client id).
    pub client_id: String,
    /// The user behind the client (e.g. the Firebase user id found in the
    /// device's permission lists), when the transport carries it.
    pub user_id: Option<String>,
    /// What is being requested, human-readable and ready to embed in a prompt
    /// — e.g. "join the session", "claim the device".
    pub permission: String,
    responder: oneshot::Sender<AccessDecision>,
}

impl AccessRequest {
    /// Build a request plus the receiver its decision arrives on (for `Bridge`
    /// impls).
    pub fn new(
        client_id: impl Into<String>,
        user_id: Option<String>,
        permission: impl Into<String>,
    ) -> (Self, oneshot::Receiver<AccessDecision>) {
        let (responder, rx) = oneshot::channel();
        (
            Self {
                client_id: client_id.into(),
                user_id,
                permission: permission.into(),
                responder,
            },
            rx,
        )
    }

    /// Answer the request. Ignores a dropped receiver.
    pub fn respond(self, decision: AccessDecision) {
        let _ = self.responder.send(decision);
    }
}

impl std::fmt::Debug for AccessRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessRequest")
            .field("client_id", &self.client_id)
            .field("user_id", &self.user_id)
            .field("permission", &self.permission)
            .finish_non_exhaustive()
    }
}

/// Stream of device-info updates. `Ok(None)` means the device was unregistered.
pub type DeviceInfoStream = Pin<Box<dyn Stream<Item = BridgeResult<Option<DeviceInfo>>> + Send>>;
/// Stream of the "a client is asking for data" (claim) toggle.
pub type DataRequestedStream = Pin<Box<dyn Stream<Item = bool> + Send>>;
/// Stream of commands from the remote.
pub type CommandStream = Pin<Box<dyn Stream<Item = BridgeCommand> + Send>>;
/// Stream of access requests from remote clients.
pub type AccessRequestStream = Pin<Box<dyn Stream<Item = AccessRequest> + Send>>;

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

    /// The device's identity on the remote (e.g. its Firestore document id),
    /// when the bridge has one. Defaults to `None` for bridges without a
    /// backend identity.
    async fn device_id(&self) -> Option<String> {
        None
    }

    /// A stream of [`AccessRequest`]s from remote clients — Studio's "session
    /// joining access control". Defaults to a stream that never yields, for
    /// bridges whose remote does not (yet) send access requests; such bridges
    /// keep their current behavior of granting access implicitly.
    async fn access_requests(&self) -> AccessRequestStream {
        Box::pin(futures::stream::pending())
    }
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

    #[tokio::test]
    async fn access_request_decision_round_trips() {
        let (req, rx) = AccessRequest::new("client-1", Some("user-1".into()), "join the session");
        assert_eq!(req.client_id, "client-1");
        assert_eq!(req.permission, "join the session");
        req.respond(AccessDecision::Allowed);
        assert_eq!(rx.await.unwrap(), AccessDecision::Allowed);
    }

    #[tokio::test]
    async fn dropping_an_access_request_cancels_its_decision() {
        let (req, rx) = AccessRequest::new("client-1", None, "claim the device");
        drop(req);
        assert!(rx.await.is_err(), "no decision should arrive");
    }

    #[tokio::test]
    async fn default_bridge_has_no_identity_and_a_pending_request_stream() {
        let bridge: Box<dyn Bridge> = Box::new(FakeBridge::new());
        assert_eq!(bridge.device_id().await, None);
        // The default stream never yields (it must NOT terminate — a finished
        // stream would look like "access control is over" to a select loop).
        let mut requests = bridge.access_requests().await;
        assert!(futures::poll!(requests.next()).is_pending());
    }
}
