//! Callback types the server dispatches incoming messages to.

use crate::method::InvokeResult;
use arora_types::value::Value;
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
