//! Live-DDS integration tests for the ROS 2 bridge.
//!
//! These create real ROS 2 nodes over DDS and verify end-to-end behaviour:
//! an inbound topic message surfaces as a `BridgeOp::Update` command, and an
//! outbound `try_send` reaches a topic subscriber. Each test uses a random DDS
//! domain to isolate itself.
//!
//! They are ignored on macOS for the same reason as `arora-hal-ros2`'s live
//! tests: DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds
//! 0.11 has no unicast-peer/interface config); they run on Linux CI. To run
//! locally, ensure an active multicast-capable interface and use `--ignored`.

// These live tests drive the DDS backend's ros2-client API directly (QoS
// statics, spinner, fallible topic creation), so they only build under `dds`.
// The Zenoh backend's interop is validated out-of-process against `rmw_zenoh`
// (see examples/zenoh_probe.rs and the interop runbook).
#![cfg(feature = "dds")]

use std::time::Duration;

use arora_bridge::{Bridge, BridgeOp, Inbound};
use arora_bridge_ros2::conversions::topic_name;
use arora_bridge_ros2::msg_types::{self, MessageType};
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type, Value};
use futures::StreamExt;
use rand::RngExt;
use ros2_client::{
    Context, ContextOptions, Name, NodeName, NodeOptions, DEFAULT_PUBLISHER_QOS,
    DEFAULT_SUBSCRIPTION_QOS,
};
use serial_test::serial;

/// Allocate a random DDS domain ID to isolate tests from each other and from
/// any locally-running ROS 2 graph.
fn random_domain_id() -> u16 {
    rand::rng().random_range(1..=200)
}

/// Create a separate ROS 2 node for use as a test peer.
fn create_test_node(domain_id: u16, name_suffix: &str) -> (Context, ros2_client::Node) {
    let ctx = Context::with_options(ContextOptions::new().domain_id(domain_id))
        .expect("failed to create test context");
    let node_name = NodeName::new("/", &format!("test_{name_suffix}")).expect("valid node name");
    let mut node = ctx
        .new_node(node_name, NodeOptions::new())
        .expect("failed to create test node");
    tokio::spawn(node.spinner().unwrap().spin());
    (ctx, node)
}

