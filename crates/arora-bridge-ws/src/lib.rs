//! The open local bridge for Arora: a WebSocket server that bridges the Arora
//! API, implementing [`arora_bridge::Bridge`] (see [`bridge::WsBridge`]), with
//! type-safe message definitions, a method registry, and a ready-to-use server.
//!
//! Messages speak the Arora data-layer vocabulary: the store is a shared,
//! path-keyed blackboard, so clients **write** and **read** [`Value`]s at
//! **keys** (hierarchical paths, e.g. `face/mouth`), list the available keys,
//! and invoke registered RPC methods. The server binds the loopback interface
//! by default: the link is unauthenticated and meant for editors and apps on
//! trusted local links.
//!
//! # Features
//!
//! - **Message Types**: Type-safe [`Incoming`] and [`Outgoing`] message enums
//! - **Registry**: Advertise keys and methods with [`Registry`]
//! - **Server**: Full WebSocket server with [`AroraWSServer`]
//! - **Bridge**: [`bridge::WsBridge`] drives the server as an Arora [`Bridge`](arora_bridge::Bridge)
//!
//! # Wire Format
//!
//! Messages are JSON-encoded with a `type` field discriminator:
//!
//! ```json
//! // Client -> Server
//! {"type": "write_values", "values": {"face/mouth": {"f64": 0.5}}}
//! {"type": "read_values", "keys": ["face/mouth"]}
//! {"type": "list_keys", "path": "face"}
//! {"type": "list_methods"}
//! {"type": "invoke", "method": "say", "args": {"text": {"str": "hello"}}, "request_id": "req-1"}
//! {"type": "halt", "run": "0195e1f2-...", "request_id": "req-2"}
//! {"type": "subscribe", "keys": ["face/mouth"]}
//!
//! // Server -> Client
//! {"type": "write_values_resp", "success": true}
//! {"type": "read_values_resp", "values": {"face/mouth": {"f64": 0.5}}}
//! {"type": "list_keys_resp", "keys": [...]}
//! {"type": "list_methods_resp", "methods": [...]}
//! {"type": "invoke_resp", "success": true, "request_id": "req-1", "value": {...}}
//! {"type": "halt_resp", "success": true, "request_id": "req-2"}
//! {"type": "subscribe_resp", "keys": ["face/mouth"]}
//!
//! // Server -> Client, unsolicited: the live state feed, for the subscribed keys
//! {"type": "values_changed", "values": {"face/mouth": {"f64": 0.5}}}
//! ```
//!
//! # Methods
//!
//! `list_methods` and `invoke` answer from two places: the [`Registry`], which
//! holds the methods the server itself owns, and the device behind the bridge,
//! whose module functions are listed with their described signatures and called
//! by name. A method that starts a **run** (a long-running, cancellable one —
//! `MethodInfo::task`) answers with the run's handle, and `halt` stops it by the
//! id that handle carried.
//!
//! # Server Example
//!
//! ```rust,no_run
//! use arora_bridge_ws::{AroraWSServer, ServerConfig, MethodInfo, InvokeResult};
//! use tokio_util::sync::CancellationToken;
//!
//! #[tokio::main]
//! async fn main() {
//!     let server = AroraWSServer::with_port(9000);
//!
//!     // A method of the server's own. The device's own methods need no
//!     // registration: they are listed and called by name through the bridge.
//!     server.registry().register_method_fn(
//!         MethodInfo {
//!             path: "reset".to_string(),
//!             description: Some("Reset to defaults".to_string()),
//!             ..Default::default()
//!         },
//!         |_args| InvokeResult::ok(),
//!     ).await;
//!
//!     // Handle writes from clients
//!     server.set_write_values_handler(|values| {
//!         println!("Received {} writes", values.len());
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
mod interpreter;
mod key;
mod messages;
mod method;
mod registry;
mod server;

pub use handlers::{
    DeviceMethods, DeviceMethodsHandler, MethodHandler, OnClientConnectedHandler,
    ReadValuesHandler, WriteValuesHandler, WriteValuesResult,
};
pub use key::KeyInfo;
pub use messages::{Incoming, Outgoing};
pub use method::{InvokeResult, MethodInfo, MethodParam};
pub use registry::Registry;
pub use server::{AroraWSServer, ServerConfig};
pub use tokio_util::sync::CancellationToken;

pub use arora_types::keyvalue::{KeyValue, KeyValueField};
pub use arora_types::value::{Type, Value};
