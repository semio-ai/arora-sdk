//! The open local bridge over a running device: a WebSocket client on the
//! socket, an Arora device with a HAL behind it.
//!
//! What these pin is what a client of the local bridge can count on from the
//! device itself — the HAL's inputs listed open and typed and its outputs
//! closed, a write in either target form moving what the HAL senses, a
//! refused write answered with the device's reason, and a client holding no
//! Rust code describing the device's functions, calling one by ids, and
//! loading a behavior through the interpreter module's ids.

#![cfg(feature = "native")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arora::bridge_ws::{AroraWSServer, ServerConfig};
use arora::{Arora, MethodSignature, ModuleBuilder, ModuleFunction};
use arora_behavior::interpreter_module;
use arora_behavior_tree::schema_groot::BehaviorTree;
use arora_hal::FakeHal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallResult};
use arora_types::data::{DataStore, StateChange};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::Value;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value as Json};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// A device over `hal`, serving the local bridge on a free loopback port, and
/// the bridge's address.
async fn device_over(hal: FakeHal) -> (Arora, String) {
    device_of(Arora::builder().with_hal(Box::new(hal))).await
}

/// The device `builder` builds, serving the local bridge on a free loopback
/// port, and the bridge's address.
async fn device_of(builder: arora::AroraBuilder) -> (Arora, String) {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let server = Arc::new(AroraWSServer::new(ServerConfig::with_port(port)));
    let bridge = arora::serve_local_ws_bridge(server)
        .await
        .expect("serve the local bridge");
    let device = builder
        .with_bridge(bridge)
        .build()
        .expect("build the device");
    (device, format!("ws://127.0.0.1:{port}"))
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
            .expect("connect to the local bridge");
        Self { socket }
    }

    async fn send(&mut self, message: Json) {
        self.socket
            .send(Message::text(message.to_string()))
            .await
            .expect("send a message");
    }

    /// The next message, of any type.
    async fn next_of_any(&mut self) -> Json {
        let wait = async {
            loop {
                match self.socket.next().await {
                    Some(Ok(Message::Text(text))) => {
                        return serde_json::from_str(&text).expect("JSON");
                    }
                    Some(Ok(_)) => continue,
                    other => panic!("the socket ended: {other:?}"),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .expect("a message within five seconds")
    }

    /// The next message of type `kind`, skipping any other.
    async fn next_of(&mut self, kind: &str) -> Json {
        let wait = async {
            loop {
                match self.socket.next().await {
                    Some(Ok(Message::Text(text))) => {
                        let message: Json = serde_json::from_str(&text).expect("JSON");
                        if message["type"] == kind {
                            return message;
                        }
                    }
                    Some(Ok(_)) => continue,
                    other => panic!("the socket ended waiting for {kind}: {other:?}"),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .unwrap_or_else(|_| panic!("no {kind} within five seconds"))
    }

    async fn write(&mut self, values: Json) -> Json {
        self.send(json!({"type": "write_values", "values": values}))
            .await;
        self.next_of("write_values_resp").await
    }

    /// The value pushed for `key` once it equals `expected`.
    async fn sees(&mut self, key: &str, expected: Json) {
        loop {
            let pushed = self.next_of("values_changed").await;
            if pushed["values"][key] == expected {
                return;
            }
        }
    }
}

/// Drive `device` at its default cadence until `client` finishes.
async fn with_device<T>(device: &mut Arora, client: impl std::future::Future<Output = T>) -> T {
    tokio::select! {
        result = device.run(Arora::DEFAULT_STEP_PERIOD) => panic!("the device stopped: {result:?}"),
        output = client => output,
    }
}

fn joint() -> FakeHal {
    let hal = FakeHal::new();
    hal.add_joints(["hip_yaw"]);
    hal
}

/// `list_keys` shows the HAL's inputs — both target forms — open and typed,
/// and what it senses closed.
#[tokio::test]
async fn the_hal_s_keys_are_listed_with_their_meta() {
    let (mut device, url) = device_over(joint()).await;
    let keys = with_device(&mut device, async {
        let mut client = Client::connect(&url).await;
        client
            .send(json!({"type": "list_keys", "path": "hip_yaw"}))
            .await;
        client.next_of("list_keys_resp").await
    })
    .await;
    let listed: Vec<(&str, &Json, &Json)> = keys["keys"]
        .as_array()
        .expect("the listed keys")
        .iter()
        .map(|key| {
            (
                key["path"].as_str().expect("a path"),
                &key["__meta"]["editable"],
                &key["__meta"]["ty"],
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![
            ("hip_yaw.position", &json!(false), &json!("f64")),
            ("hip_yaw.target_position", &json!(true), &json!("f64")),
            ("hip_yaw/target.position", &json!(true), &json!("f64")),
        ]
    );
}

/// A write in either target form is acknowledged and moves the sensed
/// position, which the client is pushed. A write the device refuses is
/// answered `success: false` with the device's reason, and a batch holding one
/// refused key is refused whole: nothing in it reaches the HAL.
#[tokio::test]
async fn a_write_is_acknowledged_in_either_target_form_or_refused_with_a_reason() {
    let (mut device, url) = device_over(joint()).await;
    with_device(&mut device, async {
        let mut client = Client::connect(&url).await;
        client
            .send(json!({"type": "subscribe", "keys": ["hip_yaw.position"]}))
            .await;
        client.next_of("subscribe_resp").await;
        for (values, position) in [
            (json!({"hip_yaw/target.position": {"f64": 0.5}}), 0.5),
            (json!({"hip_yaw.target_position": {"f64": 0.25}}), 0.25),
        ] {
            let answer = client.write(values).await;
            assert_eq!(
                answer,
                json!({"type": "write_values_resp", "success": true})
            );
            client
                .sees("hip_yaw.position", json!({"f64": position}))
                .await;
        }

        for (values, why) in [
            (
                json!({"hip_yaw.target_position": {"i64": 1}}),
                "declared type",
            ),
            (
                json!({"hip_yaw/target.position": {"i64": 1}}),
                "declared type",
            ),
            (json!({"hip_yaw.position": {"f64": 1.0}}), "not an input"),
            (
                json!({
                    "hip_yaw.target_position": {"f64": 1.0},
                    "hip_yaw.position": {"f64": 1.0},
                }),
                "not an input",
            ),
        ] {
            let answer = client.write(values.clone()).await;
            assert_eq!(answer["success"], json!(false), "{values}: {answer}");
            let message = answer["message"].as_str().unwrap_or_default();
            assert!(message.contains(why), "{values}: {answer}");
        }

        // The read is answered in a later step than the refused batch, and each
        // step takes in what the HAL sensed before it answers: a target that
        // slipped through would be read back here.
        client
            .send(json!({"type": "read_values", "keys": ["hip_yaw.position"]}))
            .await;
        let read = client.next_of("read_values_resp").await;
        assert_eq!(read["values"]["hip_yaw.position"], json!({"f64": 0.25}));
    })
    .await;
}

const MATH: Uuid = Uuid::from_u128(0x6d61_7468);
const DOUBLE: Uuid = Uuid::from_u128(0x6d61_7468_0001);
const DOUBLE_X: Uuid = Uuid::from_u128(0x6d61_7468_0002);

/// `double(x: f64) -> f64`, as its module describes it.
fn double_signature() -> Function {
    Function {
        parameters: HashMap::from([(
            DOUBLE_X,
            Parameter {
                name: "x".to_string(),
                ty: FrozenTy::from(PrimitiveKind::F64),
                mutable: false,
            },
        )]),
        parameter_ordering: vec![DOUBLE_X],
        return_ty: FrozenTy::from(PrimitiveKind::F64),
    }
}

/// A device with one module, `math`, exporting `double`, over a store holding
/// `in = 2.0`; and the bridge's address.
async fn device_with_math(store: SimpleDataStore) -> (Arora, String) {
    let math = ModuleBuilder::new(MATH)
        .described_function(
            DOUBLE,
            "double",
            double_signature(),
            |call: Call| match call.args.iter().find(|arg| arg.id == DOUBLE_X) {
                Some(arg) => match *arg.value {
                    Value::F64(x) => Ok(CallResult {
                        ret: Value::F64(2.0 * x),
                        mutated: Vec::new(),
                    }),
                    ref other => Err(arora_types::call::CallError::Generic {
                        message: format!("x is not an f64: {other:?}"),
                    }),
                },
                None => Err(arora_types::call::CallError::Generic {
                    message: "x is missing".to_string(),
                }),
            },
        )
        .build();
    store
        .write(StateChange::set("in", Value::F64(2.0)))
        .expect("seed the input");
    device_of(
        Arora::builder()
            .with_data_store(Box::new(store))
            .with_host_module(math),
    )
    .await
}

impl Client {
    /// Send `call` with `request_id`, as a client holding no Rust code would:
    /// the call's own JSON beside the message's fields.
    async fn call(&mut self, call: &Call, request_id: &str) {
        let mut message = serde_json::to_value(call).expect("a call in JSON");
        message["type"] = json!("call");
        message["request_id"] = json!(request_id);
        self.send(message).await;
    }

    /// The device's signatures, as `describe_methods` answers them.
    async fn describe(&mut self) -> Vec<MethodSignature> {
        self.send(json!({"type": "describe_methods"})).await;
        let answer = self.next_of("describe_methods_resp").await;
        serde_json::from_value(answer["methods"].clone()).expect("the described signatures")
    }
}

/// `describe_methods` answers with the module's function under the ids a call
/// names, and a call by those ids answers with its return value.
#[tokio::test]
async fn a_module_function_is_described_and_called_by_ids() {
    let (mut device, url) = device_with_math(SimpleDataStore::new()).await;
    with_device(&mut device, async {
        let mut client = Client::connect(&url).await;
        let described = client.describe().await;
        let double = described
            .iter()
            .find(|method| method.name == "double")
            .unwrap_or_else(|| panic!("double is described among {described:?}"));
        assert_eq!(double.module_id, MATH);
        assert_eq!(double.id, DOUBLE);
        assert_eq!(double.function, double_signature());
        // What the device's method index describes, and only that: the
        // interpreter's task-run functions are listed, its `load` and `edit`
        // are reached by their fixed ids and not described.
        let interpreter: Vec<&str> = described
            .iter()
            .filter(|method| method.module_id == interpreter_module::ID)
            .map(|method| method.name.as_str())
            .collect();
        assert_eq!(interpreter, ["halt", "spawn", "spawn_graph"]);

        client
            .call(
                &Call {
                    module_id: Some(double.module_id),
                    id: double.id,
                    args: vec![arora_types::value::StructureField {
                        id: DOUBLE_X,
                        value: Box::new(Value::F64(3.0)),
                    }],
                },
                "double-3",
            )
            .await;
        assert_eq!(
            client.next_of("call_resp").await,
            json!({
                "type": "call_resp",
                "success": true,
                "request_id": "double-3",
                "ret": {"f64": 6.0},
                "mutated": []
            })
        );
    })
    .await;
}

/// A behavior resolved against the described functions and loaded by a call
/// to the interpreter module's `load` runs from the next steps: the key its
/// node writes changes.
#[tokio::test]
async fn a_call_to_load_installs_a_behavior_the_device_runs() {
    let store = SimpleDataStore::new();
    let (mut device, url) = device_with_math(store.clone()).await;
    with_device(&mut device, async {
        let mut client = Client::connect(&url).await;
        // What an editor does: resolve the behavior's node names against the
        // device's functions.
        let index: HashMap<Uuid, ModuleFunction> = client
            .describe()
            .await
            .into_iter()
            .map(|method| {
                (
                    method.id,
                    ModuleFunction {
                        module_id: method.module_id,
                        function_id: method.id,
                        function_name: method.name,
                        function: method.function,
                    },
                )
            })
            .collect();
        let graph = BehaviorTree::try_from_groot_xml(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                 <double x="{in}" res="{out}"/>
               </BehaviorTree></root>"#,
        )
        .expect("the tree parses")
        .into_graph(&index)
        .expect("the tree resolves against the described functions");

        client
            .send(json!({"type": "subscribe", "keys": ["out"]}))
            .await;
        client.next_of("subscribe_resp").await;
        client
            .call(&interpreter_module::encode_load(&graph), "load")
            .await;

        // The load's answer and the first push of `out` may come in either
        // order: the step that applies the load also ticks the behavior.
        let (mut loaded, mut ran) = (None, false);
        while loaded.is_none() || !ran {
            let message = client.next_of_any().await;
            match message["type"].as_str() {
                Some("call_resp") => loaded = Some(message),
                Some("values_changed") => ran |= message["values"]["out"] == json!({"f64": 4.0}),
                _ => {}
            }
        }
        let loaded = loaded.expect("the load answered");
        assert_eq!(loaded["success"], json!(true), "{loaded}");
        assert_eq!(loaded["request_id"], json!("load"));
    })
    .await;
    assert_eq!(
        store.read(&[arora_types::data::Key::from("out")]),
        vec![Some(Value::F64(4.0))]
    );
}

/// A call to a function the module does not have answers `success: false`
/// with the device's reason.
#[tokio::test]
async fn a_call_to_an_unknown_function_answers_an_error() {
    let (mut device, url) = device_with_math(SimpleDataStore::new()).await;
    with_device(&mut device, async {
        let mut client = Client::connect(&url).await;
        let unknown = Uuid::from_u128(0x6d61_7468_ffff);
        client
            .call(
                &Call {
                    module_id: Some(MATH),
                    id: unknown,
                    args: Vec::new(),
                },
                "unknown",
            )
            .await;
        let answer = client.next_of("call_resp").await;
        assert_eq!(answer["success"], json!(false), "{answer}");
        assert_eq!(answer["request_id"], json!("unknown"));
        assert!(answer.get("ret").is_none(), "{answer}");
        let message = answer["message"].as_str().expect("a reason");
        assert!(message.contains(&unknown.to_string()), "{message}");
    })
    .await;
}
