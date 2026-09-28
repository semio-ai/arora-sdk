//! Callback types the server dispatches incoming messages to.

use crate::method::{InvokeResult, MethodInfo};
use arora_types::value::Value;
use arora_types::Uuid;
use async_trait::async_trait;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Result type for the write-values handler.
pub type WriteValuesResult = Result<(), String>;

/// Handler function type for WriteValues messages.
/// Called when an external client writes values to keys.
pub type WriteValuesHandler =
    Arc<dyn Fn(HashMap<String, Value>) -> WriteValuesResult + Send + Sync>;

/// Handler function type for ReadValues messages.
/// Called when an external client reads the current values of keys.
/// Returns a map of key paths to their current values.
pub type ReadValuesHandler = Arc<
    dyn Fn(Vec<String>) -> Pin<Box<dyn Future<Output = HashMap<String, Value>> + Send>>
        + Send
        + Sync,
>;

/// Handler function type for method invocations.
pub type MethodHandler = Arc<dyn Fn(HashMap<String, Value>) -> InvokeResult + Send + Sync>;

/// Handler called when a new client connects to this connection.
/// Receives the connection identifier (e.g., "ws://127.0.0.1:9000").
pub type OnClientConnectedHandler = Arc<dyn Fn(String) + Send + Sync>;

/// The device behind the server: the methods it exports, and how a client's
/// `invoke` reaches them.
///
/// The server answers `list_methods` and `invoke` from its [`Registry`] — the
/// methods the server itself owns — and consults this seam for the rest, so the
/// device's module functions are callable without anything mirroring their ids
/// into the registry. [`WsBridge`](crate::bridge::WsBridge) implements it over
/// the runtime's inbound command stream and installs it on the server it wraps;
/// a server serving without a bridge simply has none.
#[async_trait]
pub trait DeviceMethods: Send + Sync {
    /// The device's callable methods, each with the parameter names and value
    /// shapes a client binds arguments to.
    async fn methods(&self) -> Vec<MethodInfo>;

    /// Call `method` with arguments by parameter name. A task-shaped method
    /// starts a run and answers with its handle — the run's id, its status key,
    /// and the keys carrying its feedback, result and live updates.
    async fn invoke(&self, method: &str, args: HashMap<String, Value>) -> InvokeResult;

    /// Stop the run `run` names.
    async fn halt(&self, run: Uuid) -> InvokeResult;
}

/// The device seam as the server holds it.
pub type DeviceMethodsHandler = Arc<dyn DeviceMethods>;
