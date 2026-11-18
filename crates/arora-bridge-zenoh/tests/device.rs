//! The Zenoh bridge over a running device: a client session connected to the
//! device's session over a Unix socket, an Arora device with a HAL behind it.
//!
//! What these pin is what a client can count on: the HAL's keys listed with
//! their meta, a write moving what the HAL senses and published to a
//! subscriber, a refused write answered with the device's reason, a function
//! described and called by ids, subscriptions by prefix, values set to none
//! and unset keys, many requests in flight at once, a request sent before the
//! device is known waiting for it, a query that is not a
//! request answered with an error, a large value round trip, the device
//! told whether anyone subscribes, a change made right after subscribing
//! published, requests beyond the device's limit refused, and requests in
//! flight ended with the bridge.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use arora::{Arora, MethodSignature, ModuleBuilder};
use arora_bridge::{Bridge, BridgeOp, Caller, Inbound};
use arora_bridge_zenoh::{Changes, ZenohBridge, ZenohClient};
use arora_hal::FakeHal;
use arora_types::call::{Call, CallError, CallResult};
use arora_types::data::{DataStore, Key, KeyMeta, StateChange};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use arora_types::Uuid;
use futures::StreamExt;

const PREFIX: &str = "robots/test";

/// A device's session listening on a socket file of its own, and a client's
/// session connected to it; the connection is up when this returns.
async fn sessions(test: &str) -> (zenoh::Session, zenoh::Session) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("arora-bridge-zenoh-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a socket directory");
    let endpoint = format!("unixsock-stream/{}", dir.join("z.sock").display());
    let config = |mode: &str, key: &str| {
        let mut config = zenoh::Config::default();
        for (key, value) in [
            ("mode", format!("{mode:?}")),
            (key, format!("[{endpoint:?}]")),
            ("scouting/multicast/enabled", "false".into()),
            ("scouting/gossip/enabled", "false".into()),
        ] {
            config.insert_json5(key, &value).expect("a configuration");
        }
        config
    };
    let device = zenoh::open(config("peer", "listen/endpoints"))
        .await
        .expect("the device's session");
    let client = zenoh::open(config("client", "connect/endpoints"))
        .await
        .expect("the client's session");
    (device, client)
}

/// The device `builder` builds, serving the Zenoh bridge under [`PREFIX`].
async fn device_on(builder: arora::AroraBuilder, session: &zenoh::Session) -> Arora {
    let bridge = ZenohBridge::new(session, PREFIX).await.expect("the bridge");
    builder
        .with_bridge(Box::new(bridge))
        .build()
        .expect("build the device")
}

/// Drive `device` at its default cadence until `client` finishes.
async fn with_device<T>(device: &mut Arora, client: impl std::future::Future<Output = T>) -> T {
    let client = tokio::time::timeout(Duration::from_secs(20), client);
    tokio::select! {
        result = device.run(Arora::DEFAULT_STEP_PERIOD) => panic!("the device stopped: {result:?}"),
        output = client => output.expect("the client finishes within 20 s"),
    }
}

fn joint() -> FakeHal {
    let hal = FakeHal::new();
    hal.add_joints(["hip_yaw"]);
    hal
}

/// The next change holding `key`, and its value there: `Some` when it is
/// set, `None` when it is unset.
async fn next_change(changes: &mut Changes, key: &str) -> Option<Option<Value>> {
    let key = Key::from(key);
    loop {
        let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .unwrap_or_else(|_| panic!("no change of {} within 5 s", key.path))
            .expect("the client is open");
        if let Some(value) = change.set.get(&key) {
            return Some(value.clone());
        }
        if change.unset.contains(&key) {
            return None;
        }
    }
}

async fn next_value(changes: &mut Changes, key: &str) -> Option<Value> {
    next_change(changes, key)
        .await
        .unwrap_or_else(|| panic!("{key} is unset"))
}

