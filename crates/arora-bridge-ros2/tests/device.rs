//! A real Arora device behind the ROS 2 bridge, driven by a ROS 2 peer node:
//! the path from a HAL's key descriptions to ROS 2 topics and back.
//!
//! The device is the `arora` runtime over the fake HAL with one joint,
//! `hip_yaw`. The HAL describes its target in both forms
//! (`hip_yaw/target.position`, `hip_yaw.target_position`: `F64` inputs) and its
//! measured position (`hip_yaw.position`, an `F64` output), and senses a
//! target back as the position. Its store also opens a key-value input
//! (`common.target_trajectory`) and a string input (`text`), the input shapes
//! that have no `std_msgs` number. The bridge asks the device for its keys and
//! subscribes each input on `/{namespace}/keys/{escaped path}`; the peer
//! publishes there and hears the position on its own topic.
//!
//! What each part does alone is tested at its own level: which keys a device
//! lists and which writes it refuses in `arora`, the topic each key form
//! escapes to in `bridge.rs`, and the bridge against a stand-in device in
//! `tests.rs`.
//!
//! The peer joins the bridge's graph the way [`peer`] states for each backend.

mod peer;

use std::time::{Duration, Instant};

use arora::Arora;
use arora_bridge_ros2::conversions::topic_name;
use arora_bridge_ros2::msg_types;
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type, Value};
use arora_hal::FakeHal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, KeyMeta};
use arora_types::keyvalue::KeyValue;
use peer::Peer;
use rand::RngExt;
use tokio::task::JoinHandle;

/// Run the device until `done` holds of it, or `within` elapses; whether it
/// held. The device steps on its own cadence; `done` is asked between runs.
async fn run_until(
    arora: &mut Arora,
    within: Duration,
    mut done: impl FnMut(&Arora) -> bool,
) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        let _ = tokio::time::timeout(
            Duration::from_millis(50),
            arora.run(Arora::DEFAULT_STEP_PERIOD),
        )
        .await;
        if done(arora) {
            return true;
        }
    }
    false
}

fn read(arora: &Arora, key: &str) -> Option<Value> {
    arora.store().read(&[Key::from(key)]).pop().flatten()
}

/// The bridge subscribes each input the device describes, and no output (read
/// from the graph on Zenoh). A target published on its topic comes back as the
/// measured position on the position's topic. A key-value input takes the JSON of its value on a
/// `std_msgs/String` topic, and a string input the string itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    all(feature = "dds", target_os = "macos"),
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); runs on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use `--ignored`."
)]
async fn a_device_s_described_inputs_are_ros_topics_that_move_it() {
    let _ = env_logger::try_init();
    let domain_id = rand::rng().random_range(1..=200);
    let namespace = format!("device_{domain_id}_{}", std::process::id());
    let topic = |path: &str| topic_name(&namespace, path);

    let mut peer = Peer::join(domain_id);

    let hal = FakeHal::new();
    hal.add_joints(["hip_yaw"]);
    let store = SimpleDataStore::new();
    store
        .set_meta(
            [
                (
                    Key::from("common.target_trajectory"),
                    KeyMeta::new().editable().of_type(Type::KeyValue),
                ),
                (
                    Key::from("text"),
                    KeyMeta::new().editable().of_type(Type::String),
                ),
            ]
            .into(),
        )
        .expect("the store holds key meta");
    let bridge = Ros2Bridge::new(Ros2BridgeConfig::new(&namespace, domain_id)).await;
    let mut arora = Arora::builder()
        .with_hal(Box::new(hal))
        .with_data_store(Box::new(store))
        .with_bridge(Box::new(bridge))
        .build()
        .expect("build the device");

    // Only the Zenoh context answers graph queries; the peer has no
    // subscription of its own yet.
    #[cfg(feature = "zenoh")]
    {
        let subscribed = |path: &str| peer.context.subscription_count(&topic(path));
        let inputs = [
            "hip_yaw/target.position",
            "hip_yaw.target_position",
            "common.target_trajectory",
            "text",
        ];
        assert!(
            run_until(&mut arora, Duration::from_secs(30), |_| inputs
                .iter()
                .all(|path| subscribed(path) == 1))
            .await,
            "the bridge does not subscribe each input once: {:?}",
            inputs.map(|path| (path, subscribed(path)))
        );
        assert_eq!(
            subscribed("hip_yaw.position"),
            0,
            "an output has no input topic"
        );
    }

    // The fake senses a target only once it is written, so a position on its
    // topic is a target the peer published. The bridge publishes each change
    // of the position once, best-effort, from a publisher it creates on the
    // first, and what it publishes before the peer has matched that publisher
    // is lost: the feed sends a new target each time until the peer hears one.
    let position = topic("hip_yaw.position");
    let positions = peer.listen::<msg_types::Float64>(&position);
    let targets = peer.feed(&topic("hip_yaw/target.position"), |n| msg_types::Float64 {
        data: f64::from(n + 1),
    });
    let heard = run_until(&mut arora, Duration::from_secs(30), |_| {
        !positions.lock().unwrap().is_empty()
    })
    .await;
    targets.abort();
    assert!(
        heard,
        "the peer hears no position on {position}: store {:?}",
        read(&arora, "hip_yaw.position"),
    );

    let mut points = KeyValue::new();
    points.set_field_value("jointNames", Value::String("hip_yaw".into()));
    let points = points.as_value();
    let json = serde_json::to_string(&points).expect("a value encodes as JSON");
    let feeds = [
        peer.feed(&topic("common.target_trajectory"), move |_| {
            msg_types::String { data: json.clone() }
        }),
        peer.feed(&topic("text"), |_| msg_types::String {
            data: "hello".to_string(),
        }),
    ];
    let landed = run_until(&mut arora, Duration::from_secs(30), |arora| {
        read(arora, "common.target_trajectory").as_ref() == Some(&points)
            && read(arora, "text") == Some(Value::String("hello".into()))
    })
    .await;
    feeds.iter().for_each(JoinHandle::abort);
    assert!(
        landed,
        "the key-value and string inputs do not land: {:?}, {:?}",
        read(&arora, "common.target_trajectory"),
        read(&arora, "text"),
    );
}
