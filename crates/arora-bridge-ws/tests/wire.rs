//! The wire, end to end: a client on the socket, a fake runtime behind the
//! bridge.
//!
//! What these pin is what a client can count on — the device's methods listed
//! and called by name, a run started and halted, and a subscription deciding
//! which pushes arrive — so the pins survive any reshuffling of the server's
//! internals.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arora_behavior::interpreter_module;
use arora_behavior::{TaskHandle, TaskId};
use arora_bridge::{Bridge, BridgeOp, Inbound, MethodSignature};
use arora_bridge_ws::bridge::WsBridge;
use arora_bridge_ws::{
    AroraWSServer, CancellationToken, InvokeResult, MethodInfo, ServerConfig, Value,
};
use arora_types::call::{Call, CallResult};
use arora_types::data::{Key, StateChange};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::record::{FrozenReference, Version};
use arora_types::Uuid;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

const SAY_MODULE: Uuid = Uuid::from_u128(0x5a11);
const SAY: Uuid = Uuid::from_u128(0x5a12);
const SAY_TEXT: Uuid = Uuid::from_u128(0x5a13);
const COS: Uuid = Uuid::from_u128(0x0c51);
const COS_ANGLE: Uuid = Uuid::from_u128(0x0c52);
const RUN: Uuid = Uuid::from_u128(0x41);

/// `say(text: String) -> Status` — task-shaped, because a behavior status is
/// what a run reports — and `cos(angle: f32) -> f32`, which answers at once.
fn signatures() -> Vec<MethodSignature> {
    let parameter = |name: &str, kind: PrimitiveKind| Parameter {
        name: name.to_string(),
        ty: FrozenTy::from(kind),
        mutable: false,
    };
    let status = FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: arora_behavior_tree_types::STATUS_ENUMERATION_ID,
            version: Version::parse("1.0.0").expect("a valid version"),
        },
    });
    vec![
        MethodSignature {
            module_id: SAY_MODULE,
            id: SAY,
            name: "say".to_string(),
            function: Function {
                parameters: HashMap::from([(SAY_TEXT, parameter("text", PrimitiveKind::String))]),
                parameter_ordering: vec![SAY_TEXT],
                return_ty: status,
            },
        },
        MethodSignature {
            module_id: SAY_MODULE,
            id: COS,
            name: "cos".to_string(),
            function: Function {
                parameters: HashMap::from([(COS_ANGLE, parameter("angle", PrimitiveKind::F32))]),
                parameter_ordering: vec![COS_ANGLE],
                return_ty: FrozenTy::from(PrimitiveKind::F32),
            },
        },
    ]
}

/// The run `say` spawns: the handle a client gets back, and halts by its id.
fn handle() -> TaskHandle {
    let id = TaskId(RUN);
    TaskHandle {
        id,
        stop: interpreter_module::encode_halt(id),
        status: Key::from("arora/tasks/say/status"),
        feedback: Vec::new(),
        result: Vec::new(),
        update: Vec::new(),
    }
}

/// A served bridge with a fake runtime behind it: the runtime describes
/// [`signatures`], answers a spawn with [`handle`], and records every call it
/// was asked to make.
struct Device {
    bridge: WsBridge,
    url: String,
    calls: Arc<Mutex<Vec<Call>>>,
    cancel: CancellationToken,
}