/// Publishing a Float64 to an input key's topic surfaces as a
/// `BridgeOp::Update` command carrying `Value::F64`.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); these run on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use `--ignored`."
)]
async fn inbound_topic_becomes_update_command() {
    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_in_{domain_id}");

    let config =
        Ros2BridgeConfig::new(&namespace, domain_id).with_input("face/mouth/open", Type::F64);
    let mut bridge = Ros2Bridge::new(config).await;
    let mut inbound = bridge.take_inbound();

    let (_ctx, mut pub_node) = create_test_node(domain_id, &format!("pub_{domain_id}"));
    let topic = Name::parse(&topic_name(&namespace, "face/mouth/open")).expect("valid topic name");
    let pub_topic = pub_node
        .create_topic(
            &topic,
            msg_types::Float64::message_type_name(),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create topic");
    let publisher = pub_node
        .create_publisher::<msg_types::Float64>(&pub_topic, None)
        .expect("create publisher");
    tokio::time::timeout(
        Duration::from_secs(30),
        publisher.wait_for_subscription(&pub_node),
    )
    .await
    .expect("timed out waiting for the bridge to discover the test publisher");

    tokio::spawn(async move {
        loop {
            let _ = publisher
                .async_publish(msg_types::Float64 { data: 0.75 })
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    // Await the Update on the inbound stream. The stream also carries the
    // bridge's startup `DescribeMethods` command (from service discovery) and
    // the initial `DataRequested(true)` signal, in a timing-dependent order — so
    // skip everything that is not the Update we published, rather than assuming
    // the Update is the first command to arrive.
    let change = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match inbound.next().await {
                Some(Inbound::Command(cmd)) => {
                    if let BridgeOp::Update(change) = cmd.op {
                        break change;
                    }
                    // A non-Update command (e.g. DescribeMethods) — keep waiting.
                }
                Some(_) => {} // DataRequested and other non-command signals
                None => panic!("the inbound stream ended before an Update arrived"),
            }
        }
    })
    .await
    .expect("timed out waiting for an Update command");

    assert_eq!(
        change.set.get("face/mouth/open"),
        Some(&Some(Value::F64(0.75)))
    );
}

/// A typed ROS 2 publisher sending an `hri_msgs/Expression` lands the device's
/// key as a structured value — the ROS4HRI inbound acceptance criterion
/// (ARORA-85). The bridge subscribes raw and decodes the CDR against the
/// message's runtime type; the decoded value is byte-exact with the same
/// Expression built through the seeded bridge.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback; runs on Linux CI. \
              To run locally, ensure a multicast-capable interface and use `--ignored`."
)]
async fn a_typed_hri_expression_publisher_lands_the_device_key() {
    use arora_msgs_ros2::{builtin_interfaces, hri_msgs, std_msgs};
    use arora_types::value_serde::bridge::to_value_seeded;
    use arora_types::AroraType;

    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_hri_in_{domain_id}");

    let make_expr = || hri_msgs::Expression {
        header: std_msgs::Header {
            stamp: builtin_interfaces::Time { sec: 0, nanosec: 0 },
            frame_id: "face".into(),
        },
        expression: "happy".into(),
        valence: 0.8,
        arousal: 0.2,
        confidence: 1.0,
    };

    let config = Ros2BridgeConfig::new(&namespace, domain_id)
        .with_typed_input("expression", "hri_msgs/Expression");
    let mut bridge = Ros2Bridge::new(config).await;
    let mut inbound = bridge.take_inbound();

    let (_ctx, mut pub_node) = create_test_node(domain_id, &format!("pub_{domain_id}"));
    let topic = Name::parse(&topic_name(&namespace, "expression")).expect("valid topic name");
    let pub_topic = pub_node
        .create_topic(
            &topic,
            ros2_client::MessageTypeName::new("hri_msgs", "Expression"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create topic");
    let publisher = pub_node
        .create_publisher::<hri_msgs::Expression>(&pub_topic, None)
        .expect("create publisher");
    tokio::time::timeout(
        Duration::from_secs(30),
        publisher.wait_for_subscription(&pub_node),
    )
    .await
    .expect("timed out waiting for the bridge to discover the test publisher");

    tokio::spawn(async move {
        loop {
            let _ = publisher.async_publish(make_expr()).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    let change = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match inbound.next().await {
                Some(Inbound::Command(cmd)) => {
                    if let BridgeOp::Update(change) = cmd.op {
                        if change.set.contains_key("expression") {
                            break change;
                        }
                    }
                }
                Some(_) => {}
                None => panic!("the inbound stream ended before an Update arrived"),
            }
        }
    })
    .await
    .expect("timed out waiting for the expression Update command");

    let (ty, reg) = <hri_msgs::Expression as AroraType>::arora_type_with_registry();
    let expected = to_value_seeded(&make_expr(), &ty, &reg).expect("expression to value");
    assert_eq!(change.set.get("expression"), Some(&Some(expected)));
}

/// Enabling the `ros4hri` exposure profile is all the wiring a face device
/// needs (ARORA-86): a typed publisher on an absolute incumbent topic — here
/// the PAL expression alias and the IIIA look_at alias — fans out onto the
/// `standard/ros4hri/*` keys the face standard reads, fields routed by name
/// and the gaze point coerced to the store's vec3 form.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback; runs on Linux CI. \
              To run locally, ensure a multicast-capable interface and use `--ignored`."
)]
async fn the_ros4hri_profile_fans_typed_topics_onto_face_keys() {
    use arora_bridge_ros2::ExposureProfile;
    use arora_msgs_ros2::{builtin_interfaces, geometry_msgs, hri_msgs, std_msgs};

    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_profile_{domain_id}");

    let config =
        Ros2BridgeConfig::new(&namespace, domain_id).with_profile(ExposureProfile::ros4hri());
    let mut bridge = Ros2Bridge::new(config).await;
    let mut inbound = bridge.take_inbound();

    let (_ctx, mut pub_node) = create_test_node(domain_id, &format!("pub_{domain_id}"));

    let expr_topic = pub_node
        .create_topic(
            &Name::parse("/robot_face/expression").expect("valid topic name"),
            ros2_client::MessageTypeName::new("hri_msgs", "Expression"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create expression topic");
    let expr_publisher = pub_node
        .create_publisher::<hri_msgs::Expression>(&expr_topic, None)
        .expect("create expression publisher");
    let gaze_topic = pub_node
        .create_topic(
            &Name::parse("/expressive_face/look_at").expect("valid topic name"),
            ros2_client::MessageTypeName::new("geometry_msgs", "PointStamped"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create look_at topic");
    let gaze_publisher = pub_node
        .create_publisher::<geometry_msgs::PointStamped>(&gaze_topic, None)
        .expect("create look_at publisher");
    tokio::time::timeout(
        Duration::from_secs(30),
        expr_publisher.wait_for_subscription(&pub_node),
    )
    .await
    .expect("timed out waiting for the bridge to discover the expression publisher");
    tokio::time::timeout(
        Duration::from_secs(30),
        gaze_publisher.wait_for_subscription(&pub_node),
    )
    .await
    .expect("timed out waiting for the bridge to discover the look_at publisher");

    tokio::spawn(async move {
        loop {
            let _ = expr_publisher
                .async_publish(hri_msgs::Expression {
                    header: std_msgs::Header {
                        stamp: builtin_interfaces::Time { sec: 0, nanosec: 0 },
                        frame_id: "face".into(),
                    },
                    expression: "happy".into(),
                    valence: 0.8,
                    arousal: 0.2,
                    confidence: 1.0,
                })
                .await;
            let _ = gaze_publisher
                .async_publish(geometry_msgs::PointStamped {
                    header: std_msgs::Header {
                        stamp: builtin_interfaces::Time { sec: 0, nanosec: 0 },
                        frame_id: "sellion_link".into(),
                    },
                    point: geometry_msgs::Point {
                        x: 0.5,
                        y: -0.25,
                        z: 1.0,
                    },
                })
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    // One expression message fans out atomically; the look_at lands on its
    // own change. Collect until both surfaces arrived.
    let mut expression_change = None;
    let mut gaze_change = None;
    tokio::time::timeout(Duration::from_secs(10), async {
        while expression_change.is_none() || gaze_change.is_none() {
            match inbound.next().await {
                Some(Inbound::Command(cmd)) => {
                    if let BridgeOp::Update(change) = cmd.op {
                        if change.set.contains_key("standard/ros4hri/expression/name") {
                            expression_change = Some(change);
                        } else if change.set.contains_key("standard/ros4hri/gaze/target") {
                            gaze_change = Some(change);
                        }
                    }
                }
                Some(_) => {}
                None => panic!("the inbound stream ended before both surfaces arrived"),
            }
        }
    })
    .await
    .expect("timed out waiting for the profile's fan-out");

    let expression = expression_change.expect("expression change");
    assert_eq!(
        expression.set.get("standard/ros4hri/expression/name"),
        Some(&Some(Value::String("happy".into()))),
    );
    assert_eq!(
        expression.set.get("standard/ros4hri/expression/valence"),
        Some(&Some(Value::F32(0.8))),
    );
    assert_eq!(
        expression.set.get("standard/ros4hri/expression/arousal"),
        Some(&Some(Value::F32(0.2))),
    );

    let gaze = gaze_change.expect("gaze change");
    assert_eq!(
        gaze.set.get("standard/ros4hri/gaze/target"),
        Some(&Some(Value::ArrayF32(vec![0.5, -0.25, 1.0]))),
    );
    assert_eq!(
        gaze.set.get("standard/ros4hri/gaze/frame"),
        Some(&Some(Value::String("sellion_link".into()))),
    );
}

/// `try_send` publishes a changed key to its topic, where a separate node
/// subscribed to that topic receives it.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); these run on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use `--ignored`."
)]
async fn send_data_reaches_topic_subscriber() {
    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_out_{domain_id}");

    let mut bridge = Ros2Bridge::new(Ros2BridgeConfig::new(&namespace, domain_id)).await;

    // The bridge's startup service discovery (`run_node` awaits `discover_services`)
    // sends a `DescribeMethods` command and blocks on its reply *before* it begins
    // publishing. A caller must consume the inbound command stream; draining it
    // here resolves that reply (the command, and with it the reply channel, is
    // taken) so the bridge proceeds to publish. Without this the bridge never
    // publishes and there is nothing for the subscriber to discover.
    let mut inbound = bridge.take_inbound();
    tokio::spawn(async move { while inbound.next().await.is_some() {} });

    // Subscribe on a separate node to the key's topic.
    let (_ctx, mut sub_node) = create_test_node(domain_id, &format!("sub_{domain_id}"));
    let topic = Name::parse(&topic_name(&namespace, "battery/level")).expect("valid topic name");
    let sub_topic = sub_node
        .create_topic(
            &topic,
            msg_types::Float64::message_type_name(),
            &DEFAULT_SUBSCRIPTION_QOS,
        )
        .expect("create topic");
    let subscription = sub_node
        .create_subscription::<msg_types::Float64>(&sub_topic, None)
        .expect("create subscription");

    // Keep publishing until the subscriber sees the value (allow for discovery).
    let publisher = async {
        loop {
            bridge.try_send(&arora_types::data::StateChange::set(
                "battery/level",
                Value::F64(0.42),
            ));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::pin!(publisher);

    let received = tokio::select! {
        _ = &mut publisher => unreachable!("publisher loop never returns"),
        result = async {
            // Wait for the subscriber to discover the bridge's publisher (created
            // on the first `try_send`, driven by the loop above) before timing the
            // delivery — bounded, so a discovery failure fails fast, never hangs.
            tokio::time::timeout(
                Duration::from_secs(30),
                subscription.wait_for_publisher(&sub_node),
            )
            .await
            .expect("timed out waiting for the subscriber to discover the bridge publisher");
            tokio::time::timeout(Duration::from_secs(10), subscription.async_take()).await
        } => result,
    };

    let (msg, _info) = received
        .expect("timed out waiting for the published value")
        .expect("subscription take failed");
    assert!((msg.data - 0.42).abs() < f64::EPSILON, "got {}", msg.data);
}

// =============================================================================
// The action plane, live over DDS.
// =============================================================================

/// The LookAt lifecycle end to end over real DDS: a typed ROS 2 action client
/// against the bridge's raw action server, with the runtime side played by a
/// scripted command handler (DescribeMethods → a `look_at` task-run signature;
/// SPAWN → a task handle; the stop call → halt). The "device" re-writes its
/// run's status/feedback keys every tick like a real interpreter, the client
/// sees EXECUTING + feedback, cancels, and reads the CANCELED result carrying
/// the errno the run wrote.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); these run on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use --ignored."
)]
async fn a_look_at_action_runs_the_full_lifecycle_over_dds() {
    use arora_behavior::interpreter_module;
    use arora_behavior::{TaskHandle, TaskId};
    use arora_behavior_tree_types::{
        STATUS_ENUMERATION_ID, STATUS_FAILURE_VARIANT_ID, STATUS_RUNNING_VARIANT_ID,
    };
    use arora_bridge::MethodSignature;
    use arora_types::call::CallResult;
    use arora_types::data::{Key, StateChange};
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
    use arora_types::record::{FrozenReference, Version};
    use arora_types::value::Enumeration;
    use arora_types::{gen_uuid_from_str, value_serde, Uuid};
    use futures::StreamExt;
    use ros2_client::action_msgs::GoalStatusEnum;
    use ros2_client::{Message, ServiceMapping};
    use serde::{Deserialize, Serialize};

    // The typed client's view of the action.
    #[derive(Serialize, Deserialize, Clone)]
    struct LookAtGoal {
        policy: String,
        x: f64,
    }
    impl Message for LookAtGoal {}
    #[derive(Serialize, Deserialize, Clone)]
    struct LookAtResult {
        errno: i32,
    }
    impl Message for LookAtResult {}
    #[derive(Serialize, Deserialize)]
    struct LookAtFeedback {
        gaze_error: f32,
    }
    impl Message for LookAtFeedback {}

    const ECANCELED: i32 = -125;

    fn status_value(variant: Uuid) -> Value {
        Value::Enumeration(Enumeration {
            id: STATUS_ENUMERATION_ID,
            variant_id: variant,
            value: Box::new(Value::Unit),
        })
    }

    /// A plain method signature (`speak(text) -> f64`) the fake runtime also
    /// describes: it exercises the method-service plane in the same run — a
    /// discriminator between "raw services don't work live" and "the action
    /// assembly is at fault".
    fn speak_signature() -> MethodSignature {
        let text = gen_uuid_from_str("text");
        let mut parameters = std::collections::HashMap::new();
        parameters.insert(
            text,
            Parameter {
                name: "text".to_string(),
                ty: FrozenTy::from(PrimitiveKind::String),
                mutable: false,
            },
        );
        MethodSignature {
            module_id: gen_uuid_from_str("gaze-module"),
            id: gen_uuid_from_str("speak"),
            name: "speak".to_string(),
            function: Function {
                parameters,
                parameter_ordering: vec![text],
                return_ty: FrozenTy::from(PrimitiveKind::F64),
            },
        }
    }

    /// The `look_at` task-run signature the fake runtime describes: params
    /// (policy, x), returning the behavior-tree `Status` (the action marker).
    fn look_at_signature() -> MethodSignature {
        let mut parameters = std::collections::HashMap::new();
        let mut parameter_ordering = Vec::new();
        for (name, kind) in [("policy", PrimitiveKind::String), ("x", PrimitiveKind::F64)] {
            let id = gen_uuid_from_str(name);
            parameter_ordering.push(id);
            parameters.insert(
                id,
                Parameter {
                    name: name.to_string(),
                    ty: FrozenTy::from(kind),
                    mutable: false,
                },
            );
        }
        MethodSignature {
            module_id: gen_uuid_from_str("gaze-module"),
            id: gen_uuid_from_str("look_at"),
            name: "look_at".to_string(),
            function: Function {
                parameters,
                parameter_ordering,
                return_ty: FrozenTy::FrozenScalar(FrozenScalar {
                    reference: FrozenReference {
                        id: STATUS_ENUMERATION_ID,
                        version: Version::parse("1.0.0").unwrap(),
                    },
                }),
            },
        }
    }

    let _ = env_logger::builder()
        .parse_filters("warn")
        .is_test(true)
        .try_init();
    let domain_id = random_domain_id();
    let mut bridge = Ros2Bridge::new(Ros2BridgeConfig::new("robot", domain_id)).await;
    let mut inbound = bridge.take_inbound();

    // The scripted runtime + per-tick device writes, driven from one task like
    // the real step loop. It answers discovery and the SPAWN/halt calls, and —
    // once a run is live — re-writes the run's keys every tick (Running +
    // feedback until halted; then the errno result + the terminal Failure).
    let runtime = async move {
        let prefix = "arora/tasks/gaze/look_at/run1";
        let handle = TaskHandle {
            id: TaskId(Uuid::from_u128(0x51)),
            stop: interpreter_module::encode_halt(TaskId(Uuid::from_u128(0x51))),
            status: Key::from(format!("{prefix}/status")),
            feedback: vec![Key::from(format!("{prefix}/feedback"))],
            result: vec![Key::from(format!("{prefix}/result"))],
            update: vec![Key::from(format!("{prefix}/update"))],
        };
        let mut running = false;
        let mut halted = false;
        loop {
            let event = tokio::select! {
                event = inbound.next() => event,
                _ = tokio::time::sleep(Duration::from_millis(100)), if running => {
                    // A device tick: the run re-writes its keys.
                    let mut change = StateChange::set(
                        handle.status.clone(),
                        status_value(if halted {
                            STATUS_FAILURE_VARIANT_ID
                        } else {
                            STATUS_RUNNING_VARIANT_ID
                        }),
                    );
                    if halted {
                        change
                            .set
                            .insert(handle.result[0].clone(), Some(Value::I32(ECANCELED)));
                    } else {
                        change
                            .set
                            .insert(handle.feedback[0].clone(), Some(Value::F32(0.25)));
                    }
                    bridge.try_send(&change);
                    continue;
                }
            };
            let Some(event) = event else { break };
            let Inbound::Command(cmd) = event else {
                continue; // DataRequested etc.
            };
            match &cmd.op {
                BridgeOp::DescribeMethods { .. } => {
                    eprintln!("[runtime] answering DescribeMethods");
                    let signatures = vec![look_at_signature(), speak_signature()];
                    cmd.reply(Ok(CallResult {
                        ret: value_serde::to_value(&signatures).expect("signatures encode"),
                        mutated: Vec::new(),
                    }));
                }
                BridgeOp::Call(call) if interpreter_module::decode_spawn(call).is_ok() => {
                    let (inner, _policy) = interpreter_module::decode_spawn(call).unwrap();
                    assert_eq!(inner.id, gen_uuid_from_str("look_at"));
                    running = true;
                    eprintln!("[runtime] SPAWN accepted");
                    cmd.reply(Ok(CallResult {
                        ret: interpreter_module::encode_spawn_result(&handle),
                        mutated: Vec::new(),
                    }));
                }
                BridgeOp::Call(call) if call.id == gen_uuid_from_str("speak") => {
                    eprintln!("[runtime] speak called");
                    cmd.reply(Ok(CallResult {
                        ret: Value::F64(1.0),
                        mutated: Vec::new(),
                    }));
                }
                BridgeOp::Call(call) if interpreter_module::decode_halt(call).is_ok() => {
                    eprintln!("[runtime] halt received");
                    halted = true;
                    cmd.reply(Ok(CallResult {
                        ret: Value::Unit,
                        mutated: Vec::new(),
                    }));
                }
                other => panic!("unexpected runtime command: {other:?}"),
            }
        }
    };
    tokio::pin!(runtime);

    // The typed ROS 2 action client.
    let client_flow = async {
        let (_ctx, mut node) = create_test_node(domain_id, "action_client");
        let action_type = ros2_client::ActionTypeName::new("arora", "look_at");
        let action_name = Name::parse("/robot/actions/look_at").expect("valid action name");
        // The reliable service profile ros2-client's own action examples use —
        // the best-effort DEFAULT_SUBSCRIPTION_QOS drops service requests.
        let service_qos = {
            use ros2_client::ros2::{policy, QosPolicyBuilder};
            QosPolicyBuilder::new()
                .reliability(policy::Reliability::Reliable {
                    max_blocking_time: ros2_client::ros2::Duration::from_millis(100),
                })
                .history(policy::History::KeepLast { depth: 4 })
                .durability(policy::Durability::TransientLocal)
                .build()
        };
        let qos = ros2_client::action::ActionClientQosPolicies {
            goal_service: service_qos.clone(),
            result_service: service_qos.clone(),
            cancel_service: service_qos.clone(),
            feedback_subscription: service_qos.clone(),
            status_subscription: service_qos.clone(),
        };
        let client = node
            .create_action_client::<ros2_client::Action<LookAtGoal, LookAtResult, LookAtFeedback>>(
                ServiceMapping::Enhanced,
                &action_name,
                &action_type,
                qos,
            )
            .expect("action client creates");

        // Probe the method-service plane first: a typed client on
        // /robot/methods/speak. If this cannot round-trip, raw dds services
        // are broken live in general — not the action assembly.
        #[derive(Serialize, Deserialize, Clone)]
        struct SpeakRequest {
            text: String,
        }
        impl Message for SpeakRequest {}
        #[derive(Serialize, Deserialize, Debug)]
        struct SpeakResponse {
            result: f64,
        }
        impl Message for SpeakResponse {}
        let speak_client = node
            .create_client::<ros2_client::AService<SpeakRequest, SpeakResponse>>(
                ServiceMapping::Enhanced,
                &Name::parse("/robot/methods/speak").expect("valid service name"),
                &ros2_client::ServiceTypeName::new("arora", "speak"),
                service_qos.clone(),
                service_qos.clone(),
            )
            .expect("speak client creates");
        eprintln!("[client] probing the method service");
        loop {
            let sent = speak_client.async_send_request(SpeakRequest {
                text: "hello".to_string(),
            });
            match tokio::time::timeout(Duration::from_secs(2), sent).await {
                Ok(Ok(req_id)) => {
                    match tokio::time::timeout(
                        Duration::from_secs(2),
                        speak_client.async_receive_response(req_id),
                    )
                    .await
                    {
                        Ok(Ok(response)) => {
                            assert!((response.result - 1.0).abs() < f64::EPSILON);
                            eprintln!("[client] method service OK");
                            break;
                        }
                        other => eprintln!("[client] speak response attempt: {other:?}"),
                    }
                }
                other => eprintln!("[client] speak send attempt: {other:?}"),
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        // Send the goal until the graph connects and the server accepts.
        let goal = LookAtGoal {
            policy: "track".to_string(),
            x: 1.5,
        };
        eprintln!("[client] sending goal");
        let goal_id = loop {
            match tokio::time::timeout(Duration::from_secs(2), client.async_send_goal(goal.clone()))
                .await
            {
                Ok(Ok((goal_id, response))) if response.accepted => break goal_id,
                other => {
                    eprintln!("[client] send_goal attempt: {other:?}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        };
        eprintln!("[client] goal accepted");

        // Feedback flows while the run tracks.
        loop {
            if let Ok(Some(feedback)) = client.receive_feedback(goal_id) {
                eprintln!("[client] feedback received");
                assert!((feedback.gaze_error - 0.25).abs() < f32::EPSILON);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Cancel; the run halts and the result carries the errno.
        eprintln!("[client] canceling");
        client
            .async_cancel_goal(goal_id, ros2_client::builtin_interfaces::Time::ZERO)
            .await
            .expect("cancel round-trips");
        eprintln!("[client] cancel answered; requesting result");
        let (status, result) = client
            .async_request_result(goal_id)
            .await
            .expect("result arrives");
        assert_eq!(status, GoalStatusEnum::Canceled);
        assert_eq!(result.errno, ECANCELED);
    };

    tokio::select! {
        _ = &mut runtime => panic!("the scripted runtime ended early"),
        result = tokio::time::timeout(Duration::from_secs(60), client_flow) => {
            result.expect("the action lifecycle timed out");
        }
    }
}

/// The ros4hri profile's `/skill/look_at` binding end to end over real DDS: a
/// typed `interaction_skills/LookAt` client against the bound action server.
/// The runtime side is scripted (DescribeMethods → the `(policy, target,
/// frame)` task-run signature; SPAWN → a task handle per goal; halt → the
/// run fails on its next tick) — the throwaway stand-in for the real gaze
/// fragment. The client sees the goal become the routed spawn call, feedback
/// on `data_float`, a cancel answered `ROS_ECANCELED`, a preemption by a
/// higher `Meta.priority` answered `ROS_EINTR`, and a lower-priority goal
/// rejected outright.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); these run on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use --ignored."
)]
async fn the_bound_look_at_skill_serves_the_standard_contract() {
    use arora_behavior::interpreter_module;
    use arora_behavior::{TaskHandle, TaskId};
    use arora_behavior_tree_types::{
        STATUS_ENUMERATION_ID, STATUS_FAILURE_VARIANT_ID, STATUS_RUNNING_VARIANT_ID,
    };
    use arora_bridge::MethodSignature;
    use arora_bridge_ros2::ExposureProfile;
    use arora_types::call::CallResult;
    use arora_types::data::{Key, StateChange};
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
    use arora_types::record::{FrozenReference, Version};
    use arora_types::value::Enumeration;
    use arora_types::{gen_uuid_from_str, value_serde, Uuid};
    use futures::StreamExt;
    use ros2_client::action_msgs::GoalStatusEnum;
    use ros2_client::{Message, ServiceMapping};
    use serde::{Deserialize, Serialize};

    // The typed client's view of `interaction_skills/LookAt` — local mirrors
    // of the vendored messages (`ros2_client::Message` is a foreign marker,
    // so the generated structs cannot carry it).
    #[derive(Serialize, Deserialize, Clone)]
    struct Meta {
        caller: String,
        priority: u8,
    }
    #[derive(Serialize, Deserialize, Clone)]
    struct Header {
        stamp: ros2_client::builtin_interfaces::Time,
        frame_id: String,
    }
    #[derive(Serialize, Deserialize, Clone)]
    struct Point {
        x: f64,
        y: f64,
        z: f64,
    }
    #[derive(Serialize, Deserialize, Clone)]
    struct PointStamped {
        header: Header,
        point: Point,
    }
    #[derive(Serialize, Deserialize, Clone)]
    struct LookAtGoal {
        meta: Meta,
        policy: String,
        target: PointStamped,
    }
    impl Message for LookAtGoal {}
    #[derive(Serialize, Deserialize, Debug, Clone)]
    struct SkillResult {
        error_code: u8,
        error_msg: String,
    }
    #[derive(Serialize, Deserialize, Debug, Clone)]
    struct LookAtResult {
        result: SkillResult,
    }
    impl Message for LookAtResult {}
    #[derive(Serialize, Deserialize, Clone)]
    struct SkillFeedback {
        data_bool: bool,
        data_int: u16,
        data_float: f32,
        data_str: String,
    }
    #[derive(Serialize, Deserialize, Clone)]
    struct LookAtFeedback {
        feedback: SkillFeedback,
    }
    impl Message for LookAtFeedback {}

    const ROS_EINTR: u8 = 4;
    const ROS_ECANCELED: u8 = 125;

    fn status_value(variant: Uuid) -> Value {
        Value::Enumeration(Enumeration {
            id: STATUS_ENUMERATION_ID,
            variant_id: variant,
            value: Box::new(Value::Unit),
        })
    }

    /// The `(policy, target, frame)` task-run signature the ros4hri binding
    /// routes onto.
    fn look_at_signature() -> MethodSignature {
        let mut parameters = std::collections::HashMap::new();
        let mut parameter_ordering = Vec::new();
        for (name, kind) in [
            ("policy", PrimitiveKind::String),
            ("target", PrimitiveKind::ArrayF32),
            ("frame", PrimitiveKind::String),
        ] {
            let id = gen_uuid_from_str(name);
            parameter_ordering.push(id);
            parameters.insert(
                id,
                Parameter {
                    name: name.to_string(),
                    ty: FrozenTy::from(kind),
                    mutable: false,
                },
            );
        }
        MethodSignature {
            module_id: gen_uuid_from_str("gaze-module"),
            id: gen_uuid_from_str("look_at"),
            name: "look_at".to_string(),
            function: Function {
                parameters,
                parameter_ordering,
                return_ty: FrozenTy::FrozenScalar(FrozenScalar {
                    reference: FrozenReference {
                        id: STATUS_ENUMERATION_ID,
                        version: Version::parse("1.0.0").unwrap(),
                    },
                }),
            },
        }
    }

    let _ = env_logger::builder()
        .parse_filters("warn")
        .is_test(true)
        .try_init();
    let domain_id = random_domain_id();
    let config = Ros2BridgeConfig::new("robot", domain_id).with_profile(ExposureProfile::ros4hri());
    let mut bridge = Ros2Bridge::new(config).await;
    let mut inbound = bridge.take_inbound();

    // The scripted runtime: one task handle per spawned goal, halted runs
    // fail on their next tick (the halt path of a real interpreter), live
    // ones re-write Running + feedback. The first spawn call is asserted to
    // be the routed goal — policy, the coerced vec3 target, the frame.
    let runtime = async move {
        struct Run {
            handle: TaskHandle,
            halted: bool,
        }
        let mut runs: Vec<Run> = Vec::new();
        let mut spawned = 0u128;
        loop {
            let event = tokio::select! {
                event = inbound.next() => event,
                _ = tokio::time::sleep(Duration::from_millis(100)), if !runs.is_empty() => {
                    let mut change = StateChange::new();
                    for run in &runs {
                        change.set.insert(
                            run.handle.status.clone(),
                            Some(status_value(if run.halted {
                                STATUS_FAILURE_VARIANT_ID
                            } else {
                                STATUS_RUNNING_VARIANT_ID
                            })),
                        );
                        if !run.halted {
                            change
                                .set
                                .insert(run.handle.feedback[0].clone(), Some(Value::F32(0.25)));
                        }
                    }
                    bridge.try_send(&change);
                    continue;
                }
            };
            let Some(event) = event else { break };
            let Inbound::Command(cmd) = event else {
                continue; // DataRequested etc.
            };
            match &cmd.op {
                BridgeOp::DescribeMethods { .. } => {
                    eprintln!("[runtime] answering DescribeMethods");
                    let signatures = vec![look_at_signature()];
                    cmd.reply(Ok(CallResult {
                        ret: value_serde::to_value(&signatures).expect("signatures encode"),
                        mutated: Vec::new(),
                    }));
                }
                BridgeOp::Call(call) if interpreter_module::decode_spawn(call).is_ok() => {
                    let (inner, _policy) = interpreter_module::decode_spawn(call).unwrap();
                    assert_eq!(inner.id, gen_uuid_from_str("look_at"));
                    if spawned == 0 {
                        let arg = |name: &str| {
                            inner
                                .args
                                .iter()
                                .find(|arg| arg.id == gen_uuid_from_str(name))
                                .map(|arg| arg.value.as_ref().clone())
                        };
                        assert_eq!(arg("policy"), Some(Value::String(String::new())));
                        assert_eq!(arg("target"), Some(Value::ArrayF32(vec![0.5, -0.25, 1.0])));
                        assert_eq!(arg("frame"), Some(Value::String("sellion_link".into())));
                    }
                    let task = TaskId(Uuid::from_u128(0x60 + spawned));
                    let prefix = format!("arora/tasks/gaze/look_at/run{spawned}");
                    spawned += 1;
                    let handle = TaskHandle {
                        id: task,
                        stop: interpreter_module::encode_halt(task),
                        status: Key::from(format!("{prefix}/status")),
                        feedback: vec![Key::from(format!("{prefix}/feedback"))],
                        result: vec![Key::from(format!("{prefix}/result"))],
                        update: vec![Key::from(format!("{prefix}/update"))],
                    };
                    eprintln!("[runtime] SPAWN accepted ({prefix})");
                    cmd.reply(Ok(CallResult {
                        ret: interpreter_module::encode_spawn_result(&handle),
                        mutated: Vec::new(),
                    }));
                    runs.push(Run {
                        handle,
                        halted: false,
                    });
                }
                BridgeOp::Call(call) if interpreter_module::decode_halt(call).is_ok() => {
                    let task = interpreter_module::decode_halt(call).unwrap();
                    eprintln!("[runtime] halt received for {task:?}");
                    if let Some(run) = runs.iter_mut().find(|run| run.handle.id == task) {
                        run.halted = true;
                    }
                    cmd.reply(Ok(CallResult {
                        ret: Value::Unit,
                        mutated: Vec::new(),
                    }));
                }
                other => panic!("unexpected runtime command: {other:?}"),
            }
        }
    };
    tokio::pin!(runtime);

    let client_flow = async {
        let (_ctx, mut node) = create_test_node(domain_id, "skill_client");
        let action_type = ros2_client::ActionTypeName::new("interaction_skills", "LookAt");
        let action_name = Name::parse("/skill/look_at").expect("valid action name");
        let service_qos = {
            use ros2_client::ros2::{policy, QosPolicyBuilder};
            QosPolicyBuilder::new()
                .reliability(policy::Reliability::Reliable {
                    max_blocking_time: ros2_client::ros2::Duration::from_millis(100),
                })
                .history(policy::History::KeepLast { depth: 4 })
                .durability(policy::Durability::TransientLocal)
                .build()
        };
        let qos = ros2_client::action::ActionClientQosPolicies {
            goal_service: service_qos.clone(),
            result_service: service_qos.clone(),
            cancel_service: service_qos.clone(),
            feedback_subscription: service_qos.clone(),
            status_subscription: service_qos.clone(),
        };
        let client = node
            .create_action_client::<ros2_client::Action<LookAtGoal, LookAtResult, LookAtFeedback>>(
                ServiceMapping::Enhanced,
                &action_name,
                &action_type,
                qos,
            )
            .expect("action client creates");

        let goal = |priority: u8| LookAtGoal {
            meta: Meta {
                caller: "test".to_string(),
                priority,
            },
            policy: String::new(),
            target: PointStamped {
                header: Header {
                    stamp: ros2_client::builtin_interfaces::Time::ZERO,
                    frame_id: "sellion_link".to_string(),
                },
                point: Point {
                    x: 0.5,
                    y: -0.25,
                    z: 1.0,
                },
            },
        };

        // Goal A: retried until the graph connects and the server accepts.
        eprintln!("[client] sending goal A");
        let goal_a = loop {
            match tokio::time::timeout(Duration::from_secs(2), client.async_send_goal(goal(128)))
                .await
            {
                Ok(Ok((goal_id, response))) if response.accepted => break goal_id,
                other => {
                    eprintln!("[client] send_goal attempt: {other:?}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        };
        eprintln!("[client] goal A accepted");

        // Feedback rides std_skills/Feedback's data_float.
        loop {
            if let Ok(Some(feedback)) = client.receive_feedback(goal_a) {
                eprintln!("[client] feedback received");
                assert!((feedback.feedback.data_float - 0.25).abs() < f32::EPSILON);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Cancel A: the halt lands as the run's Failure, answered Canceled
        // with the ROS_ECANCELED errno in the standard Result message.
        eprintln!("[client] canceling goal A");
        client
            .async_cancel_goal(goal_a, ros2_client::builtin_interfaces::Time::ZERO)
            .await
            .expect("cancel round-trips");
        let (status, result) = client
            .async_request_result(goal_a)
            .await
            .expect("result A arrives");
        assert_eq!(status, GoalStatusEnum::Canceled);
        assert_eq!(result.result.error_code, ROS_ECANCELED);
        eprintln!("[client] goal A canceled with ECANCELED");

        // Goal B at normal priority, then goal C above it: B is preempted —
        // Aborted, with the ROS_EINTR errno.
        let goal_b = loop {
            match tokio::time::timeout(Duration::from_secs(2), client.async_send_goal(goal(128)))
                .await
            {
                Ok(Ok((goal_id, response))) if response.accepted => break goal_id,
                other => {
                    eprintln!("[client] send_goal B attempt: {other:?}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        };
        eprintln!("[client] goal B accepted");
        let (_goal_c, response) = client
            .async_send_goal(goal(200))
            .await
            .expect("goal C sends");
        assert!(
            response.accepted,
            "an equal-or-higher priority goal replaces"
        );
        eprintln!("[client] goal C accepted (preempting B)");
        let (status, result) = client
            .async_request_result(goal_b)
            .await
            .expect("result B arrives");
        assert_eq!(status, GoalStatusEnum::Aborted);
        assert_eq!(result.result.error_code, ROS_EINTR);
        eprintln!("[client] goal B preempted with EINTR");

        // A lower-priority goal is rejected while C runs.
        let (_goal_d, response) = client.async_send_goal(goal(1)).await.expect("goal D sends");
        assert!(!response.accepted, "a lower-priority goal is rejected");
        eprintln!("[client] goal D rejected");
    };

    tokio::select! {
        _ = &mut runtime => panic!("the scripted runtime ended early"),
        result = tokio::time::timeout(Duration::from_secs(60), client_flow) => {
            result.expect("the skill lifecycle timed out");
        }
    }
}
