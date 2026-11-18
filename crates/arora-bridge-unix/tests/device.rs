//! The Unix bridge over a running device: clients on the socket file, an Arora
//! device with a HAL behind it.
//!
//! What these pin is what a client can count on: the HAL's inputs listed open
//! and its outputs closed, a write moving what the HAL senses and pushed back
//! to a subscriber, a refused write answered with the device's reason, a
//! function described and called by ids, subscriptions by prefix, many
//! clients and many requests in flight at once, a client that sends garbage
//! closed without harm to the others, and the socket file's lifecycle.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use arora::{Arora, MethodSignature, ModuleBuilder};
use arora_bridge::{BridgeOp, Caller};
use arora_bridge_unix::{Changes, UnixBridge, UnixClient};
use arora_hal::FakeHal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallError, CallResult};
use arora_types::data::{DataStore, Key, KeyMeta, StateChange};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use arora_types::Uuid;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A fresh directory for one test's socket file.
fn socket_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("arora-bridge-unix-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a socket directory");
    dir
}

/// The device `builder` builds, serving the Unix bridge at `path`.
fn device_at(builder: arora::AroraBuilder, path: &Path) -> Arora {
    let bridge = UnixBridge::bind(path).expect("bind the bridge");
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

async fn connect(path: &Path) -> (UnixClient, Changes) {
    UnixClient::connect(path).await.expect("connect")
}

/// The next change holding `key`, and its value there.
async fn next_value(changes: &mut Changes, key: &str) -> Option<Value> {
    loop {
        let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .unwrap_or_else(|_| panic!("no change of {key} within 5 s"))
            .expect("the connection is open");
        if let Some(value) = change.set.get(&Key::from(key)) {
            return value.clone();
        }
    }
}

/// `list_keys` shows the HAL's inputs — both target forms — open, and what it
/// senses closed.
#[tokio::test]
async fn the_hal_s_keys_are_listed_with_their_meta() {
    let path = socket_dir("list").join("device.sock");
    let mut device = device_at(Arora::builder().with_hal(Box::new(joint())), &path);
    let listed = with_device(&mut device, async {
        let (client, _) = connect(&path).await;
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

/// A write is acknowledged and moves the sensed position, pushed to the
/// subscriber. A write the device refuses is answered with its reason, and a
/// batch holding one refused key is refused whole. `get` reads what holds.
#[tokio::test]
async fn a_write_moves_the_hal_and_is_pushed_or_is_refused_with_a_reason() {
    let path = socket_dir("write").join("device.sock");
    let mut device = device_at(Arora::builder().with_hal(Box::new(joint())), &path);
    with_device(&mut device, async {
        let (client, mut changes) = connect(&path).await;
        client
            .subscribe(vec!["hip_yaw.position".into()])
            .await
            .expect("subscribed");
        for (key, position) in [
            ("hip_yaw/target.position", 0.5),
            ("hip_yaw.target_position", 0.25),
        ] {
            let answer = client
                .ask(BridgeOp::Update(StateChange::set(
                    key,
                    Value::F64(position),
                )))
                .await
                .expect("the write is acknowledged");
            assert_eq!(answer.ret, Value::Unit);
            while next_value(&mut changes, "hip_yaw.position").await != Some(Value::F64(position)) {
            }
        }

        let refused = [
            (
                vec![("hip_yaw.target_position", Value::I64(1))],
                "declared type",
            ),
            (vec![("hip_yaw.position", Value::F64(1.0))], "not an input"),
            (
                vec![
                    ("hip_yaw.target_position", Value::F64(1.0)),
                    ("hip_yaw.position", Value::F64(1.0)),
                ],
                "not an input",
            ),
        ];
        for (values, why) in refused {
            let change = StateChange::from(values);
            let error = client
                .ask(BridgeOp::Update(change.clone()))
                .await
                .expect_err("refused");
            assert!(error.contains(why), "{change:?}: {error}");
        }

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
/// names; a call by those ids answers with its return value, through the
/// client's `Caller` too, and a failing call with the module's reason.
#[tokio::test]
async fn a_module_function_is_described_and_called_by_ids() {
    let path = socket_dir("call").join("device.sock");
    let mut device = device_at(builder_with_math(), &path);
    with_device(&mut device, async {
        let (client, _) = connect(&path).await;
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

        let answer = client.call(double(21.0)).await.expect("double(21)");
        assert_eq!(answer.ret, Value::F64(42.0));

        let mut wrong = double(0.0);
        *wrong.args[0].value = Value::String("two".into());
        let error = client
            .ask(BridgeOp::Call(wrong))
            .await
            .expect_err("a string is refused");
        assert!(error.contains("not an f64"), "{error}");
    })
    .await;
}

/// A connection starts subscribed to nothing; a prefix selects the keys
/// under it, the empty prefix all of them, and no prefix none again.
#[tokio::test]
async fn subscriptions_select_keys_by_prefix() {
    let path = socket_dir("subscribe").join("device.sock");
    let store = SimpleDataStore::new();
    store
        .set_prefix_meta(HashMap::from([(
            "cmd".to_string(),
            KeyMeta::new().editable(),
        )]))
        .expect("open cmd/");
    let mut device = device_at(Arora::builder().with_data_store(Box::new(store)), &path);
    with_device(&mut device, async {
        let (writer, _) = connect(&path).await;
        let (client, mut changes) = connect(&path).await;
        let write = |key: &'static str, x: f64| {
            writer.ask(BridgeOp::Update(StateChange::set(key, Value::F64(x))))
        };

        // Unsubscribed: steps pass, keys change, nothing is pushed.
        write("cmd/a", 1.0).await.expect("write");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            futures::poll!(changes.next()).is_pending(),
            "a connection that subscribed to nothing is sent nothing"
        );

        client
            .subscribe(vec!["cmd/b".into()])
            .await
            .expect("subscribed");
        write("cmd/a", 2.0).await.expect("write");
        write("cmd/b", 3.0).await.expect("write");
        let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("a change")
            .expect("open");
        let keys: Vec<&str> = change.set.keys().map(|k| k.path.as_str()).collect();
        assert_eq!(keys, ["cmd/b"], "only the subscribed prefix: {change:?}");

        // A prefix covers a subtree on a `/` boundary only.
        client
            .subscribe(vec!["cmd".into()])
            .await
            .expect("subscribed");
        write("cmd/a", 4.0).await.expect("write");
        let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("a change")
            .expect("open");
        assert_eq!(
            change.set.get(&Key::from("cmd/a")),
            Some(&Some(Value::F64(4.0)))
        );
        assert!(
            change.set.keys().all(|k| k.path.starts_with("cmd/")),
            "{change:?}"
        );

        // Everything: the clock changes every step.
        client.subscribe(vec!["".into()]).await.expect("subscribed");
        let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("a change")
            .expect("open");
        assert!(
            change.set.keys().any(|k| k.path.starts_with("arora/")),
            "the empty prefix covers the clock: {change:?}"
        );

        // Nothing again; what was already sent drains, then nothing comes.
        client.subscribe(vec![]).await.expect("unsubscribed");
        while tokio::time::timeout(Duration::from_millis(200), changes.next())
            .await
            .is_ok()
        {}
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(futures::poll!(changes.next()).is_pending());
    })
    .await;
}

/// Many clients with many requests in flight each get their own answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_clients_with_requests_in_flight_get_their_own_answers() {
    let path = socket_dir("many").join("device.sock");
    let mut device = device_at(builder_with_math(), &path);
    with_device(&mut device, async {
        let clients = futures::future::join_all((0..8).map(|c| {
            let path = path.clone();
            async move {
                let (client, _) = connect(&path).await;
                let answers = futures::future::join_all(
                    (0..200).map(|i| client.call(double((c * 1000 + i) as f64))),
                )
                .await;
                for (i, answer) in answers.into_iter().enumerate() {
                    let expected = 2.0 * (c * 1000 + i) as f64;
                    assert_eq!(answer.expect("answered").ret, Value::F64(expected));
                }
            }
        }));
        clients.await;
    })
    .await;
}

/// A client that sends bytes that are not a request — or announces a message
/// over the limit — is closed; the device and its other clients go on.
#[tokio::test]
async fn a_client_sending_garbage_is_closed_and_the_others_go_on() {
    let path = socket_dir("garbage").join("device.sock");
    let mut device = device_at(builder_with_math(), &path);
    with_device(&mut device, async {
        let (good, _) = connect(&path).await;
        let not_a_request = arora_buffers::froto_value::serialize(&Value::F64(1.0));
        let over_the_limit = (u32::MAX).to_le_bytes();
        let not_a_buffer = 2u32.to_le_bytes();
        for garbage in [&not_a_request[..], &over_the_limit, &not_a_buffer] {
            let mut raw = tokio::net::UnixStream::connect(&path)
                .await
                .expect("connect");
            raw.write_all(garbage).await.expect("send");
            let mut rest = Vec::new();
            let read = tokio::time::timeout(Duration::from_secs(5), raw.read_to_end(&mut rest))
                .await
                .expect("the device closes the connection");
            assert!(rest.is_empty(), "nothing is answered: {read:?} {rest:?}");
            let answer = good.call(double(1.5)).await.expect("the others go on");
            assert_eq!(answer.ret, Value::F64(3.0));
        }
    })
    .await;
}

/// A client that leaves with requests in flight harms nothing.
#[tokio::test]
async fn a_client_leaving_with_requests_in_flight_harms_nothing() {
    let path = socket_dir("leave").join("device.sock");
    let mut device = device_at(builder_with_math(), &path);
    with_device(&mut device, async {
        for _ in 0..10 {
            let (client, changes) = connect(&path).await;
            client.subscribe(vec!["".into()]).await.expect("subscribed");
            let in_flight =
                futures::future::join_all((0..50).map(|i| client.call(double(i as f64))));
            let _ = tokio::time::timeout(Duration::from_millis(1), in_flight).await;
            drop((client, changes));
        }
        let (client, _) = connect(&path).await;
        assert_eq!(
            client.call(double(4.0)).await.expect("answered").ret,
            Value::F64(8.0)
        );
    })
    .await;
}

/// A client that stops sending, closing its writing half as the relay does
/// when its input ends, is still written the answers it waits for, and the
/// connection then ends.
#[tokio::test]
async fn a_client_that_stops_sending_still_gets_its_answers() {
    use arora_bridge_unix::wire::{encode_request, Message, Request};

    let path = socket_dir("half").join("device.sock");
    let mut device = device_at(builder_with_math(), &path);
    with_device(&mut device, async {
        let mut stream = tokio::net::UnixStream::connect(&path)
            .await
            .expect("connect");
        for id in 0..20u32 {
            let ask = Request::Op {
                id,
                op: BridgeOp::Call(double(id as f64)),
            };
            stream
                .write_all(&encode_request(&ask).unwrap())
                .await
                .expect("a request");
        }
        stream.shutdown().await.expect("stop sending");
        let mut answered = Vec::new();
        for _ in 0..20 {
            let Message::Answer { id, result } = read_message(&mut stream).await else {
                panic!("an answer");
            };
            assert_eq!(result.expect("answered").ret, Value::F64(2.0 * id as f64));
            answered.push(id);
        }
        answered.sort();
        assert_eq!(answered, (0..20).collect::<Vec<_>>());
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).await.expect("the end");
        assert!(rest.is_empty());
    })
    .await;
}

/// A client reaches the device over a relay's standard streams, as through
/// an ssh server that runs a command but forwards no socket file.
#[tokio::test]
async fn a_client_reaches_the_device_over_the_relay_s_standard_streams() {
    let path = socket_dir("relay").join("device.sock");
    let mut device = device_at(builder_with_math(), &path);
    let relay = concat!(env!("CARGO_MANIFEST_DIR"), "/relay.py");
    let python = ["python3", "python"]
        .into_iter()
        .find(|python| {
            std::process::Command::new(python)
                .arg("-V")
                .output()
                .is_ok()
        })
        .expect("a python to run the relay");
    let mut child = tokio::process::Command::new(python)
        .arg(relay)
        .arg(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("run the relay");
    let (client, mut changes) = UnixClient::over(
        child.stdout.take().expect("its stdout"),
        child.stdin.take().expect("its stdin"),
    );
    with_device(&mut device, async {
        let answers =
            futures::future::join_all((0..100).map(|i| client.call(double(i as f64)))).await;
        for (i, answer) in answers.into_iter().enumerate() {
            assert_eq!(answer.expect("answered").ret, Value::F64(2.0 * i as f64));
        }
        client.subscribe(vec!["".into()]).await.expect("subscribed");
        tokio::time::timeout(Duration::from_secs(5), changes.next())
            .await
            .expect("a change through the relay")
            .expect("open");
    })
    .await;
    drop((client, changes));
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("the relay ends when the client does")
        .expect("its status");
    assert!(status.success(), "{status}");
}

/// A large value goes and comes back whole.
#[tokio::test]
async fn a_large_value_round_trips() {
    let path = socket_dir("large").join("device.sock");
    let store = SimpleDataStore::new();
    store
        .set_prefix_meta(HashMap::from([(String::new(), KeyMeta::new().editable())]))
        .expect("open everything");
    let mut device = device_at(Arora::builder().with_data_store(Box::new(store)), &path);
    with_device(&mut device, async {
        let (client, _) = connect(&path).await;
        let big = Value::ArrayF32((0..2_000_000).map(|i| i as f32).collect());
        client
            .ask(BridgeOp::Update(StateChange::set("big", big.clone())))
            .await
            .expect("written");
        let read = client
            .ask(BridgeOp::Get(vec![Key::from("big")]))
            .await
            .expect("read");
        assert_eq!(
            read.ret,
            Value::ArrayValue(vec![Value::Option(Some(Box::new(big)))])
        );
    })
    .await;
}

/// The device ends the connection when it stops: the client's requests fail
/// and its changes end.
#[tokio::test]
async fn a_stopped_device_ends_its_clients_connections() {
    let path = socket_dir("stop").join("device.sock");
    let mut device = device_at(builder_with_math(), &path);
    let (client, mut changes) = with_device(&mut device, async {
        let (client, changes) = connect(&path).await;
        client.subscribe(vec!["".into()]).await.expect("subscribed");
        (client, changes)
    })
    .await;
    drop(device);
    while tokio::time::timeout(Duration::from_secs(5), changes.next())
        .await
        .expect("the changes end")
        .is_some()
    {}
    assert!(client.call(double(1.0)).await.is_err());
    assert!(!path.exists(), "the device removes its socket file");
}

/// The socket file is its owner's alone; a socket file no device answers on
/// is replaced, one a device answers on is not, nor is a file of another
/// kind; a bridge removes its own file, never one that replaced it.
#[tokio::test]
async fn the_socket_file_s_lifecycle() {
    let dir = socket_dir("file");
    let path = dir.join("nested/device.sock");

    let first = UnixBridge::bind(&path).expect("bind, creating the directory");
    let mode = std::fs::metadata(&path)
        .expect("the file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let beside: Vec<_> = std::fs::read_dir(path.parent().expect("a directory"))
        .expect("the directory")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    assert_eq!(beside, ["device.sock"], "the private directory is gone");
    let error = UnixBridge::bind(&path)
        .err()
        .expect("a device answers there");
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);

    // A file left behind: a socket nothing listens on.
    drop(first);
    assert!(!path.exists());
    let stale = std::os::unix::net::UnixListener::bind(&path).expect("a stale socket");
    drop(stale);
    assert!(path.exists());
    let second = UnixBridge::bind(&path).expect("replaces a stale file");

    // A path at the length a socket address holds on Linux binds, although
    // the private directory beside it makes a longer one.
    #[cfg(target_os = "linux")]
    {
        let room = 107 - dir.as_os_str().len() - "/device.sock".len() - 1;
        let long = dir.join("d".repeat(room)).join("device.sock");
        assert_eq!(long.as_os_str().len(), 107);
        let near_limit = UnixBridge::bind(&long).expect("a path at the limit binds");
        std::os::unix::net::UnixStream::connect(&long).expect("and is reached");
        drop(near_limit);
    }

    // A path too long for a client to connect to is refused.
    let too_long = dir.join("d".repeat(200)).join("device.sock");
    let error = UnixBridge::bind(&too_long)
        .err()
        .expect("too long to reach");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

    // A file of another kind, at a mistyped path, is not a device's.
    let other = dir.join("notes.txt");
    std::fs::write(&other, "kept").expect("a file");
    let error = UnixBridge::bind(&other).err().expect("not a socket file");
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&other).expect("kept"), "kept");

    // A bridge whose file was replaced leaves the replacement alone.
    std::fs::remove_file(&path).expect("remove");
    let third = UnixBridge::bind(&path).expect("bind again");
    drop(second);
    assert!(path.exists(), "the replacement stays");
    drop(third);
    assert!(!path.exists());
}

/// A lock on the socket's directory held by someone else fails the bind
/// after a bounded wait, never hangs it.
#[tokio::test]
async fn a_directory_locked_by_another_fails_the_bind() {
    use std::os::fd::AsRawFd;
    let dir = socket_dir("locked");
    let held = std::fs::File::open(&dir).expect("the directory");
    // SAFETY: `held` is an open file descriptor for the call's duration.
    assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
    let error = UnixBridge::bind(dir.join("device.sock"))
        .err()
        .expect("the lock stays held");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    drop(held);
    UnixBridge::bind(dir.join("device.sock")).expect("binds once released");
}

/// Read one message off a raw connection.
async fn read_message(stream: &mut tokio::net::UnixStream) -> arora_bridge_unix::wire::Message {
    let mut len = [0; 4];
    stream.read_exact(&mut len).await.expect("a length");
    let mut bytes = vec![0; u32::from_le_bytes(len) as usize];
    bytes[..4].copy_from_slice(&len);
    stream.read_exact(&mut bytes[4..]).await.expect("a message");
    arora_bridge_unix::wire::decode_message(&bytes).expect("a message")
}

/// A client reading slower than the device changes keys it pushes newest
/// only (camera frames) is sent every step's other keys and the newest frame,
/// and stays connected: frames are replaced, not queued, so the backlog never
/// reaches its cap.
#[tokio::test]
async fn a_slow_client_skips_frames_and_keeps_every_record() {
    use arora_bridge::Bridge;
    use arora_bridge_unix::wire::{encode_request, Message, Request};

    let path = socket_dir("slow").join("device.sock");
    let mut bridge = UnixBridge::bind(&path).expect("bind the bridge");
    let mut stream = tokio::net::UnixStream::connect(&path)
        .await
        .expect("connect");
    let subscribe = Request::Subscribe {
        id: 7,
        prefixes: vec!["".into()],
        latest: vec!["camera".into()],
    };
    stream
        .write_all(&encode_request(&subscribe).unwrap())
        .await
        .expect("subscribe");
    assert!(matches!(
        read_message(&mut stream).await,
        Message::Answer {
            id: 7,
            result: Ok(_)
        }
    ));

    // 400 steps of a 256 KiB frame, 100 MiB in all, with the client not
    // reading: more than the backlog's cap, had the frames been queued.
    let frame = 256 << 10;
    for i in 0..400u32 {
        let mut change = StateChange::new();
        change
            .set
            .insert(Key::from("dcm/stamp"), Some(Value::U32(i)));
        change.set.insert(
            Key::from("camera/frame"),
            Some(Value::ArrayU8(vec![i as u8; frame])),
        );
        bridge.try_send(&change);
        tokio::task::yield_now().await;
    }

    let mut stamps = Vec::new();
    let mut frames = Vec::new();
    while stamps.last() != Some(&399) {
        let Message::Changed(change) = read_message(&mut stream).await else {
            panic!("a change");
        };
        if let Some(Some(Value::U32(stamp))) = change.set.get(&Key::from("dcm/stamp")) {
            stamps.push(*stamp);
        }
        if let Some(Some(Value::ArrayU8(bytes))) = change.set.get(&Key::from("camera/frame")) {
            frames.push(bytes[0]);
        }
    }
    assert_eq!(
        stamps,
        (0..400).collect::<Vec<_>>(),
        "every record, in order"
    );
    assert!(frames.len() < 100, "frames skipped: {} sent", frames.len());
    assert_eq!(frames.last(), Some(&(399u32 as u8)), "the newest frame");
}