/// `list_keys` shows the HAL's inputs open and what it senses closed.
#[tokio::test(flavor = "multi_thread")]
async fn the_hal_s_keys_are_listed_with_their_meta() {
    let (device_session, client_session) = sessions("list").await;
    let mut device = device_on(
        Arora::builder().with_hal(Box::new(joint())),
        &device_session,
    )
    .await;
    let listed = with_device(&mut device, async {
        let (client, _) = ZenohClient::new(&client_session, PREFIX)
            .await
            .expect("the client");
        client
            .ask(BridgeOp::ListKeys {
                prefix: Some("hip_yaw".into()),
            })
            .await
            .expect("the keys")
    })
    .await;
    let keys: Vec<(String, KeyMeta)> =
        arora_types::value_serde::from_value(listed.ret).expect("keys and their meta");
    let keys: Vec<(&str, bool)> = keys
        .iter()
        .map(|(path, meta)| (path.as_str(), meta.editable))
        .collect();
    assert_eq!(
        keys,
        [
            ("hip_yaw.position", false),
            ("hip_yaw.target_position", true),
            ("hip_yaw/target.position", true),
        ]
    );
}

/// A write is acknowledged and moves the sensed position, published to the
/// subscriber. A write the device refuses is answered with its reason. `get`
/// reads what holds.
#[tokio::test(flavor = "multi_thread")]
async fn a_write_moves_the_hal_and_is_published_or_is_refused_with_a_reason() {
    let (device_session, client_session) = sessions("write").await;
    let mut device = device_on(
        Arora::builder().with_hal(Box::new(joint())),
        &device_session,
    )
    .await;
    with_device(&mut device, async {
        let (client, mut changes) = ZenohClient::new(&client_session, PREFIX)
            .await
            .expect("the client");
        client
            .subscribe(vec!["hip_yaw.position".into()])
            .await
            .expect("subscribed");
        let answer = client
            .ask(BridgeOp::Update(StateChange::set(
                "hip_yaw.target_position",
                Value::F64(0.25),
            )))
            .await
            .expect("the write is acknowledged");
        assert_eq!(answer.ret, Value::Unit);
        while next_value(&mut changes, "hip_yaw.position").await != Some(Value::F64(0.25)) {}

        let error = client
            .ask(BridgeOp::Update(StateChange::set(
                "hip_yaw.position",
                Value::F64(1.0),
            )))
            .await
            .expect_err("refused");
        assert!(error.contains("not an input"), "{error}");

        let read = client
            .ask(BridgeOp::Get(vec![
                Key::from("hip_yaw.position"),
                Key::from("nothing/here"),
            ]))
            .await
            .expect("read");
        assert_eq!(
            read.ret,
            Value::ArrayValue(vec![
                Value::Option(Some(Box::new(Value::F64(0.25)))),
                Value::Option(None),
            ])
        );
    })
    .await;
}

const MATH: Uuid = Uuid::from_u128(0x6d61_7468);
const DOUBLE: Uuid = Uuid::from_u128(0x6d61_7468_0001);
const DOUBLE_X: Uuid = Uuid::from_u128(0x6d61_7468_0002);

/// A device with one module, `math`, exporting `double(x: f64) -> f64`.
fn builder_with_math() -> arora::AroraBuilder {
    let signature = Function {
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
    };
    let math = ModuleBuilder::new(MATH)
        .described_function(DOUBLE, "double", signature, |call: Call| {
            match call.args.iter().find(|arg| arg.id == DOUBLE_X) {
                Some(arg) => match *arg.value {
                    Value::F64(x) => Ok(CallResult {
                        ret: Value::F64(2.0 * x),
                        mutated: Vec::new(),
                    }),
                    ref other => Err(CallError::Generic {
                        message: format!("x is not an f64: {other:?}"),
                    }),
                },
                None => Err(CallError::Generic {
                    message: "x is missing".to_string(),
                }),
            }
        })
        .build();
    Arora::builder().with_host_module(math)
}

fn double(x: f64) -> Call {
    Call {
        module_id: Some(MATH),
        id: DOUBLE,
        args: vec![StructureField {
            id: DOUBLE_X,
            value: Box::new(Value::F64(x)),
        }],
    }
}

