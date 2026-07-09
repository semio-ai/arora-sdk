//! The Arora Bridge interface.
//!
//! A [`Bridge`] connects an Arora runtime to a remote — in practice Semio Studio
//! over `studio-bridge`. It is modelled on studio-bridge's `device-client`
//! trait: push local state changes out, receive device-info updates and
//! commands in, and learn when a client is asking for data.
//!
//! # The seam: an owned inbound stream, a non-blocking outbound push
//!
//! A `Bridge` value is an **endpoint**: the connection as seen by exactly one
//! device, owned by exactly one runtime. Its data plane is two methods:
//!
//! - [`take_inbound`](Bridge::take_inbound) hands over the endpoint's inbound
//!   event stream — **once**, at assembly. The runtime owns the stream from
//!   then on and polls it from its own loop (natively `run`'s select drains it
//!   between steps; on the web the per-frame sweep does). The stream
//!   **ending** means the endpoint disconnected; the runtime maps that
//!   explicitly, it is never silent.
//! - [`try_send`](Bridge::try_send) pushes an outbound [`StateChange`] toward
//!   the remote, now, without blocking (a channel send / synchronous put).
//!
//! Exclusive ownership is the point: one poller per endpoint, so there is no
//! shared receiver and **no lock anywhere on the data plane**. An
//! implementation whose transport genuinely serves several devices (one Zenoh
//! session, one WebSocket server) keeps that transport in a shared *core* and
//! hands out one endpoint per device, demuxing inbound events to the right
//! endpoint's stream — share the core, not the endpoint.
//!
//! The trait depends on `futures-core` (for [`Stream`]) but on **no async
//! runtime**:
//! native impls may feed the stream from their own tokio task, web impls from
//! browser events; the runtime only polls.
//!
//! The trait lives here (lean: `arora-types` + stream primitives) so the
//! runtime can depend on the *interface* without depending on `studio-bridge`.
//! studio-bridge keeps its device-client implementations and implements this
//! trait on them.

use std::pin::Pin;

use async_trait::async_trait;
use futures_channel::oneshot;
use futures_core::Stream;

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

/// Stream of access requests from remote clients.
pub type AccessRequestStream = Pin<Box<dyn Stream<Item = AccessRequest> + Send>>;

/// An inbound event on a bridge endpoint, yielded by the stream
/// [`Bridge::take_inbound`] hands over.
///
/// The implementation feeds these from its own transport (a task, a browser
/// callback); the runtime's loop buffers them and applies them in arrival
/// order during its step.
pub enum Inbound {
    /// A command from the remote, carrying its one-shot reply channel.
    Command(BridgeCommand),
    /// A device-info update. `Ok(None)` means the device was unregistered — the
    /// runtime should stop stepping.
    DeviceInfo(BridgeResult<Option<DeviceInfo>>),
    /// A client claimed/released interest in the data (the "data requested"
    /// toggle).
    DataRequested(bool),
}

/// The inbound event stream of one bridge endpoint. Owned: whoever takes it is
/// the endpoint's one poller. The stream ending means the endpoint
/// disconnected.
pub type InboundStream = Pin<Box<dyn Stream<Item = Inbound> + Send>>;

/// One device's connection to a remote (e.g. Semio Studio) — an **endpoint**,
/// owned by exactly one runtime.
///
/// Modelled on studio-bridge's `device-client`. The data plane is exclusive
/// (`&mut self`, no lock): the runtime takes the inbound stream once at
/// assembly and pushes outbound changes as it steps. The remaining methods are
/// the control plane (device identity/info, access requests), used around the
/// loop rather than inside it.
#[async_trait]
pub trait Bridge: Send + Sync {
    /// Hand over the endpoint's inbound event stream. Called **once**, at
    /// assembly; the caller owns the stream from then on (one poller per
    /// endpoint). The stream ending means the endpoint disconnected.
    ///
    /// Implementations typically move an internal channel receiver out here
    /// (and may panic on a second call — taking twice is a programming error).
    fn take_inbound(&mut self) -> InboundStream;

    /// Push an outbound state change toward the remote, now, without blocking.
    ///
    /// The implementation enqueues it onto its own outbound channel/transport;
    /// it must not block the caller's step.
    fn try_send(&mut self, change: &StateChange);

    /// The device's current info, if registered.
    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>>;

    /// Push updated device info to the remote; returns the merged result.
    async fn update_device_info(
        &self,
        info: Option<DeviceInfo>,
    ) -> BridgeResult<Option<DeviceInfo>>;

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
    ///
    /// This is a separate concern from the inbound data plane
    /// ([`take_inbound`](Bridge::take_inbound)): the operator front end serves
    /// it on its own task, one request at a time.
    async fn access_requests(&self) -> AccessRequestStream {
        Box::pin(futures_util::stream::pending())
    }
}

/// A no-op [`Bridge`] for tests and offline runs: never registers, never emits
/// inbound events (its stream never yields — the endpoint never disconnects),
/// and accepts (drops) any data sent.
#[derive(Clone, Default)]
pub struct FakeBridge;

impl FakeBridge {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Bridge for FakeBridge {
    fn take_inbound(&mut self) -> InboundStream {
        Box::pin(futures_util::stream::pending())
    }

    fn try_send(&mut self, _change: &StateChange) {}

    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
        Ok(None)
    }

    async fn update_device_info(
        &self,
        info: Option<DeviceInfo>,
    ) -> BridgeResult<Option<DeviceInfo>> {
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn fake_bridge_is_usable_as_trait_object() {
        let mut bridge: Box<dyn Bridge> = Box::new(FakeBridge::new());
        assert_eq!(bridge.get_device_info().await.unwrap(), None);
        // The inbound stream never yields (the fake never disconnects), and
        // outbound is dropped.
        let mut inbound = bridge.take_inbound();
        assert!(futures::poll!(inbound.next()).is_pending());
        bridge.try_send(&StateChange::new());
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
