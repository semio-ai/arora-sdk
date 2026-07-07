//! Live-DDS integration tests for the ROS 2 bridge.
//!
//! These create real ROS 2 nodes over DDS and verify end-to-end behaviour:
//! an inbound topic message surfaces as a `BridgeOp::Update` command, and an
//! outbound `send_data` reaches a topic subscriber. Each test uses a random DDS
//! domain to isolate itself.
//!
//! They are ignored on macOS for the same reason as `arora-hal-ros2`'s live
//! tests: DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds
//! 0.11 has no unicast-peer/interface config); they run on Linux CI. To run
//! locally, ensure an active multicast-capable interface and use `--ignored`.

use std::time::Duration;

use arora_bridge::{Bridge, BridgeOp};
use arora_bridge_ros2::conversions::topic_name;
use arora_bridge_ros2::msg_types::{self, MessageType};
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type, Value};
use futures::StreamExt;
use rand::Rng;
use ros2_client::{
    Context, ContextOptions, Name, NodeName, NodeOptions, DEFAULT_PUBLISHER_QOS,
    DEFAULT_SUBSCRIPTION_QOS,
};

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
    let bridge = Ros2Bridge::new(config).await;
    let mut commands = bridge.commands().await;

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
    publisher.wait_for_subscription(&pub_node).await;

    tokio::spawn(async move {
        loop {
            let _ = publisher
                .async_publish(msg_types::Float64 { data: 0.75 })
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    let command = tokio::time::timeout(Duration::from_secs(10), commands.next())
        .await
        .expect("timed out waiting for a command")
        .expect("command stream ended");

    match &command.op {
        BridgeOp::Update(change) => {
            assert_eq!(
                change.set.get("face/mouth/open"),
                Some(&Some(Value::F64(0.75)))
            );
        }
        other => panic!("expected Update, got {other:?}"),
    }
}

/// `send_data` publishes a changed key to its topic, where a separate node
/// subscribed to that topic receives it.
#[tokio::test]
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

    let bridge = Ros2Bridge::new(Ros2BridgeConfig::new(&namespace, domain_id)).await;

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
            bridge
                .send_data(arora_types::data::StateChange::set(
                    "battery/level",
                    Value::F64(0.42),
                ))
                .await
                .expect("send_data");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::pin!(publisher);

    let received = tokio::select! {
        _ = &mut publisher => unreachable!("publisher loop never returns"),
        msg = tokio::time::timeout(Duration::from_secs(10), subscription.async_take()) => msg,
    };

    let (msg, _info) = received
        .expect("timed out waiting for the published value")
        .expect("subscription take failed");
    assert!((msg.data - 0.42).abs() < f64::EPSILON, "got {}", msg.data);
}
