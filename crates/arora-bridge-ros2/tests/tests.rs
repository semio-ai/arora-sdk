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
use arora_types::call::CallResult;
use arora_types::data::KeyMeta;
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

    let mut bridge = Ros2Bridge::new(Ros2BridgeConfig::new(&namespace, domain_id)).await;
    let mut inbound = bridge.take_inbound();

    // The device: it opened `face/mouth/open` to remote writers, and hands every
    // Update to the test. It answers concurrently, because the bridge asks for
    // the device's inputs before it subscribes them — the publisher below waits
    // on that subscription.
    let (updates_tx, mut updates) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = inbound.next().await {
            let Inbound::Command(cmd) = event else {
                continue; // DataRequested and other non-command signals
            };
            match &cmd.op {
                BridgeOp::ListKeys { .. } => {
                    let inputs = vec![(
                        "face/mouth/open".to_string(),
                        KeyMeta::new().editable().of_type(Type::F64),
                    )];
                    cmd.reply(
                        arora_types::value_serde::to_value(&inputs)
                            .map(|ret| CallResult {
                                ret,
                                mutated: Vec::new(),
                            })
                            .map_err(|e| e.to_string()),
                    );
                }
                BridgeOp::Update(change) => {
                    let _ = updates_tx.send(change.clone());
                }
                // DescribeMethods and the rest: this device describes none.
                _ => {}
            }
        }
    });

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

    let change = tokio::time::timeout(Duration::from_secs(10), updates.recv())
        .await
        .expect("timed out waiting for an Update command")
        .expect("the device task stopped before an Update arrived");

    assert_eq!(
        change.set.get("face/mouth/open"),
        Some(&Some(Value::F64(0.75)))
    );
}