impl Device {
    async fn serve() -> Self {
        let server = Arc::new(AroraWSServer::new(ServerConfig::with_port(0)));
        // A method of the server's own, to pin that it keeps precedence.
        server
            .registry()
            .register_method_fn(
                MethodInfo {
                    path: "reset".to_string(),
                    ..Default::default()
                },
                |_args| InvokeResult::ok(),
            )
            .await;
        let mut bridge = WsBridge::new(server.clone()).await;
        let listener = server.bind().await.expect("bind an ephemeral port");
        let url = format!("ws://{}", listener.local_addr().expect("the bound address"));
        let cancel = CancellationToken::new();
        tokio::spawn({
            let server = server.clone();
            let cancel = cancel.clone();
            async move { server.run_on(listener, cancel).await }
        });

        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut inbound = bridge.take_inbound();
        tokio::spawn({
            let calls = calls.clone();
            async move {
                while let Some(event) = inbound.next().await {
                    let Inbound::Command(command) = event else {
                        continue;
                    };
                    let reply = match &command.op {
                        BridgeOp::DescribeMethods { .. } => {
                            arora_types::value_serde::to_value(&signatures())
                                .map(|ret| CallResult {
                                    ret,
                                    mutated: Vec::new(),
                                })
                                .map_err(|e| e.to_string())
                        }
                        BridgeOp::Call(call) => {
                            calls.lock().expect("the call log").push(call.clone());
                            let ret = if interpreter_module::decode_spawn(call).is_ok() {
                                interpreter_module::encode_spawn_result(&handle())
                            } else {
                                Value::F32(1.0)
                            };
                            Ok(CallResult {
                                ret,
                                mutated: Vec::new(),
                            })
                        }
                        _ => Ok(CallResult {
                            ret: Value::Unit,
                            mutated: Vec::new(),
                        }),
                    };
                    command.reply(reply);
                }
            }
        });

        Self {
            bridge,
            url,
            calls,
            cancel,
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("the call log").clone()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// One client on the socket, sending JSON and reading the answers.
struct Client {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl Client {
    async fn connect(url: &str) -> Self {
        let (socket, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("connect to the served bridge");
        Self { socket }
    }

    async fn send(&mut self, message: serde_json::Value) {
        self.socket
            .send(Message::Text(message.to_string()))
            .await
            .expect("send a message");
    }

    /// The next answer, or `None` if none arrives within `patience`.
    async fn next(&mut self, patience: Duration) -> Option<serde_json::Value> {
        loop {
            let message = tokio::time::timeout(patience, self.socket.next())
                .await
                .ok()?;
            match message {
                Some(Ok(Message::Text(text))) => {
                    return Some(serde_json::from_str(&text).expect("an answer in JSON"))
                }
                Some(Ok(_)) => continue,
                _ => return None,
            }
        }
    }

    async fn answer(&mut self) -> serde_json::Value {
        self.next(Duration::from_secs(5))
            .await
            .expect("an answer within five seconds")
    }
}

/// The device's methods are listed beside the server's own, each with the
/// parameters of its described signature, and a run says so.
#[tokio::test]
async fn the_device_methods_are_listed() {
    let device = Device::serve().await;
    let mut client = Client::connect(&device.url).await;

    client
        .send(serde_json::json!({"type": "list_methods"}))
        .await;
    let answer = client.answer().await;
    let methods = answer["methods"].as_array().expect("the listed methods");
    let by_name = |name: &str| {
        methods
            .iter()
            .find(|method| method["path"] == name)
            .unwrap_or_else(|| panic!("{name} is listed"))
            .clone()
    };

    let say = by_name("say");
    assert_eq!(say["task"], true, "say starts a run");
    assert_eq!(say["params"][0]["name"], "text");
    assert_eq!(say["params"][0]["param_type"], "str");
    assert_eq!(say["params"][0]["required"], true);

    assert_eq!(by_name("cos")["task"], false, "cos answers at once");
    by_name("reset"); // the server's own method is listed too
}

/// An invoke of a task-shaped method spawns it with the arguments bound to the
/// parameter ids its signature declares, and answers with the run's handle; the
/// halt that follows stops that run.
#[tokio::test]
async fn a_run_is_spawned_by_name_and_halted_by_id() {
    let device = Device::serve().await;
    let mut client = Client::connect(&device.url).await;

    client
        .send(serde_json::json!({
            "type": "invoke",
            "method": "say",
            "args": {"text": {"str": "hello"}},
            "request_id": "req-1"
        }))
        .await;
    let answer = client.answer().await;
    assert_eq!(answer["success"], true, "{answer}");
    assert_eq!(answer["request_id"], "req-1");
    // The run comes back by name, like everything else on this wire.
    let run = &answer["value"]["keyvalue"]["fields"];
    assert_eq!(
        run["run"]["value"]["str"],
        RUN.to_string(),
        "the answer names the run: {answer}"
    );
    assert_eq!(
        run["status"]["value"]["str"], "arora/tasks/say/status",
        "and the key that says how it ends: {answer}"
    );

    let (spawned, _policy) =
        interpreter_module::decode_spawn(&device.calls()[0]).expect("the invoke spawned a run");
    assert_eq!(spawned.module_id, Some(SAY_MODULE));
    assert_eq!(spawned.id, SAY);
    assert_eq!(spawned.args.len(), 1);
    assert_eq!(spawned.args[0].id, SAY_TEXT, "bound by parameter name");
    assert_eq!(
        spawned.args[0].value.as_ref(),
        &Value::String("hello".to_string())
    );

    let id = run["run"]["value"]["str"].as_str().expect("the run id");
    client
        .send(serde_json::json!({"type": "halt", "run": id, "request_id": "req-2"}))
        .await;
    let answer = client.answer().await;
    assert_eq!(answer["success"], true, "{answer}");
    assert_eq!(
        interpreter_module::decode_halt(&device.calls()[1]).expect("a halt"),
        TaskId(RUN)
    );
}

/// A plain method is called, not spawned, and answers with its return value.
#[tokio::test]
async fn a_plain_method_answers_with_its_value() {
    let device = Device::serve().await;
    let mut client = Client::connect(&device.url).await;

    client
        .send(serde_json::json!({
            "type": "invoke", "method": "cos", "args": {"angle": {"f32": 0.0}}
        }))
        .await;
    let answer = client.answer().await;
    assert_eq!(answer["success"], true, "{answer}");
    assert_eq!(answer["value"], serde_json::json!({"f32": 1.0}));

    let call = &device.calls()[0];
    assert_eq!(call.id, COS, "called, not spawned");
    assert_eq!(call.args[0].id, COS_ANGLE);
}

/// A misspelled parameter fails the call instead of being dropped.
#[tokio::test]
async fn an_unknown_parameter_fails_the_call() {
    let device = Device::serve().await;
    let mut client = Client::connect(&device.url).await;

    client
        .send(serde_json::json!({
            "type": "invoke", "method": "say", "args": {"txet": {"str": "hello"}}
        }))
        .await;
    let answer = client.answer().await;
    assert_eq!(answer["success"], false, "{answer}");
    assert!(
        answer["message"]
            .as_str()
            .expect("a message")
            .contains("txet"),
        "the answer names the parameter: {answer}"
    );
    assert!(device.calls().is_empty(), "nothing was called");
}

/// A client that subscribes is pushed the keys it asked for, and nothing else —
/// no key needs to be special-cased for it.
#[tokio::test]
async fn a_subscription_decides_what_is_pushed() {
    let mut device = Device::serve().await;
    let mut client = Client::connect(&device.url).await;

    client
        .send(serde_json::json!({"type": "subscribe", "keys": ["face/mouth"]}))
        .await;
    let answer = client.answer().await;
    assert_eq!(answer["type"], "subscribe_resp");

    device
        .bridge
        .try_send(&StateChange::set("arora/time", Value::F64(1.0)));
    device
        .bridge
        .try_send(&StateChange::set("face/mouth", Value::F64(0.5)));

    let pushed = client.answer().await;
    assert_eq!(pushed["type"], "values_changed");
    assert_eq!(
        pushed["values"],
        serde_json::json!({"face/mouth": {"f64": 0.5}}),
        "the clock was not subscribed to"
    );
    assert!(
        client.next(Duration::from_millis(200)).await.is_none(),
        "nothing else follows"
    );
}

/// A client that never subscribes is pushed everything — what a client written
/// before subscription existed still gets.
#[tokio::test]
async fn without_a_subscription_every_key_is_pushed() {
    let mut device = Device::serve().await;
    let mut client = Client::connect(&device.url).await;

    // The connection is live once a first exchange has completed.
    client.send(serde_json::json!({"type": "list_keys"})).await;
    client.answer().await;

    device
        .bridge
        .try_send(&StateChange::set("arora/time", Value::F64(1.0)));
    let pushed = client.answer().await;
    assert_eq!(
        pushed["values"],
        serde_json::json!({"arora/time": {"f64": 1.0}})
    );
}