/// `describe_methods` answers with the module's function under the ids a call
/// names; calls by those ids, many in flight at once, answer each with its own
/// return value, through the client's `Caller`; a failing call answers with
/// the module's reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_function_is_described_and_called_by_ids() {
    let (device_session, client_session) = sessions("call").await;
    let mut device = device_on(builder_with_math(), &device_session).await;
    with_device(&mut device, async {
        let (client, _) = ZenohClient::new(&client_session, PREFIX)
            .await
            .expect("the client");
        let described = client
            .ask(BridgeOp::DescribeMethods { prefix: None })
            .await
            .expect("described");
        let methods: Vec<MethodSignature> =
            arora_types::value_serde::from_value(described.ret).expect("signatures");
        let double_signature = methods
            .iter()
            .find(|method| method.name == "double")
            .expect("double is described");
        assert_eq!(
            (double_signature.module_id, double_signature.id),
            (MATH, DOUBLE)
        );

        let answers =
            futures::future::join_all((0..32).map(|x| client.call(double(x as f64)))).await;
        for (x, answer) in answers.into_iter().enumerate() {
            assert_eq!(answer.expect("double(x)").ret, Value::F64(2.0 * x as f64));
        }

        let mut wrong = double(0.0);
        *wrong.args[0].value = Value::String("two".into());
        let error = client.call(wrong).await.expect_err("refused");
        assert!(format!("{error:?}").contains("not an f64"), "{error:?}");
    })
    .await;
}

/// A subscription receives the keys its prefixes cover, on a `/` boundary,
/// and no other; a key set to no value arrives as set to none, an unset key
/// as unset; a new subscription replaces the old one.
#[tokio::test(flavor = "multi_thread")]
async fn subscriptions_select_keys_by_prefix() {
    let (device_session, client_session) = sessions("prefix").await;
    let store = arora_simple_data_store::SimpleDataStore::new();
    let mut device = device_on(
        Arora::builder().with_data_store(Box::new(store.clone())),
        &device_session,
    )
    .await;
    with_device(&mut device, async {
        let (client, mut changes) = ZenohClient::new(&client_session, PREFIX)
            .await
            .expect("the client");
        client
            .subscribe(vec!["joints".into(), "camera/".into()])
            .await
            .expect("subscribed");
        // A subscriber is matched once the device's session hears of it: write
        // (a new value each time, or it is no change) until a change arrives.
        let mut first = None;
        for i in 0.. {
            if first.is_some() {
                break;
            }
            store
                .write(StateChange::set("joints/head_yaw", Value::F64(i as f64)))
                .unwrap();
            first = tokio::time::timeout(Duration::from_millis(200), changes.next())
                .await
                .ok();
        }

        let mut change = StateChange::new();
        change
            .set
            .insert(Key::from("joints_extra"), Some(Value::F64(9.0)));
        change
            .set
            .insert(Key::from("joints"), Some(Value::F64(2.0)));
        change
            .set
            .insert(Key::from("camera/frame"), Some(Value::U8(1)));
        store.write(change).unwrap();
        // Each key is a change of its own, in no set order.
        let mut seen = HashMap::new();
        while seen.len() < 2 {
            let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
                .await
                .expect("a change within 5 s")
                .expect("the client is open");
            for (key, value) in change.set {
                assert_ne!(key.path, "joints_extra", "not under joints/");
                if key.path == "joints" || key.path == "camera/frame" {
                    seen.insert(key.path, value);
                }
            }
        }
        assert_eq!(
            seen,
            HashMap::from([
                ("joints".to_string(), Some(Value::F64(2.0))),
                ("camera/frame".to_string(), Some(Value::U8(1))),
            ])
        );
        let mut change = StateChange::new();
        change.set.insert(Key::from("camera/frame"), None);
        store.write(change).unwrap();
        assert_eq!(next_change(&mut changes, "camera/frame").await, Some(None));

        let mut unset = StateChange::new();
        unset.unset.insert(Key::from("joints"));
        store.write(unset).unwrap();
        assert_eq!(next_change(&mut changes, "joints").await, None);

        client
            .subscribe(vec!["arm".into()])
            .await
            .expect("subscribed again");
        store
            .write(StateChange::set("joints", Value::F64(3.0)))
            .unwrap();
        store
            .write(StateChange::set("arm/target", Value::F64(4.0)))
            .unwrap();
        let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("a change")
            .expect("the client is open");
        // The old subscription is gone when `subscribe` returns: `joints`,
        // written after, does not arrive before `arm/target`, written next.
        let mut seen = vec![change];
        while !seen
            .last()
            .unwrap()
            .set
            .contains_key(&Key::from("arm/target"))
        {
            seen.push(changes.next().await.expect("the client is open"));
        }
        assert!(
            !seen
                .iter()
                .any(|change| change.set.contains_key(&Key::from("joints"))),
            "{seen:?}"
        );
    })
    .await;
}