/// A writer of another type on an input's topic does not cost the input its
/// subscription. RustDDS matches it to the bridge's `Float64` reader by the
/// topic name alone, so its `Int32` messages reach the reader and fail to decode;
/// each is dropped, more of them than the subscription tolerates receive
/// failures in a row, and a `Float64` published afterwards still surfaces as
/// an `Update`.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); these run on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use `--ignored`."
)]
async fn messages_of_another_type_leave_the_input_subscribed() {
    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_mistyped_{domain_id}");

    let mut bridge = Ros2Bridge::new(Ros2BridgeConfig::new(&namespace, domain_id)).await;
    let mut inbound = bridge.take_inbound();

    // The device: it opened `face/mouth/open`, an F64, to remote writers, and
    // hands every Update to the test.
    let (updates_tx, mut updates) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = inbound.next().await {
            let Inbound::Command(cmd) = event else {
                continue;
            };
            match &cmd.op {
                BridgeOp::ListKeys { .. } => {
                    let inputs = vec![(
                        "face/mouth/open".to_string(),
                        KeyMeta::new().editable().of_type(Type::F64),
                    )];
                    cmd.reply(
                        arora_types::value_serde::to_value(&inputs)
                            .map(|ret| CallResult {
                                ret,
                                mutated: Vec::new(),
                            })
                            .map_err(|e| e.to_string()),
                    );
                }
                BridgeOp::Update(change) => {
                    let _ = updates_tx.send(change.clone());
                }
                _ => {}
            }
        }
    });

    let (_ctx, mut pub_node) = create_test_node(domain_id, &format!("pub_{domain_id}"));
    let topic = Name::parse(&topic_name(&namespace, "face/mouth/open")).expect("valid topic name");

    let mistyped_topic = pub_node
        .create_topic(
            &topic,
            msg_types::Int32::message_type_name(),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create the Int32 topic");
    let mistyped = pub_node
        .create_publisher::<msg_types::Int32>(&mistyped_topic, None)
        .expect("create the Int32 publisher");
    tokio::time::timeout(
        Duration::from_secs(30),
        mistyped.wait_for_subscription(&pub_node),
    )
    .await
    .expect("timed out waiting for the bridge's reader to match the Int32 writer");
    for data in 0..10 {
        let _ = mistyped.async_publish(msg_types::Int32 { data }).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Long enough for the reader to have taken every one of them.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // A node of its own: one participant holds one type per topic name.
    let (_float_ctx, mut float_node) = create_test_node(domain_id, &format!("pub2_{domain_id}"));
    let float_topic = float_node
        .create_topic(
            &topic,
            msg_types::Float64::message_type_name(),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create the Float64 topic");
    let publisher = float_node
        .create_publisher::<msg_types::Float64>(&float_topic, None)
        .expect("create the Float64 publisher");
    tokio::time::timeout(
        Duration::from_secs(30),
        publisher.wait_for_subscription(&float_node),
    )
    .await
    .expect("timed out waiting for the bridge to discover the Float64 publisher");
    tokio::spawn(async move {
        loop {
            let _ = publisher
                .async_publish(msg_types::Float64 { data: 0.75 })
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    let change = tokio::time::timeout(Duration::from_secs(10), updates.recv())
        .await
        .expect("no Update after messages of another type: the input lost its subscription")
        .expect("the device task stopped before an Update arrived");
    assert_eq!(
        change.set.get("face/mouth/open"),
        Some(&Some(Value::F64(0.75)))
    );
}

/// A typed ROS 2 publisher sending an `interaction_skills/SetExpression`, the
/// `hri_msgs/Expression` it wraps included, lands the device's key as a
/// structured value — the ROS4HRI inbound acceptance criterion
/// (ARORA-85). The bridge subscribes raw and decodes the CDR against the
/// message's runtime type; the decoded value is byte-exact with the same
/// SetExpression built through the seeded bridge.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback; runs on Linux CI. \
              To run locally, ensure a multicast-capable interface and use `--ignored`."
)]
async fn a_typed_hri_expression_publisher_lands_the_device_key() {
    use arora_msgs_ros2::{builtin_interfaces, hri_msgs, interaction_skills, std_msgs, std_skills};
    use arora_types::value_serde::bridge::to_value_seeded;
    use arora_types::AroraType;

    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_hri_in_{domain_id}");

    let make_expr = || interaction_skills::SetExpression {
        meta: std_skills::Meta {
            caller: "test".into(),
            priority: std_skills::Meta::NORMAL_PRIORITY,
        },
        expression: hri_msgs::Expression {
            header: std_msgs::Header {
                stamp: builtin_interfaces::Time { sec: 0, nanosec: 0 },
                frame_id: "face".into(),
            },
            expression: "happy".into(),
            valence: 0.8,
            arousal: 0.2,
            confidence: 1.0,
        },
    };

    let config = Ros2BridgeConfig::new(&namespace, domain_id)
        .with_typed_input("expression", "interaction_skills/SetExpression");

    let mut bridge = Ros2Bridge::new(config).await;
    let mut inbound = bridge.take_inbound();

    let (_ctx, mut pub_node) = create_test_node(domain_id, &format!("pub_{domain_id}"));

    let topic = Name::parse(&topic_name(&namespace, "expression")).expect("valid topic name");

    let pub_topic = pub_node
        .create_topic(
            &topic,
            ros2_client::MessageTypeName::new("interaction_skills", "SetExpression"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create topic");

    let publisher = pub_node
        .create_publisher::<interaction_skills::SetExpression>(&pub_topic, None)
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

    let (ty, reg) = <interaction_skills::SetExpression as AroraType>::arora_type_with_registry();

    let expected = to_value_seeded(&make_expr(), &ty, &reg).expect("SetExpression to value");

    assert_eq!(change.set.get("expression"), Some(&Some(expected)));
}

/// Enabling the `ros4hri` exposure profile is all the wiring a face device
/// needs (ARORA-86): a typed publisher on an absolute incumbent topic — here
/// the expression skill topic, the PAL look_at topic, and both shapes a TTS
/// node streams a viseme in — fans out onto the `standard/ros4hri/*` keys the
/// face standard reads, fields routed by name, the gaze point coerced to the
/// store's vec3 form, and the sequence indexed at its first element.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback; runs on Linux CI. \
              To run locally, ensure a multicast-capable interface and use `--ignored`."
)]
async fn the_ros4hri_profile_fans_typed_topics_onto_face_keys() {
    use arora_bridge_ros2::ExposureProfile;
    use arora_msgs_ros2::{
        builtin_interfaces, geometry_msgs, hri_msgs, interaction_skills, std_msgs, std_skills,
    };

    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_profile_{domain_id}");

    // The expression lands on its keys here; served as a skill, it spawns the
    // device's `set_expression` (covered by the topic-skill unit tests).
    let config = Ros2BridgeConfig::new(&namespace, domain_id)
        .with_profile(ExposureProfile::ros4hri().with_set_expression_on_keys());
    let mut bridge = Ros2Bridge::new(config).await;
    let mut inbound = bridge.take_inbound();

    let (_ctx, mut pub_node) = create_test_node(domain_id, &format!("pub_{domain_id}"));

    let expr_topic = pub_node
        .create_topic(
            &Name::parse("/skill/set_expression").expect("valid topic name"),
            ros2_client::MessageTypeName::new("interaction_skills", "SetExpression"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create expression topic");
    let expr_publisher = pub_node
        .create_publisher::<interaction_skills::SetExpression>(&expr_topic, None)
        .expect("create expression publisher");
    let gaze_topic = pub_node
        .create_topic(
            &Name::parse("/robot_face/look_at").expect("valid topic name"),
            ros2_client::MessageTypeName::new("geometry_msgs", "PointStamped"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create look_at topic");
    let gaze_publisher = pub_node
        .create_publisher::<geometry_msgs::PointStamped>(&gaze_topic, None)
        .expect("create look_at publisher");
    let viseme_topic = pub_node
        .create_topic(
            &Name::parse("/tts/viseme").expect("valid topic name"),
            ros2_client::MessageTypeName::new("hri_msgs", "Viseme"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create viseme topic");
    let viseme_publisher = pub_node
        .create_publisher::<hri_msgs::Viseme>(&viseme_topic, None)
        .expect("create viseme publisher");
    let visemes_topic = pub_node
        .create_topic(
            &Name::parse("/tts/visemes").expect("valid topic name"),
            ros2_client::MessageTypeName::new("hri_msgs", "Visemes"),
            &DEFAULT_PUBLISHER_QOS,
        )
        .expect("create visemes topic");
    let visemes_publisher = pub_node
        .create_publisher::<hri_msgs::Visemes>(&visemes_topic, None)
        .expect("create visemes publisher");
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
    tokio::time::timeout(
        Duration::from_secs(30),
        viseme_publisher.wait_for_subscription(&pub_node),
    )
    .await
    .expect("timed out waiting for the bridge to discover the viseme publisher");
    tokio::time::timeout(
        Duration::from_secs(30),
        visemes_publisher.wait_for_subscription(&pub_node),
    )
    .await
    .expect("timed out waiting for the bridge to discover the visemes publisher");

    tokio::spawn(async move {
        loop {
            let _ = expr_publisher
                .async_publish(interaction_skills::SetExpression {
                    meta: std_skills::Meta {
                        caller: "test".into(),
                        priority: std_skills::Meta::NORMAL_PRIORITY,
                    },
                    expression: hri_msgs::Expression {
                        header: std_msgs::Header {
                            stamp: builtin_interfaces::Time { sec: 0, nanosec: 0 },
                            frame_id: "face".into(),
                        },
                        expression: "happy".into(),
                        valence: 0.8,
                        arousal: 0.2,
                        confidence: 1.0,
                    },
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
            // Distinct shapes, so each change says which topic it came from.
            let _ = viseme_publisher
                .async_publish(hri_msgs::Viseme {
                    value: hri_msgs::Viseme::PP,
                    time: 0.0,
                    duration: 0.1,
                })
                .await;
            let _ = visemes_publisher
                .async_publish(hri_msgs::Visemes {
                    visemes: vec![
                        hri_msgs::Viseme {
                            value: hri_msgs::Viseme::OU,
                            time: 0.2,
                            duration: 0.1,
                        },
                        hri_msgs::Viseme {
                            value: hri_msgs::Viseme::AA,
                            time: 0.3,
                            duration: 0.1,
                        },
                    ],
                })
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    // One expression message fans out atomically; the look_at and each viseme
    // land on their own change. Collect until every surface arrived.
    let mut expression_change = None;
    let mut gaze_change = None;
    let mut viseme = None;
    let mut sequence_viseme = None;
    tokio::time::timeout(Duration::from_secs(10), async {
        while expression_change.is_none()
            || gaze_change.is_none()
            || viseme.is_none()
            || sequence_viseme.is_none()
        {
            match inbound.next().await {
                Some(Inbound::Command(cmd)) => {
                    if let BridgeOp::Update(change) = cmd.op {
                        if change.set.contains_key("standard/ros4hri/expression/name") {
                            expression_change = Some(change);
                        } else if change.set.contains_key("standard/ros4hri/gaze/target") {
                            gaze_change = Some(change);
                        } else if let Some(Some(shape)) =
                            change.set.get("standard/ros4hri/viseme").cloned()
                        {
                            // Its time and duration land in the same change.
                            let at = |key: &str| change.set.get(key).cloned().flatten();
                            let landed = (
                                shape.clone(),
                                at("standard/ros4hri/viseme/time"),
                                at("standard/ros4hri/viseme/duration"),
                            );
                            if shape == Value::U8(hri_msgs::Viseme::PP) {
                                viseme = Some(landed);
                            } else {
                                sequence_viseme = Some(landed);
                            }
                        }
                    }
                }
                Some(_) => {}
                None => panic!("the inbound stream ended before every surface arrived"),
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

    // Both shapes reach the one key, the sequence at its first element.
    assert_eq!(
        viseme,
        Some((
            Value::U8(hri_msgs::Viseme::PP),
            Some(Value::F32(0.0)),
            Some(Value::F32(0.1)),
        ))
    );
    assert_eq!(
        sequence_viseme,
        Some((
            Value::U8(hri_msgs::Viseme::OU),
            Some(Value::F32(0.2)),
            Some(Value::F32(0.1)),
        ))
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

    // The bridge's discovery at start (`run_node` asks the device for its
    // inputs and functions) sends `ListKeys` and `DescribeMethods` commands and
    // waits for their replies *before* it begins publishing. A caller must consume the inbound command stream; draining it
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

/// The ROS4HRI preset's speech surface, live: the device writes the speech
/// state key and a `std_msgs/String` subscriber on `/robot_face/speech` reads
/// the utterance — the key composed into the message's `data` field by the
/// outbound route, on the absolute topic the profile names.
#[tokio::test]
#[serial]
#[cfg_attr(
    target_os = "macos",
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); these run on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use `--ignored`."
)]
async fn the_ros4hri_profile_publishes_the_speech_text_as_a_string_topic() {
    let _ = env_logger::try_init();
    let domain_id = random_domain_id();
    let namespace = format!("test_speech_{domain_id}");

    let mut bridge = Ros2Bridge::new(
        Ros2BridgeConfig::new(&namespace, domain_id)
            .with_profile(arora_bridge_ros2::ExposureProfile::ros4hri()),
    )
    .await;
    // Drain the inbound stream so the startup discovery resolves and the
    // bridge proceeds to publish (see `send_data_reaches_topic_subscriber`).
    let mut inbound = bridge.take_inbound();
    tokio::spawn(async move { while inbound.next().await.is_some() {} });

    let (_ctx, mut sub_node) = create_test_node(domain_id, &format!("subtitles_{domain_id}"));
    let topic = Name::parse("/robot_face/speech").expect("valid topic name");
    // The profile publishes state as sensor data (best-effort); the reader
    // asks for the same, as a subtitle node would.
    let sub_topic = sub_node
        .create_topic(
            &topic,
            msg_types::String::message_type_name(),
            &DEFAULT_SUBSCRIPTION_QOS,
        )
        .expect("create topic");
    let subscription = sub_node
        .create_subscription::<msg_types::String>(&sub_topic, None)
        .expect("create subscription");

    let publisher = async {
        loop {
            bridge.try_send(&arora_types::data::StateChange::set(
                "standard/ros4hri/speech/text",
                Value::String("hello there".into()),
            ));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::pin!(publisher);

    let received = tokio::select! {
        _ = &mut publisher => unreachable!("publisher loop never returns"),
        result = async {
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
        .expect("timed out waiting for the published utterance")
        .expect("subscription take failed");
    assert_eq!(msg.data, "hello there");
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
                BridgeOp::ListKeys { .. } => {
                    // This device opens no inputs: its actions are its whole surface.
                    let inputs: Vec<(String, KeyMeta)> = Vec::new();
                    cmd.reply(Ok(CallResult {
                        ret: value_serde::to_value(&inputs).expect("keys encode"),
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
        // The service profile a native rclcpp/rclpy action client runs
        // (`rmw_qos_profile_services_default`): reliable — the best-effort
        // DEFAULT_SUBSCRIPTION_QOS drops service requests — and volatile, so
        // this client matches the server exactly as a native one would.
        let service_qos = {
            use ros2_client::ros2::{policy, QosPolicyBuilder};
            QosPolicyBuilder::new()
                .reliability(policy::Reliability::Reliable {
                    max_blocking_time: ros2_client::ros2::Duration::from_millis(100),
                })
                .history(policy::History::KeepLast { depth: 4 })
                .durability(policy::Durability::Volatile)
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
                BridgeOp::ListKeys { .. } => {
                    // This device opens no inputs: its actions are its whole surface.
                    let inputs: Vec<(String, KeyMeta)> = Vec::new();
                    cmd.reply(Ok(CallResult {
                        ret: value_serde::to_value(&inputs).expect("keys encode"),
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
                .durability(policy::Durability::Volatile)
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

/// An action's readers that stop acknowledging. Each test here runs a second
/// process — this test binary, running [`silent_reader`](unacknowledged_readers::silent_reader)
/// — whose reliable readers match the bridge's `look_at` action writers and
/// are then stopped with SIGSTOP. A stopped process stays matched for its
/// participant lease but never acknowledges, so each writer it reads keeps a
/// full send window. The bridge runs on worker threads apart from the
/// scripted device, so a write that blocks its thread cannot slow the
/// device's ticks and hide the delay.
mod unacknowledged_readers {
    use super::*;

    use std::io::BufRead;
    use std::process::{Child, Command, Stdio};

    use arora_behavior::interpreter_module;
    use arora_behavior::{TaskHandle, TaskId};
    use arora_behavior_tree_types::{
        STATUS_ENUMERATION_ID, STATUS_RUNNING_VARIANT_ID, STATUS_SUCCESS_VARIANT_ID,
    };
    use arora_bridge::MethodSignature;
    use arora_types::data::{Key, StateChange};
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
    use arora_types::record::{FrozenReference, Version};
    use arora_types::value::Enumeration;
    use arora_types::{gen_uuid_from_str, value_serde, Uuid};
    use ros2_client::action_msgs::GoalStatusEnum;
    use ros2_client::{Message, ServiceMapping};
    use serde::{Deserialize, Serialize};
    use tokio::sync::oneshot;
    use tokio::time::Instant;

    /// The DDS domain [`silent_reader`] joins; set only by the tests that run
    /// it as a separate process.
    const SILENT_READER_DOMAIN: &str = "ARORA_SILENT_READER_DOMAIN";
    /// The `look_at` endpoints [`silent_reader`] reads, comma-separated:
    /// `feedback`, `status`, `result` (the `GetResult` replies).
    const SILENT_READER_ENDPOINTS: &str = "ARORA_SILENT_READER_ENDPOINTS";
    /// The line [`silent_reader`] prints once each of its readers holds a
    /// sample.
    const SILENT_READER_READY: &str = "silent-reader: matched";

    #[derive(Serialize, Deserialize, Clone)]
    struct LookAtGoal {
        x: f64,
    }
    impl Message for LookAtGoal {}
    #[derive(Serialize, Deserialize, Clone)]
    struct LookAtResult {
        code: i32,
    }
    impl Message for LookAtResult {}
    #[derive(Serialize, Deserialize)]
    struct LookAtFeedback {
        gaze_error: f32,
    }
    impl Message for LookAtFeedback {}

    type LookAt = ros2_client::Action<LookAtGoal, LookAtResult, LookAtFeedback>;

    /// The result code every run writes.
    const RESULT_CODE: i32 = 7;

    /// The readers of [`SilentReader`], run as a separate process: a plain DDS
    /// participant reading the `look_at` endpoints its environment names,
    /// reliably. It announces itself once each reader holds a sample — proof
    /// that the bridge's writer has matched it — and then idles until the
    /// parent stops it. Without its environment it returns at once.
    #[test]
    #[ignore = "the child process of the unacknowledged_readers tests"]
    fn silent_reader() {
        use ros2_client::action_msgs::GoalStatusArray;
        use ros2_client::rustdds::{
            policy, DomainParticipant, QosPolicies, QosPolicyBuilder, Subscriber, TopicKind,
        };
        use serde::de::DeserializeOwned;

        #[derive(Deserialize)]
        struct FeedbackMessage {
            _goal_id: [u8; 16],
            _gaze_error: f32,
        }
        #[derive(Deserialize)]
        struct GetResultResponse {
            _status: i8,
            _code: i32,
        }

        /// A reliable reader of `topic`, as a check that it took a sample.
        fn reader<D: DeserializeOwned + 'static>(
            participant: &DomainParticipant,
            subscriber: &Subscriber,
            topic: &str,
            type_name: &str,
            durability: policy::Durability,
        ) -> Box<dyn FnMut() -> bool> {
            let qos: QosPolicies = QosPolicyBuilder::new()
                .reliability(policy::Reliability::Reliable {
                    max_blocking_time: ros2_client::rustdds::Duration::from_millis(100),
                })
                .history(policy::History::KeepLast { depth: 4 })
                .durability(durability)
                .build();
            let topic = participant
                .create_topic(
                    topic.to_string(),
                    type_name.to_string(),
                    &qos,
                    TopicKind::NoKey,
                )
                .expect("topic creates");
            let mut reader = subscriber
                .create_datareader_no_key_cdr::<D>(&topic, Some(qos))
                .expect("reader creates");
            Box::new(move || matches!(reader.take_next_sample(), Ok(Some(_))))
        }

        let (Ok(domain_id), Ok(endpoints)) = (
            std::env::var(SILENT_READER_DOMAIN),
            std::env::var(SILENT_READER_ENDPOINTS),
        ) else {
            return;
        };
        let domain_id: u16 = domain_id.parse().expect("a DDS domain id");
        let participant = DomainParticipant::new(domain_id).expect("participant creates");
        let subscriber = participant
            .create_subscriber(&QosPolicies::qos_none())
            .expect("subscriber creates");
        let mut readers: Vec<_> = endpoints
            .split(',')
            .map(|endpoint| match endpoint {
                "feedback" => reader::<FeedbackMessage>(
                    &participant,
                    &subscriber,
                    "rt/robot/actions/look_at/_action/feedback",
                    "arora::action::dds_::look_at_FeedbackMessage_",
                    policy::Durability::Volatile,
                ),
                // The status topic is transient-local: the reader gets the
                // current array as soon as it matches.
                "status" => reader::<GoalStatusArray>(
                    &participant,
                    &subscriber,
                    "rt/robot/actions/look_at/_action/status",
                    "action_msgs::msg::dds_::GoalStatusArray_",
                    policy::Durability::TransientLocal,
                ),
                // A DDS service reply reaches every reader of the reply topic;
                // the requester picks its own out by request id.
                "result" => reader::<GetResultResponse>(
                    &participant,
                    &subscriber,
                    "rr/robot/actions/look_at/_action/get_resultReply",
                    "arora::action::dds_::look_at_GetResult_Response_",
                    policy::Durability::Volatile,
                ),
                other => panic!("no such look_at endpoint: {other}"),
            })
            .collect();
        // The readers stay alive to the end: a dropped reader unmatches.
        let mut matched = vec![false; readers.len()];
        while !matched.iter().all(|matched| *matched) {
            for (took_a_sample, matched) in readers.iter_mut().zip(&mut matched) {
                *matched |= took_a_sample();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("{SILENT_READER_READY}");
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// A [`silent_reader`] process, killed however the test ends (a stopped
    /// process still takes SIGKILL).
    struct SilentReader {
        process: Child,
        /// Resolves once each of its readers holds a sample.
        matched: oneshot::Receiver<()>,
    }

    impl SilentReader {
        fn spawn(domain_id: u16, endpoints: &str) -> Self {
            let mut process = Command::new(std::env::current_exe().expect("the test binary"))
                .args([
                    "unacknowledged_readers::silent_reader",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .env(SILENT_READER_DOMAIN, domain_id.to_string())
                .env(SILENT_READER_ENDPOINTS, endpoints)
                .stdout(Stdio::piped())
                .spawn()
                .expect("the reader process starts");
            let (matched_tx, matched) = oneshot::channel();
            let stdout = process.stdout.take().expect("piped stdout");
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout).lines() {
                    if line.is_ok_and(|line| line.contains(SILENT_READER_READY)) {
                        let _ = matched_tx.send(());
                        return;
                    }
                }
            });
            Self { process, matched }
        }

        /// Send the process `signal` (`STOP`, `CONT`).
        fn signal(&self, signal: &str) {
            let sent = Command::new("kill")
                .args([format!("-{signal}"), self.process.id().to_string()])
                .status()
                .expect("kill runs");
            assert!(sent.success(), "the reader process takes SIG{signal}");
        }
    }

    impl Drop for SilentReader {
        fn drop(&mut self) {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }

    fn status_value(variant: Uuid) -> Value {
        Value::Enumeration(Enumeration {
            id: STATUS_ENUMERATION_ID,
            variant_id: variant,
            value: Box::new(Value::Unit),
        })
    }

    /// `look_at(x) -> Status`: the smallest task-run signature.
    fn look_at_signature() -> MethodSignature {
        let x = gen_uuid_from_str("x");
        let mut parameters = std::collections::HashMap::new();
        parameters.insert(
            x,
            Parameter {
                name: "x".to_string(),
                ty: FrozenTy::from(PrimitiveKind::F64),
                mutable: false,
            },
        );
        MethodSignature {
            module_id: gen_uuid_from_str("gaze-module"),
            id: gen_uuid_from_str("look_at"),
            name: "look_at".to_string(),
            function: Function {
                parameters,
                parameter_ordering: vec![x],
                return_ty: FrozenTy::FrozenScalar(FrozenScalar {
                    reference: FrozenReference {
                        id: STATUS_ENUMERATION_ID,
                        version: Version::parse("1.0.0").unwrap(),
                    },
                }),
            },
        }
    }

    /// The handle of run `n`: its keys live under `arora/tasks/gaze/look_at/run{n}`.
    fn run_handle(n: u128) -> TaskHandle {
        let prefix = format!("arora/tasks/gaze/look_at/run{n}");
        TaskHandle {
            id: TaskId(Uuid::from_u128(n)),
            stop: interpreter_module::encode_halt(TaskId(Uuid::from_u128(n))),
            status: Key::from(format!("{prefix}/status")),
            feedback: vec![Key::from(format!("{prefix}/feedback"))],
            result: vec![Key::from(format!("{prefix}/result"))],
            update: vec![Key::from(format!("{prefix}/update"))],
        }
    }

    /// Answer the runtime commands every scripted device here answers alike:
    /// discovery describes `look_at`, and the device opens no inputs.
    fn answer_discovery(op: &BridgeOp) -> Option<CallResult> {
        let ret = match op {
            BridgeOp::DescribeMethods { .. } => {
                value_serde::to_value(&vec![look_at_signature()]).expect("signatures encode")
            }
            BridgeOp::ListKeys { .. } => {
                let inputs: Vec<(String, KeyMeta)> = Vec::new();
                value_serde::to_value(&inputs).expect("keys encode")
            }
            _ => return None,
        };
        Some(CallResult {
            ret,
            mutated: Vec::new(),
        })
    }

    /// The typed client of the bridge's `look_at` action, on the services
    /// profile every native client uses.
    fn look_at_client(node: &mut ros2_client::Node) -> ros2_client::action::ActionClient<LookAt> {
        let service_qos = {
            use ros2_client::ros2::{policy, QosPolicyBuilder};
            QosPolicyBuilder::new()
                .reliability(policy::Reliability::Reliable {
                    max_blocking_time: ros2_client::ros2::Duration::from_millis(100),
                })
                .history(policy::History::KeepLast { depth: 4 })
                .durability(policy::Durability::Volatile)
                .build()
        };
        let qos = ros2_client::action::ActionClientQosPolicies {
            goal_service: service_qos.clone(),
            result_service: service_qos.clone(),
            cancel_service: service_qos.clone(),
            feedback_subscription: service_qos.clone(),
            status_subscription: service_qos.clone(),
        };
        node.create_action_client::<LookAt>(
            ServiceMapping::Enhanced,
            &Name::parse("/robot/actions/look_at").expect("valid action name"),
            &ros2_client::ActionTypeName::new("arora", "look_at"),
            qos,
        )
        .expect("action client creates")
    }

    /// Send a goal until the bridge accepts it — the first ones may go out
    /// before discovery completes.
    async fn accepted_goal(
        client: &ros2_client::action::ActionClient<LookAt>,
    ) -> ros2_client::action::GoalId {
        loop {
            match tokio::time::timeout(
                Duration::from_secs(2),
                client.async_send_goal(LookAtGoal { x: 1.5 }),
            )
            .await
            {
                Ok(Ok((goal_id, response))) if response.accepted => return goal_id,
                other => {
                    eprintln!("[client] send_goal attempt: {other:?}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    /// A reader of an action's feedback that stops acknowledging does not
    /// hold back the goal's result. The silent reader reads the feedback
    /// topic; once it is stopped, the run keeps writing feedback every 10 ms
    /// for a second, then succeeds. A write that waited on the window — the
    /// 100 ms a reliable write may block — would put the result some ten
    /// seconds behind the run; feedback that does not wait puts it within
    /// moments.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
                  has no unicast-peer/interface config), and the reader is stopped with \
                  SIGSTOP; this runs on Linux CI."
    )]
    async fn a_feedback_reader_that_stops_acknowledging_does_not_hold_back_the_result() {
        /// Feedback ticks after the reader is stopped, 10 ms apart.
        const SILENT_TICKS: u32 = 100;

        let _ = env_logger::builder()
            .parse_filters("warn")
            .is_test(true)
            .try_init();
        let domain_id = random_domain_id();
        let mut bridge = Ros2Bridge::new(Ros2BridgeConfig::new("robot", domain_id)).await;
        let mut inbound = bridge.take_inbound();
        let mut reader = SilentReader::spawn(domain_id, "feedback");

        // The scripted runtime: it answers discovery and SPAWN, then writes the
        // run's feedback every 10 ms — until the reader is stopped, and for
        // SILENT_TICKS ticks after — and finally its result and `Success`,
        // reporting when it did.
        let (stopped_tx, mut stopped_rx) = oneshot::channel::<()>();
        let (finished_tx, finished_rx) = oneshot::channel::<Instant>();
        let runtime = async move {
            let handle = run_handle(0x52);
            let mut running = false;
            let mut tick = 0u32;
            let mut silent_ticks: Option<u32> = None;
            let mut finished_tx = Some(finished_tx);
            loop {
                let event = tokio::select! {
                    event = inbound.next() => event,
                    _ = tokio::time::sleep(Duration::from_millis(10)), if running => {
                        if silent_ticks.is_none() && stopped_rx.try_recv().is_ok() {
                            silent_ticks = Some(0);
                        }
                        if silent_ticks == Some(SILENT_TICKS) {
                            let mut change = StateChange::set(
                                handle.status.clone(),
                                status_value(STATUS_SUCCESS_VARIANT_ID),
                            );
                            change
                                .set
                                .insert(handle.result[0].clone(), Some(Value::I32(RESULT_CODE)));
                            bridge.try_send(&change);
                            running = false;
                            if let Some(tx) = finished_tx.take() {
                                let _ = tx.send(Instant::now());
                            }
                            continue;
                        }
                        tick += 1;
                        silent_ticks = silent_ticks.map(|n| n + 1);
                        let mut change = StateChange::set(
                            handle.status.clone(),
                            status_value(STATUS_RUNNING_VARIANT_ID),
                        );
                        change
                            .set
                            .insert(handle.feedback[0].clone(), Some(Value::F32(tick as f32)));
                        bridge.try_send(&change);
                        continue;
                    }
                };
                let Some(event) = event else { break };
                let Inbound::Command(cmd) = event else {
                    continue;
                };
                if let Some(answer) = answer_discovery(&cmd.op) {
                    cmd.reply(Ok(answer));
                    continue;
                }
                match &cmd.op {
                    BridgeOp::Call(call) if interpreter_module::decode_spawn(call).is_ok() => {
                        running = true;
                        cmd.reply(Ok(CallResult {
                            ret: interpreter_module::encode_spawn_result(&handle),
                            mutated: Vec::new(),
                        }));
                    }
                    other => panic!("unexpected runtime command: {other:?}"),
                }
            }
        };
        tokio::pin!(runtime);

        let client_flow = async {
            let (_ctx, mut node) = create_test_node(domain_id, "silent_reader_client");
            let client = look_at_client(&mut node);
            let goal_id = accepted_goal(&client).await;
            eprintln!("[client] goal accepted");

            (&mut reader.matched)
                .await
                .expect("the reader process reads feedback");
            reader.signal("STOP");
            eprintln!("[client] reader stopped");
            let _ = stopped_tx.send(());

            let (status, result) = client
                .async_request_result(goal_id)
                .await
                .expect("the result arrives");
            let answered = Instant::now();
            let finished = finished_rx.await.expect("the run finished");
            assert_eq!(status, GoalStatusEnum::Succeeded);
            assert_eq!(result.code, RESULT_CODE);
            let behind = answered.saturating_duration_since(finished);
            eprintln!("[client] result {behind:?} after the run finished");
            assert!(
                behind < Duration::from_secs(3),
                "the result arrived {behind:?} after the run finished"
            );
        };

        tokio::select! {
            _ = &mut runtime => panic!("the scripted runtime ended early"),
            result = tokio::time::timeout(Duration::from_secs(60), client_flow) => {
                result.expect("the silent-reader lifecycle timed out");
            }
        }
        drop(reader);
    }

    /// A reader of an action's status and `GetResult` replies that stops
    /// acknowledging holds back no goal, and loses no reply. The silent reader
    /// reads both; once it is stopped, a client sends `GOALS` goals one after
    /// another, each run succeeding three ticks after it starts, and asks for
    /// every result. The status writer's window then holds one array, the
    /// reply writer's four replies.
    ///
    /// - Every goal is accepted at once. A status write that waited on the
    ///   window would hold the action task 100 ms per goal accepted and per
    ///   goal finished, and each acceptance would wait behind them.
    /// - Once the reader resumes, every result arrives, `Succeeded`, and the
    ///   client reads a status array where every goal has succeeded: what the
    ///   windows refused was held, not dropped.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    #[cfg_attr(
        not(target_os = "linux"),
        ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
                  has no unicast-peer/interface config), and the reader is stopped with \
                  SIGSTOP; this runs on Linux CI."
    )]
    async fn a_status_and_result_reader_that_stops_acknowledging_holds_back_no_goal() {
        /// Goals sent while the reader is stopped.
        const GOALS: usize = 16;
        /// Ticks, 10 ms apart, from a run's start to its success.
        const RUN_TICKS: u32 = 3;

        let _ = env_logger::builder()
            .parse_filters("warn")
            .is_test(true)
            .try_init();
        let domain_id = random_domain_id();
        let mut bridge = Ros2Bridge::new(Ros2BridgeConfig::new("robot", domain_id)).await;
        let mut inbound = bridge.take_inbound();
        let mut reader = SilentReader::spawn(domain_id, "status,result");

        // The scripted runtime: every SPAWN starts a run, and each device step
        // (10 ms) writes every run's status, `Success` and its result once
        // the run is RUN_TICKS steps old.
        let runtime = async move {
            let mut runs: Vec<(TaskHandle, u32)> = Vec::new();
            let mut spawned = 0u128;
            loop {
                let event = tokio::select! {
                    event = inbound.next() => event,
                    _ = tokio::time::sleep(Duration::from_millis(10)), if !runs.is_empty() => {
                        let mut change = StateChange::default();
                        for (handle, ticks) in &mut runs {
                            *ticks += 1;
                            let status = if *ticks == RUN_TICKS {
                                change
                                    .set
                                    .insert(handle.result[0].clone(), Some(Value::I32(RESULT_CODE)));
                                STATUS_SUCCESS_VARIANT_ID
                            } else {
                                STATUS_RUNNING_VARIANT_ID
                            };
                            change
                                .set
                                .insert(handle.status.clone(), Some(status_value(status)));
                        }
                        runs.retain(|(_, ticks)| *ticks < RUN_TICKS);
                        bridge.try_send(&change);
                        continue;
                    }
                };
                let Some(event) = event else { break };
                let Inbound::Command(cmd) = event else {
                    continue;
                };
                if let Some(answer) = answer_discovery(&cmd.op) {
                    cmd.reply(Ok(answer));
                    continue;
                }
                match &cmd.op {
                    BridgeOp::Call(call) if interpreter_module::decode_spawn(call).is_ok() => {
                        spawned += 1;
                        let handle = run_handle(spawned);
                        cmd.reply(Ok(CallResult {
                            ret: interpreter_module::encode_spawn_result(&handle),
                            mutated: Vec::new(),
                        }));
                        runs.push((handle, 0));
                    }
                    other => panic!("unexpected runtime command: {other:?}"),
                }
            }
        };
        tokio::pin!(runtime);

        let client_flow = async {
            let (_ctx, mut node) = create_test_node(domain_id, "silent_status_client");
            let client = look_at_client(&mut node);
            // A request written before the server's request reader matched
            // reaches no one.
            client.result_client().wait_for_service(&node).await;

            // Run goals to completion until the reader holds a status array
            // and a reply: then both writers have matched it.
            loop {
                let goal_id = accepted_goal(&client).await;
                let (status, _) = client
                    .async_request_result(goal_id)
                    .await
                    .expect("a result arrives");
                assert_eq!(status, GoalStatusEnum::Succeeded);
                match tokio::time::timeout(Duration::from_millis(200), &mut reader.matched).await {
                    Ok(matched) => break matched.expect("the reader process reads"),
                    Err(_) => eprintln!("[client] the reader has not matched yet"),
                }
            }
            reader.signal("STOP");
            eprintln!("[client] reader stopped");

            let sending = Instant::now();
            let mut goals = Vec::new();
            for _ in 0..GOALS {
                let (goal_id, response) = client
                    .async_send_goal(LookAtGoal { x: 1.5 })
                    .await
                    .expect("the goal is answered");
                assert!(response.accepted, "the goal is accepted");
                let request = client
                    .request_result(goal_id)
                    .expect("the result is requested");
                goals.push((goal_id, request));
            }
            let accepting = sending.elapsed();
            eprintln!("[client] {GOALS} goals accepted in {accepting:?}");
            assert!(
                accepting < Duration::from_secs(1),
                "{GOALS} goals took {accepting:?} to be accepted"
            );

            // Let the runs finish while the reader is stopped, then resume it.
            tokio::time::sleep(Duration::from_millis(500)).await;
            reader.signal("CONT");
            eprintln!("[client] reader resumed");

            let mut unanswered: Vec<_> = goals.iter().map(|(_, request)| *request).collect();
            let answering = async {
                while !unanswered.is_empty() {
                    match client.result_client().receive_response() {
                        Ok(Some((request, response))) => {
                            if unanswered.contains(&request) {
                                assert_eq!(response.status, GoalStatusEnum::Succeeded);
                                assert_eq!(response.result.code, RESULT_CODE);
                                unanswered.retain(|unanswered| *unanswered != request);
                            }
                        }
                        Ok(None) => tokio::time::sleep(Duration::from_millis(10)).await,
                        Err(e) => panic!("reading a result: {e:?}"),
                    }
                }
            };
            tokio::time::timeout(Duration::from_secs(10), answering)
                .await
                .unwrap_or_else(|_| {
                    panic!("{} of {GOALS} results never arrived", unanswered.len())
                });
            eprintln!("[client] every result arrived");

            let succeeded = async {
                loop {
                    match client.receive_status() {
                        Ok(Some(statuses)) => {
                            let all = goals.iter().all(|(goal_id, _)| {
                                statuses.status_list.iter().any(|status| {
                                    status.goal_info.goal_id == *goal_id
                                        && status.status == GoalStatusEnum::Succeeded
                                })
                            });
                            if all {
                                return;
                            }
                        }
                        Ok(None) => tokio::time::sleep(Duration::from_millis(10)).await,
                        Err(e) => panic!("reading a status array: {e:?}"),
                    }
                }
            };
            tokio::time::timeout(Duration::from_secs(10), succeeded)
                .await
                .expect("a status array shows every goal succeeded");
            eprintln!("[client] the status array shows every goal succeeded");
        };

        tokio::select! {
            _ = &mut runtime => panic!("the scripted runtime ended early"),
            result = tokio::time::timeout(Duration::from_secs(90), client_flow) => {
                result.expect("the silent-reader lifecycle timed out");
            }
        }
        drop(reader);
    }
}
