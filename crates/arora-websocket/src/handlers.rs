//! Callback types the server dispatches incoming protocol messages to.

use crate::method::InvokeResult;
use arora_types::value::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Result type for set slot values handler.
pub type SetSlotValuesResult = Result<(), String>;

/// Handler function type for SetSlotValues messages.
/// Called when an external client wants to update slot values.
pub type SetSlotValuesHandler =
    Arc<dyn Fn(HashMap<String, Value>) -> SetSlotValuesResult + Send + Sync>;

/// Handler function type for GetSlotValues messages.
/// Called when an external client wants to read current slot values.
/// Returns a map of slot paths to their current values.
pub type GetSlotValuesHandler = Arc<
    dyn Fn(Vec<String>) -> Pin<Box<dyn Future<Output = HashMap<String, Value>> + Send>>
        + Send
        + Sync,
>;

/// Handler function type for method invocations.
pub type MethodHandler = Arc<dyn Fn(HashMap<String, Value>) -> InvokeResult + Send + Sync>;

/// Handler called when a new client connects to this connection.
/// Receives the connection identifier (e.g., "ws://127.0.0.1:9000").
pub type OnClientConnectedHandler = Arc<dyn Fn(String) + Send + Sync>;
