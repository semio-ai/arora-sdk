//! Arora WebSocket Protocol
//!
//! The open local bridge for Arora: a WebSocket server implementing
//! [`arora_bridge::Bridge`] (see [`bridge::WsBridge`]), with type-safe message
//! definitions, a method registry, and a ready-to-use server.
//!
//! # Features
//!
//! - **Message Types**: Type-safe [`Incoming`] and [`Outgoing`] message enums
//! - **Registry**: Store slots and methods with [`Registry`]
//! - **Server**: Full WebSocket server with [`AroraWSServer`] (requires `server` feature)
//! - **Connection Trait**: Implements [`AroraConnection`] for protocol-agnostic usage
//!
//! # Protocol Overview
//!
//! Messages are JSON-encoded with a `type` field discriminator:
//!
//! ```json
//! // Client -> Server
//! {"type": "set_slot_values", "values": {"face/mouth": {"f64": 0.5}}}
//! {"type": "list_slots", "path": "face"}
//! {"type": "list_methods"}
//! {"type": "invoke", "method": "reset", "request_id": "req-1"}
//!
//! // Server -> Client
//! {"type": "set_slot_values_resp", "success": true}
//! {"type": "list_slots_resp", "slots": [...]}
//! {"type": "list_methods_resp", "methods": [...]}
//! {"type": "invoke_resp", "success": true, "request_id": "req-1"}
//! ```
//!
//! # Server Example
//!
//! ```rust,no_run
//! use arora_websocket::{AroraWSServer, ServerConfig, MethodInfo, InvokeResult};
//! use tokio_util::sync::CancellationToken;
//!
//! #[tokio::main]
//! async fn main() {
//!     let server = AroraWSServer::with_port(9000);
//!
//!     // Register a method
//!     server.registry().register_method_fn(
//!         MethodInfo {
//!             path: "reset".to_string(),
//!             params: vec![],
//!             return_type: None,
//!             description: Some("Reset to defaults".to_string()),
//!         },
//!         |_args| InvokeResult::ok(),
//!     ).await;
//!
//!     // Set update handler
//!     server.set_set_slot_values_handler(|values| {
//!         println!("Received {} updates", values.len());
//!         Ok(())
//!     }).await;
//!
//!     // Run the server
//!     let cancel = CancellationToken::new();
//!     server.run(cancel).await.unwrap();
//! }
//! ```

/// The WS server as an Arora `Bridge`.
pub mod bridge;
pub mod handlers;
mod messages;
mod method;
mod registry;
mod server;
mod slot;

pub use handlers::{
    GetSlotValuesHandler, MethodHandler, OnClientConnectedHandler, SetSlotValuesResult,
};
pub use messages::{Incoming, Outgoing};
pub use method::{InvokeResult, MethodInfo, MethodParam};
pub use registry::Registry;
pub use server::{process_message, AroraWSServer, ServerConfig, SetSlotValuesHandler};
pub use slot::SlotInfo;
pub use tokio_util::sync::CancellationToken;

pub use arora_types::keyvalue::{KeyValue, KeyValueField};
pub use arora_types::value::{Type, Value};
