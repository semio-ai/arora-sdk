//! Callback types the server dispatches incoming messages to.

use crate::method::InvokeResult;
use arora_bridge::client::KeyInfo;
use arora_bridge::client::MethodInfo;
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
/// Called when an external client writes values to keys; its answer is the
/// client's, so a write the device refuses is reported rather than acknowledged.
pub type WriteValuesHandler = Arc<
    dyn Fn(HashMap<String, Value>) -> Pin<Box<dyn Future<Output = WriteValuesResult> + Send>>
        + Send
        + Sync,
>;

/// Handler function type for ReadValues messages.
/// Called when an external client reads the current values of keys.
/// Returns a map of key paths to their current values.
pub type ReadValuesHandler = Arc<
    dyn Fn(Vec<String>) -> Pin<Box<dyn Future<Output = HashMap<String, Value>> + Send>>
        + Send
        + Sync,
>;

/// Handler called when a new client connects to this connection.
/// Receives the connection identifier (e.g., "ws://127.0.0.1:9000").
pub type OnClientConnectedHandler = Arc<dyn Fn(String) + Send + Sync>;

/// The device behind the server — everything a client can discover or call.
///
/// A bridge relays the device; it keeps nothing of its own. Keys and their meta
/// come from the store, functions from the modules that export them, and both
/// are asked at the moment the client asks, so what a module loaded mid-run
/// brought is there at once.
#[async_trait]
pub trait Device: Send + Sync {
    /// The keys the device holds, each with what the store says it is.
    async fn keys(&self) -> Vec<KeyInfo>;

    /// The device's callable functions, each with the parameter names and value
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
pub type DeviceHandler = Arc<dyn Device>;
