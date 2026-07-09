//! [`WsBridge`]: the WebSocket server driven as an Arora
//! [`Bridge`](arora_bridge::Bridge).
//!
//! Each incoming message becomes a [`BridgeCommand`] the runtime drains through
//! [`try_recv`](Bridge::try_recv), and reacts to the ones it cares about (writes
//! apply to the store, reads reply). Runtime state flows back out through
//! [`try_send`](Bridge::try_send) as a `values_changed` push.
//!
//! The async lives in the server (its own accept/serve task, spawned by the
//! embedder): the server's registered handlers translate each incoming message
//! into a command and enqueue it on an internal channel that [`try_recv`] drains
//! non-blocking. [`try_send`] pushes to the connected clients synchronously. The
//! value vocabulary is `arora_types::Value`, so the translation is structural,
//! not a conversion.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arora_bridge::{Bridge, BridgeCommand, BridgeOp, BridgeResult, DeviceInfo, Inbound};
use arora_types::data::{Key, StateChange};
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::{mpsc, oneshot};

use crate::messages::Outgoing;
use crate::server::AroraWSServer;

/// The WebSocket server as an Arora [`Bridge`].
///
/// Note: the server's `validate_paths` (on by default) checks written key
/// paths against its `Registry`, which this bridge does not populate — either
/// mirror the runtime's keys into the registry or disable `validate_paths`
/// when serving purely through the bridge.
pub struct WsBridge {
    server: Arc<AroraWSServer>,
    /// Inbound commands the server handlers enqueue; drained by [`try_recv`].
    commands: Mutex<mpsc::UnboundedReceiver<BridgeCommand>>,
    /// A connected editor is a data consumer: signal `data_requested(true)` once
    /// on the first [`try_recv`], before any command.
    data_requested_signaled: AtomicBool,
}

impl WsBridge {
    /// Wrap a server: register handlers that turn each incoming message into a
    /// [`BridgeCommand`] on the [`commands`](Bridge::commands) stream.
    pub async fn new(server: Arc<AroraWSServer>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded::<BridgeCommand>();

        // WriteValues -> Update. The handler is synchronous, so enqueue the
        // command and acknowledge; the runtime applies it on its next step.
        let tx = cmd_tx.clone();
        server
            .set_write_values_handler(move |values: HashMap<String, Value>| {
                let mut change = StateChange::new();
                for (path, value) in values {
                    change.set.insert(Key::from(path), Some(value));
                }
                let (reply_tx, _reply_rx) = oneshot::channel();
                tx.unbounded_send(BridgeCommand::new(BridgeOp::Update(change), reply_tx))
                    .map_err(|_| "bridge command channel closed".to_string())
            })
            .await;

        // ReadValues -> Get, awaiting the runtime's reply.
        let tx = cmd_tx.clone();
        server
            .set_read_values_handler(Arc::new(move |keys: Vec<String>| {
                let tx = tx.clone();
                Box::pin(async move {
                    let store_keys: Vec<Key> = keys.iter().cloned().map(Key::from).collect();
                    let (reply_tx, reply_rx) = oneshot::channel();
                    if tx
                        .unbounded_send(BridgeCommand::new(BridgeOp::Get(store_keys), reply_tx))
                        .is_err()
                    {
                        return HashMap::new();
                    }
                    match reply_rx.await {
                        Ok(Ok(result)) => values_from_get(&keys, result.ret),
                        _ => HashMap::new(),
                    }
                }) as _
            }))
            .await;

        Self {
            server,
            commands: Mutex::new(cmd_rx),
            data_requested_signaled: AtomicBool::new(false),
        }
    }
}

/// Decode a `Get` reply — an `ArrayValue` of `Option`s in request order — into a
/// `path -> value` map.
fn values_from_get(keys: &[String], ret: Value) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Value::ArrayValue(items) = ret {
        for (key, item) in keys.iter().zip(items) {
            if let Value::Option(Some(value)) = item {
                out.insert(key.clone(), *value);
            }
        }
    }
    out
}

#[async_trait]
impl Bridge for WsBridge {
    fn try_recv(&self) -> Option<Inbound> {
        // A connected editor is a data consumer: emit the claim once, up front.
        if !self.data_requested_signaled.swap(true, Ordering::Relaxed) {
            return Some(Inbound::DataRequested(true));
        }
        // Drain the next command the server's handlers enqueued, if any.
        // `Err` = empty right now or the sender was dropped; either way, nothing.
        match self.commands.lock().unwrap().try_recv() {
            Ok(cmd) => Some(Inbound::Command(cmd)),
            Err(_) => None,
        }
    }

    fn try_send(&self, change: &StateChange) {
        // Push the changed keys to the connected client(s).
        let mut values = HashMap::new();
        for (key, value) in &change.set {
            if let Some(value) = value {
                values.insert(key.path.clone(), value.clone());
            }
        }
        if !values.is_empty() {
            self.server.push(Outgoing::ValuesChanged { values });
        }
    }

    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
        // A local editor connection has no device-registration concept.
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
    use crate::server::ServerConfig;

    #[tokio::test]
    async fn try_send_pushes_values_changed() {
        let server = Arc::new(AroraWSServer::new(ServerConfig::default()));
        let bridge = WsBridge::new(server.clone()).await;
        let mut rx = server.subscribe();

        bridge.try_send(&StateChange::set("face/mouth", Value::F64(0.5)));

        match rx.recv().await.expect("a push") {
            Outgoing::ValuesChanged { values } => {
                assert_eq!(values.get("face/mouth"), Some(&Value::F64(0.5)));
            }
            other => panic!("expected ValuesChanged, got {other:?}"),
        }
    }
}
