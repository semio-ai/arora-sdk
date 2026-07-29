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
use rand::Rng;
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

    let _ = env_logger::builder().is_test(true).try_init();
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
                    let signatures = vec![look_at_signature()];
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
            status_subscription: service_qos,
        };
        let client = node
            .create_action_client::<ros2_client::Action<LookAtGoal, LookAtResult, LookAtFeedback>>(
                ServiceMapping::Enhanced,
                &action_name,
                &action_type,
                qos,
            )
            .expect("action client creates");

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