/// A large value (a camera frame) set by the client reaches a subscriber whole.
#[tokio::test(flavor = "multi_thread")]
async fn a_large_value_round_trips() {
    let (device_session, client_session) = sessions("large").await;
    let store = arora_simple_data_store::SimpleDataStore::new();
    store
        .set_prefix_meta(HashMap::from([(
            "camera".to_string(),
            KeyMeta {
                editable: true,
                ..KeyMeta::default()
            },
        )]))
        .unwrap();
    let mut device = device_on(
        Arora::builder().with_data_store(Box::new(store)),
        &device_session,
    )
    .await;
    with_device(&mut device, async {
        let (client, mut changes) = ZenohClient::new(&client_session, PREFIX)
            .await
            .expect("the client");
        client
            .subscribe(vec!["camera".into()])
            .await
            .expect("subscribed");
        let frame: Vec<u8> = (0..640 * 480 * 2).map(|i| i as u8).collect();
        loop {
            client
                .ask(BridgeOp::Update(StateChange::set(
                    "camera/frame",
                    Value::ArrayU8(frame.clone()),
                )))
                .await
                .expect("written");
            if let Ok(Some(change)) =
                tokio::time::timeout(Duration::from_millis(500), changes.next()).await
            {
                let value = change.set.get(&Key::from("camera/frame"));
                assert_eq!(value, Some(&Some(Value::ArrayU8(frame))));
                break;
            }
        }
    })
    .await;
}

/// A query that is not a request is answered with an error, and the device
/// goes on answering requests.
#[tokio::test(flavor = "multi_thread")]
async fn a_query_that_is_not_a_request_is_answered_with_an_error() {
    let (device_session, client_session) = sessions("garbage").await;
    let mut device = device_on(
        Arora::builder().with_hal(Box::new(joint())),
        &device_session,
    )
    .await;
    with_device(&mut device, async {
        // Bytes that are not a buffer, and an answer (`err`) sent as a request.
        let answer = Value::ArrayValue(vec![Value::U8(65), Value::String("no".into())]);
        for payload in [
            vec![1, 2, 3],
            arora_buffers::froto_value::serialize(&answer).into_vec(),
        ] {
            let replies = client_session
                .get(format!("{PREFIX}/rpc"))
                .payload(payload)
                .await
                .expect("the query");
            let reply = replies.recv_async().await.expect("a reply");
            let sample = reply.result().expect("an answer");
            let Value::ArrayValue(answer) =
                arora_buffers::froto_value::deserialize(&sample.payload().to_bytes())
            else {
                panic!("an answer is an array");
            };
            assert_eq!(answer.first(), Some(&Value::U8(65)), "an error: {answer:?}");
        }
        let (client, _) = ZenohClient::new(&client_session, PREFIX)
            .await
            .expect("the client");
        client
            .ask(BridgeOp::Get(vec![Key::from("hip_yaw.position")]))
            .await
            .expect("still answering");
    })
    .await;
}

