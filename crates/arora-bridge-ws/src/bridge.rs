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
//!
//! The device's **methods** travel the same channel: [`MethodPlane`] describes
//! them on demand and calls them by name, so a client reaches the device's
//! module functions without anything registering them on the server.

use std::collections::HashMap;
use std::sync::Arc;

use arora_bridge::{
    Bridge, BridgeCommand, BridgeOp, BridgeResult, DeviceInfo, Inbound, InboundStream,
    MethodSignature,
};
use arora_types::call::Call;
use arora_types::data::{Key, StateChange};
use arora_types::value::{StructureField, Value};
use arora_types::Uuid;
use async_trait::async_trait;
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;

use crate::handlers::DeviceMethods;
use crate::interpreter;
use crate::messages::Outgoing;
use crate::method::{InvokeResult, MethodInfo, MethodParam};
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

        // The method plane: what `list_methods` and `invoke` reach for every name
        // the server's own registry does not own. It holds a sender, not the
        // bridge, so it lives on the server while the device owns the bridge.
        server
            .set_device_methods(Arc::new(MethodPlane {
                commands: cmd_tx.clone(),
            }))
            .await;

        Self {
            server,
            commands: Some(cmd_rx),
        }
    }
}

/// The device's method plane over the runtime's inbound commands: it describes
/// the device's methods on demand — so a module loaded mid-run is callable at
/// once — and turns an `invoke` into the [`Call`] the described signature
/// defines.
struct MethodPlane {
    commands: mpsc::UnboundedSender<BridgeCommand>,
}

impl MethodPlane {
    /// Put `op` to the runtime and await its answer.
    async fn ask(&self, op: BridgeOp) -> Result<Value, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.commands
            .unbounded_send(BridgeCommand::new(op, reply_tx))
            .map_err(|_| "the device is gone".to_string())?;
        match reply_rx.await {
            Ok(result) => result.map(|result| result.ret),
            Err(_) => Err("the device dropped the request".to_string()),
        }
    }

    /// The device's methods with their full signatures. Empty when the device
    /// cannot answer — it stopped, or it never described any.
    async fn signatures(&self) -> Vec<MethodSignature> {
        match self.ask(BridgeOp::DescribeMethods { prefix: None }).await {
            Ok(ret) => arora_types::value_serde::from_value(ret).unwrap_or_else(|e| {
                log::warn!("the device's method signatures did not decode: {e}");
                Vec::new()
            }),
            Err(e) => {
                log::warn!("the device did not describe its methods: {e}");
                Vec::new()
            }
        }
    }
}

#[async_trait]
impl DeviceMethods for MethodPlane {
    async fn methods(&self) -> Vec<MethodInfo> {
        self.signatures().await.iter().map(method_info).collect()
    }

    async fn invoke(&self, method: &str, args: HashMap<String, Value>) -> InvokeResult {
        let signatures = self.signatures().await;
        let Some(signature) = signatures.iter().find(|s| s.name == method) else {
            return InvokeResult::err(format!("Method not found: {method}"));
        };
        let call = match call_of(signature, args) {
            Ok(call) => call,
            Err(message) => return InvokeResult::err(message),
        };
        // A task-shaped method starts a run: the spawn answers at once with the
        // run's handle, and the run reports on the handle's status key. Anything
        // else answers with its return value.
        let task = interpreter::task_shaped(&signature.function.return_ty);
        let call = if task {
            interpreter::spawn(&call)
        } else {
            call
        };
        match self.ask(BridgeOp::Call(call)).await {
            Ok(Value::Unit) => InvokeResult::ok(),
            Ok(value) if task => match interpreter::run_of(&value) {
                Ok(run) => InvokeResult::ok_with_value(interpreter::run_value(&run)),
                // The run did start: answer with the handle as it came rather
                // than telling the client it failed.
                Err(e) => {
                    log::warn!("{method} started a run whose handle did not decode: {e}");
                    InvokeResult::ok_with_value(value)
                }
            },
            Ok(value) => InvokeResult::ok_with_value(value),
            Err(message) => InvokeResult::err(message),
        }
    }

