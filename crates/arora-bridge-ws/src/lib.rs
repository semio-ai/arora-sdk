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
//! - **Device**: [`Device`] is where everything a client discovers or calls
//!   comes from, asked as the client asks
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
//! # What a client discovers
//!
//! Everything comes from the device ([`Device`]), asked at the moment the client
//! asks: `list_keys` gives the keys it holds with the [`KeyMeta`] its store keeps
//! for each — the shape, the range, where it rests, whether anything outside the
//! device may write it — and `list_methods` the functions its modules export.
//! The server keeps nothing of its own, so a module loaded while the device runs
//! brings its keys and functions with it.
//!
//! A method that starts a **run** (a long-running, cancellable one —
//! `MethodInfo::task`) answers with the run named, and `halt` stops it by that
//! run's id.
//!
//! A write reaches the device, which refuses a key it writes itself
//! (`KeyMeta::editable`) rather than letting a client set what the next tick
//! would undo.
//!
//! # Server Example
//!
//! ```rust,no_run
//! use arora_bridge_ws::{AroraWSServer, ServerConfig};
//! use tokio_util::sync::CancellationToken;
//!
//! #[tokio::main]
//! async fn main() {
//!     let server = AroraWSServer::with_port(9000);
//!
//!     // Serving a device means wrapping the server in `bridge::WsBridge` and
//!     // handing that to the runtime; its keys and functions then answer for
//!     // themselves. These handlers are the seam underneath, for a server
//!     // driven without one.
//!     server.set_write_values_handler(std::sync::Arc::new(|values: std::collections::HashMap<String, arora_bridge_ws::Value>| {
//!         Box::pin(async move {
//!             println!("Received {} writes", values.len());
//!             Ok(())
//!         }) as _
//!     })).await;
//!
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
mod server;

pub use handlers::{
    Device, DeviceHandler, OnClientConnectedHandler, ReadValuesHandler, WriteValuesHandler,
    WriteValuesResult,
};
pub use key::KeyInfo;
pub use messages::{Incoming, Outgoing};
pub use method::{InvokeResult, MethodInfo, MethodParam};
pub use server::{AroraWSServer, ServerConfig};
pub use tokio_util::sync::CancellationToken;

pub use arora_types::data::KeyMeta;
pub use arora_types::keyvalue::{KeyValue, KeyValueField};
pub use arora_types::value::{Type, Value};