/// The device is told someone wants its changes while a subscriber watches
/// one of its keys, and told no one does once the last one leaves.
#[tokio::test(flavor = "multi_thread")]
async fn the_device_is_told_whether_anyone_subscribes() {
    let (device_session, client_session) = sessions("requested").await;
    let mut bridge = ZenohBridge::new(&device_session, PREFIX)
        .await
        .expect("the bridge");
    let mut inbound = bridge.take_inbound();
    assert!(!requested(&mut inbound).await, "no subscriber yet");
    let (client, _changes) = ZenohClient::new(&client_session, PREFIX)
        .await
        .expect("the client");
    client
        .subscribe(vec!["joints".into()])
        .await
        .expect("subscribed");
    assert!(requested(&mut inbound).await, "a subscriber");
    client.subscribe(Vec::new()).await.expect("unsubscribed");
    // The subscription's own `subscribe` may repeat that someone does.
    while requested(&mut inbound).await {}
}

/// The next `DataRequested` the bridge tells the device.
async fn requested(inbound: &mut arora_bridge::InboundStream) -> bool {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), inbound.next())
            .await
            .expect("an event within 5 s")
            .expect("the bridge is open");
        if let Inbound::DataRequested(requested) = event {
            return requested;
        }
    }
}

/// A key whose path is not a key expression without wildcards is not
/// published, set or unset, and the keys beside it are: a wildcard would reach
/// every key it covers.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_that_is_not_a_key_expression_leaves_the_others_published() {
    let (device_session, client_session) = sessions("refused").await;
    let store = arora_simple_data_store::SimpleDataStore::new();
    let mut device = device_on(
        Arora::builder().with_data_store(Box::new(store.clone())),
        &device_session,
    )
    .await;
    with_device(&mut device, async {
        let (client, mut changes) = ZenohClient::new(&client_session, PREFIX)
            .await
            .expect("the client");
        client
            .subscribe(vec!["arm".into()])
            .await
            .expect("subscribed");
        let mut seen = Vec::new();
        for i in 0.. {
            let mut change = StateChange::set("arm//target", Value::U32(i));
            change.set.insert(Key::from("arm/*"), Some(Value::U32(i)));
            change.set.insert(Key::from("arm/**"), Some(Value::U32(i)));
            change
                .set
                .insert(Key::from("arm/target"), Some(Value::U32(i)));
            store.write(change).unwrap();
            if let Ok(Some(change)) =
                tokio::time::timeout(Duration::from_millis(200), changes.next()).await
            {
                seen.push(change);
                break;
            }
        }
        let mut unset = StateChange::set("arm/next", Value::U32(0));
        unset.unset.insert(Key::from("arm/*"));
        unset.unset.insert(Key::from("arm/**"));
        store.write(unset).unwrap();
        while !seen
            .last()
            .unwrap()
            .set
            .contains_key(&Key::from("arm/next"))
        {
            seen.push(changes.next().await.expect("the client is open"));
        }
        let keys: Vec<(&str, bool)> = seen
            .iter()
            .flat_map(|change| {
                let set = change.set.keys().map(|key| (key.path.as_str(), true));
                set.chain(change.unset.iter().map(|key| (key.path.as_str(), false)))
            })
            .collect();
        // An earlier write may have reached the client too: `arm/target`
        // set, any number of times, then `arm/next`.
        assert!(
            keys.iter()
                .all(|key| *key == ("arm/target", true) || *key == ("arm/next", true)),
            "{keys:?}"
        );
    })
    .await;
}