    async fn halt(&self, run: Uuid) -> InvokeResult {
        match self.ask(BridgeOp::Call(interpreter::halt(run))).await {
            Ok(_) => InvokeResult::ok(),
            Err(message) => InvokeResult::err(message),
        }
    }
}

/// Bind `args` by parameter name onto the parameter ids `signature` declares.
///
/// An argument the signature does not name fails the call rather than being
/// dropped, so a client that misspells a parameter learns it. A parameter no
/// argument names is left out, and whether it may be is the function's business:
/// an optional parameter reads as absent, a required one fails the call naming
/// itself.
fn call_of(signature: &MethodSignature, args: HashMap<String, Value>) -> Result<Call, String> {
    let function = &signature.function;
    let mut fields = Vec::with_capacity(args.len());
    for (name, value) in args {
        let Some(id) = function.parameter_id(&name) else {
            return Err(format!("{} has no parameter '{name}'", signature.name));
        };
        fields.push(StructureField {
            id: *id,
            value: Box::new(value),
        });
    }
    // Declaration order, so the call reads like the signature.
    fields.sort_by_key(|field| {
        function
            .parameter_ordering
            .iter()
            .position(|id| *id == field.id)
            .unwrap_or(usize::MAX)
    });
    Ok(Call {
        module_id: Some(signature.module_id),
        id: signature.id,
        args: fields,
    })
}

/// A described signature as a client reads it: the method's name, its parameters
/// in declaration order with the value shape and optionality of each, and
/// whether calling it starts a run.
fn method_info(signature: &MethodSignature) -> MethodInfo {
    let function = &signature.function;
    MethodInfo {
        path: signature.name.clone(),
        params: function
            .parameter_ordering
            .iter()
            .filter_map(|id| {
                let parameter = function.parameter(id)?;
                Some(MethodParam {
                    name: parameter.name.clone(),
                    param_type: interpreter::value_type(&parameter.ty),
                    required: !parameter.ty.is_option(),
                    default_value: None,
                    description: None,
                })
            })
            .collect(),
        return_type: Some(interpreter::value_type(&function.return_ty)),
        description: None,
        task: interpreter::task_shaped(&function.return_ty),
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
        //
        // The handlers holding the command senders live in the server this
        // bridge keeps alive, so the channel alone can never close — the
        // stream ends on the server's `stopped` signal instead. Ending is the
        // endpoint-disconnect signal (the runtime chains its marker on it), so
        // a dead server (port taken, accept loop gone) surfaces as a
        // disconnect instead of a device running on with an unreachable
        // bridge.
        let commands = self
            .commands
            .take()
            .expect("WsBridge inbound stream already taken");
        Box::pin(
            futures::stream::once(async { Inbound::DataRequested(true) })
                .chain(commands.map(Inbound::Command))
                .take_until(self.server.stopped()),
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

    /// A dead server ends the inbound stream — the endpoint-disconnect signal.
    /// Here the server dies at bind (port already taken); the same lifecycle
    /// signal covers an accept loop exiting later.
    #[tokio::test]
    async fn inbound_ends_when_the_server_dies() {
        use futures::StreamExt;

        // Occupy a port, then point the server at it so run() fails to bind.
        let taken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = taken.local_addr().unwrap().port();
        let server = Arc::new(AroraWSServer::new(ServerConfig::with_port(port)));
        let mut bridge = WsBridge::new(server.clone()).await;

        let error = server
            .run(crate::CancellationToken::new())
            .await
            .expect_err("the port is taken");
        assert!(error.contains("bind"), "{error}");

        // The stream ends (instead of pending forever on the open channel).
        let mut inbound = bridge.take_inbound();
        let ended = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while inbound.next().await.is_some() {}
        })
        .await;
        assert!(ended.is_ok(), "the inbound stream must end, not hang");
    }
}
