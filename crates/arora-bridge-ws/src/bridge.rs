//! [`WsBridge`]: the WebSocket server driven as an Arora
//! [`Bridge`](arora_bridge::Bridge).
//!
//! Each incoming message becomes a [`BridgeCommand`] on the endpoint's inbound
//! stream (handed to the runtime once, via
//! [`take_inbound`](Bridge::take_inbound)); the runtime reacts to the ones it
//! cares about (writes apply to the store, reads reply). Runtime state flows
//! back out through [`try_send`](Bridge::try_send) as a `values_changed` push.
//!
//! The async lives in the server (its own accept/serve task, spawned by the
//! embedder): the server's registered handlers translate each incoming message
//! into a command and send it down the inbound channel whose receiver *is* the
//! stream the runtime polls — no intermediate buffer, no lock. [`try_send`]
//! pushes to the connected clients synchronously. The value vocabulary is
//! `arora_types::Value`, so the translation is structural, not a conversion.

use std::collections::HashMap;
use std::sync::Arc;

use arora_bridge::{
    Bridge, BridgeCommand, BridgeOp, BridgeResult, DeviceInfo, Inbound, InboundStream,
};
use arora_types::data::{Key, StateChange};
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;

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
    /// The inbound command receiver, moved out (once) by [`take_inbound`].
    commands: Option<mpsc::UnboundedReceiver<BridgeCommand>>,
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
            commands: Some(cmd_rx),
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
    fn take_inbound(&mut self) -> InboundStream {
        // A connected editor is a data consumer: the claim opens the stream,
        // then every command the server's handlers enqueue follows, in order.
        let commands = self
            .commands
            .take()
            .expect("WsBridge inbound stream already taken");
        Box::pin(
            futures::stream::once(async { Inbound::DataRequested(true) })
                .chain(commands.map(Inbound::Command)),
        )
    }

    fn try_send(&mut self, change: &StateChange) {
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
        let mut bridge = WsBridge::new(server.clone()).await;
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