/// A request sent before the client's session knows the device waits for it,
/// rather than reaching no one.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_sent_before_the_device_is_known_waits_for_it() {
    let (device_session, client_session) = sessions("early").await;
    let (client, _) = ZenohClient::new(&client_session, PREFIX)
        .await
        .expect("the client");
    let asked = tokio::spawn(async move {
        client
            .ask(BridgeOp::Get(vec![Key::from("hip_yaw.position")]))
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut device = device_on(
        Arora::builder().with_hal(Box::new(joint())),
        &device_session,
    )
    .await;
    let answer = with_device(&mut device, asked).await;
    answer.expect("the task").expect("the device answers");
}

/// A device with the open subtree `cmd`.
fn open_store() -> arora_simple_data_store::SimpleDataStore {
    let store = arora_simple_data_store::SimpleDataStore::new();
    store
        .set_prefix_meta(HashMap::from([(
            "cmd".to_string(),
            KeyMeta {
                editable: true,
                ..KeyMeta::default()
            },
        )]))
        .unwrap();
    store
}

/// A write sent right after `subscribe` returns is published to the new
/// subscription: no change is lost while the subscription reaches the device.
#[tokio::test(flavor = "multi_thread")]
async fn a_change_made_right_after_subscribing_is_published() {
    for round in 0..10 {
        let (device_session, client_session) = sessions(&format!("ready-{round}")).await;
        let mut device = device_on(
            Arora::builder().with_data_store(Box::new(open_store())),
            &device_session,
        )
        .await;
        with_device(&mut device, async {
            let (client, mut changes) = ZenohClient::new(&client_session, PREFIX)
                .await
                .expect("the client");
            client
                .subscribe(vec!["cmd".into()])
                .await
                .expect("subscribed");
            client
                .ask(BridgeOp::Update(StateChange::set(
                    "cmd/go",
                    Value::U32(round),
                )))
                .await
                .expect("written");
            assert_eq!(
                next_value(&mut changes, "cmd/go").await,
                Some(Value::U32(round))
            );
        })
        .await;
    }
}

/// Beyond the requests the device holds, a query is answered at once with an
/// error; the others are answered once the device steps.
#[tokio::test(flavor = "multi_thread")]
async fn requests_beyond_the_device_s_limit_are_refused() {
    let (device_session, client_session) = sessions("limit").await;
    let mut bridge = ZenohBridge::new(&device_session, PREFIX)
        .await
        .expect("the bridge");
    // The device holds the inbound stream and reads none of it yet.
    let mut inbound = bridge.take_inbound();
    let (client, _) = ZenohClient::new(&client_session, PREFIX)
        .await
        .expect("the client");
    let client = std::sync::Arc::new(client);
    let held: Vec<_> = (0..ZenohBridge::REQUESTS_IN_FLIGHT)
        .map(|_| {
            let client = client.clone();
            tokio::spawn(async move { client.ask(BridgeOp::Get(Vec::new())).await })
        })
        .collect();
    // Every held request reaches the device before the one beyond.
    let mut commands = Vec::new();
    while commands.len() < ZenohBridge::REQUESTS_IN_FLIGHT {
        match tokio::time::timeout(Duration::from_secs(10), inbound.next())
            .await
            .expect("the requests within 10 s")
            .expect("the bridge is open")
        {
            Inbound::Command(command) => commands.push(command),
            _ => continue,
        }
    }
    let refused = client
        .ask(BridgeOp::Get(Vec::new()))
        .await
        .expect_err("beyond the limit");
    assert!(refused.contains("requests"), "{refused}");
    for command in commands {
        command.reply(Ok(CallResult {
            ret: Value::Unit,
            mutated: Vec::new(),
        }));
    }
    for request in held {
        request.await.expect("the task").expect("answered");
    }
}

/// Dropping the bridge ends the requests it holds: their clients are told no
/// answer comes, rather than waiting for one.
#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_bridge_ends_its_requests_in_flight() {
    let (device_session, client_session) = sessions("dropped").await;
    let mut bridge = ZenohBridge::new(&device_session, PREFIX)
        .await
        .expect("the bridge");
    let mut inbound = bridge.take_inbound();
    let (client, _) = ZenohClient::new(&client_session, PREFIX)
        .await
        .expect("the client");
    let asked = tokio::spawn(async move { client.ask(BridgeOp::Get(Vec::new())).await });
    // Held, unanswered, until the end: only the bridge's drop may end the
    // request.
    let _held = loop {
        if let Inbound::Command(command) = inbound.next().await.expect("the bridge is open") {
            break command;
        }
    };
    drop(bridge);
    let answer = tokio::time::timeout(Duration::from_secs(5), asked)
        .await
        .expect("the client is told within 5 s")
        .expect("the task");
    assert!(answer.is_err(), "{answer:?}");
}
