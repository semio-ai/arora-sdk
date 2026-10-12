//! A real Arora device behind the ROS 2 bridge, driven by a ROS 2 peer node.
//!
//! The device is the `arora` runtime over the fake HAL with one joint,
//! `hip_yaw`, which describes its target in both forms (`hip_yaw/target.position`
//! and `hip_yaw.target_position`, `F64` inputs) and its measured position
//! (`hip_yaw.position`, an `F64` output), and senses a target back as the
//! position. Its store also opens a key-value input (`common.target_trajectory`)
//! and a string input (`text`), the other input shapes a HAL describes. The
//! bridge asks the device for its keys and exposes each input on
//! `/{namespace}/keys/{escaped path}`; the peer publishes on those topics and
//! reads the position back from its output topic.
//!
//! Each backend joins the peer its own way. On `zenoh` the two sessions connect
//! over loopback TCP, with no router and no multicast, so the test runs
//! everywhere. On `dds` the peers discover each other over multicast SPDP, which
//! is unreliable on macOS loopback: there it is ignored like the other live DDS
//! tests, and runs on Linux.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arora::Arora;
use arora_bridge_ros2::conversions::topic_name;
use arora_bridge_ros2::msg_types::{self, MessageType};
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type, Value};
use arora_hal::FakeHal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, KeyMeta};
use arora_types::keyvalue::KeyValue;
use rand::RngExt;
use ros2_client::{Context, ContextOptions, Name, NodeName, NodeOptions};
use tokio::task::JoinHandle;

/// The peer: a ROS 2 node on the bridge's graph, and the context that holds
/// its session. Only the Zenoh backend's context answers graph queries
/// (`subscription_count`); on DDS it is held for the node's life.
struct Peer {
    #[cfg_attr(feature = "dds", allow(dead_code))]
    context: Context,
    node: ros2_client::Node,
}

impl Peer {
    /// On `zenoh`, the bridge's session is opened from the environment
    /// (`ZENOH_CONFIG_OVERRIDE`, as `rmw_zenoh` reads it) when the bridge
    /// starts. The peer listens on a free loopback port and the environment
    /// then points every later session — the bridge's — at it.
    #[cfg(feature = "zenoh")]
    fn join(domain_id: u16) -> Peer {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("a free loopback port")
            .port();
        std::env::set_var(
            "ZENOH_CONFIG_OVERRIDE",
            format!("listen/endpoints=[\"tcp/127.0.0.1:{port}\"]"),
        );
        let context = Context::with_options(ContextOptions::new().domain_id(domain_id))
            .expect("open the peer's zenoh session");
        std::env::set_var(
            "ZENOH_CONFIG_OVERRIDE",
            format!("connect/endpoints=[\"tcp/127.0.0.1:{port}\"]"),
        );
        let node = context.new_node(
            NodeName::new("/", &format!("peer_{domain_id}")).expect("valid node name"),
            NodeOptions::new(),
        );
        Peer { context, node }
    }

    #[cfg(feature = "dds")]
    fn join(domain_id: u16) -> Peer {
        let context = Context::with_options(ContextOptions::new().domain_id(domain_id))
            .expect("open the peer's DDS participant");
        let mut node = context
            .new_node(
                NodeName::new("/", &format!("peer_{domain_id}")).expect("valid node name"),
                NodeOptions::new(),
            )
            .expect("create the peer node");
        tokio::spawn(node.spinner().expect("the peer's spinner").spin());
        Peer { context, node }
    }

    /// The topic `name` of `M`. On DDS it takes the QoS a writer or a reader
    /// has by default — reliable writers, best-effort readers — which match
    /// the bridge's reliable inputs and best-effort outputs. Zenoh delivers
    /// across reliabilities, so its default profile serves both.
    #[cfg(feature = "zenoh")]
    fn topic<M: MessageType>(&mut self, name: &str, _reader: bool) -> ros2_client::Topic {
        self.node.create_topic(
            &Name::parse(name).expect("valid topic name"),
            M::message_type_name(),
            &ros2_client::QosProfile::default(),
        )
    }

    #[cfg(feature = "dds")]
    fn topic<M: MessageType>(&mut self, name: &str, reader: bool) -> ros2_client::rustdds::Topic {
        let qos: &ros2_client::ros2::QosPolicies = if reader {
            &ros2_client::DEFAULT_SUBSCRIPTION_QOS
        } else {
            &ros2_client::DEFAULT_PUBLISHER_QOS
        };
        self.node
            .create_topic(
                &Name::parse(name).expect("valid topic name"),
                M::message_type_name(),
                qos,
            )
            .expect("create the topic")
    }

    /// Publish `message` on `topic` every 100 ms until the returned task is
    /// aborted: a subscriber that joins late still gets it.
    fn feed<M: MessageType>(&mut self, topic: &str, message: M) -> JoinHandle<()> {
        self.feed_with(topic, move |_| message.clone())
    }

    /// Publish the `n`th message `message(n)` on `topic` every 100 ms, from
    /// 0, until the returned task is aborted.
    fn feed_with<M: MessageType>(
        &mut self,
        topic: &str,
        mut message: impl FnMut(u32) -> M + Send + 'static,
    ) -> JoinHandle<()> {
        let ros_topic = self.topic::<M>(topic, false);
        let publisher = self
            .node
            .create_publisher::<M>(&ros_topic, None)
            .expect("create the publisher");
        tokio::spawn(async move {
            for n in 0.. {
                let _ = publisher.async_publish(message(n)).await;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    }

    /// Every message received on `topic`, newest last.
    fn listen<M: MessageType>(&mut self, topic: &str) -> Arc<Mutex<Vec<M>>> {
        let ros_topic = self.topic::<M>(topic, true);
        let subscription = self
            .node
            .create_subscription::<M>(&ros_topic, None)
            .expect("create the subscription");
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        tokio::spawn(async move {
            while let Ok((message, _info)) = subscription.async_take().await {
                sink.lock().unwrap().push(message);
            }
        });
        received
    }
}

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

/// A device's described inputs get topics on the bridge, a publish on each
/// target form's topic moves the measured key, and the measured key comes back
/// on its own topic. A message of another type on a target's topic does not
/// land and leaves the target subscribed. A key-value input takes the JSON of its value on a
/// `std_msgs/String` topic, and the device refuses the JSON of a value of
/// another type; a string input takes the string itself.
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

    let entity_form = topic("hip_yaw/target.position");
    let prefixed_form = topic("hip_yaw.target_position");
    let position = topic("hip_yaw.position");
    let trajectory = topic("common.target_trajectory");
    let text = topic("text");
    assert_eq!(
        entity_form,
        format!("/{namespace}/keys/hip_yaw/target_position")
    );
    assert_eq!(
        prefixed_form,
        format!("/{namespace}/keys/hip_yaw_target_position")
    );
    assert_eq!(position, format!("/{namespace}/keys/hip_yaw_position"));
    assert_eq!(
        trajectory,
        format!("/{namespace}/keys/common_target_trajectory")
    );
    assert_eq!(text, format!("/{namespace}/keys/text"));

    // The bridge subscribes the device's inputs, and only them; the peer has
    // no subscription of its own yet. (Only the Zenoh context answers graph
    // queries.)
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
            "the bridge does not subscribe every input: {:?}",
            inputs.map(|path| (path, subscribed(path)))
        );
        assert_eq!(
            subscribed("hip_yaw.position"),
            0,
            "an output has no input topic"
        );
    }

    let positions = peer.listen::<msg_types::Float64>(&position);
    let sensed = |arora: &Arora, value: f64| {
        read(arora, "hip_yaw.position") == Some(Value::F64(value))
            && positions.lock().unwrap().last().map(|m| m.data) == Some(value)
    };

    // The bridge publishes each change of the position once, best-effort,
    // from a publisher it creates on the first: what it publishes before the
    // peer has matched that publisher is lost. So the position first changes
    // until the peer hears it, then settles.
    let warm_up = peer.feed_with(&entity_form, |n| msg_types::Float64 {
        data: f64::from(n + 1) * 0.001,
    });
    assert!(
        run_until(&mut arora, Duration::from_secs(30), |_| !positions
            .lock()
            .unwrap()
            .is_empty())
        .await,
        "the peer hears nothing on {position}: store {:?}",
        read(&arora, "hip_yaw.position"),
    );
    warm_up.abort();
    run_until(&mut arora, Duration::from_millis(500), |_| false).await;

    // The entity form.
    let feed = peer.feed(&entity_form, msg_types::Float64 { data: 0.5 });
    assert!(
        run_until(&mut arora, Duration::from_secs(30), |arora| sensed(
            arora, 0.5
        ))
        .await,
        "a publish on {entity_form} does not move hip_yaw.position: store {:?}, topic {:?}",
        read(&arora, "hip_yaw.position"),
        positions.lock().unwrap().last().map(|m| m.data),
    );
    feed.abort();

    // A message of another type on a target's topic neither lands nor ends
    // the target's subscription: the `Float64` published next still lands. On
    // Zenoh the type name is part of the key expression, so the message
    // reaches no subscription of the target. On DDS it reaches the bridge's
    // reader, which matches on the topic name alone, and is dropped as
    // undecodable; the feed sends far more messages than the five receive
    // failures in a row that end an input.
    let feed = peer.feed(&prefixed_form, msg_types::Int32 { data: 7 });
    run_until(&mut arora, Duration::from_secs(3), |_| false).await;
    feed.abort();
    assert_eq!(read(&arora, "hip_yaw.target_position"), None);
    assert_eq!(read(&arora, "hip_yaw.position"), Some(Value::F64(0.5)));

    // The prefixed form.
    let feed = peer.feed(&prefixed_form, msg_types::Float64 { data: 0.25 });
    assert!(
        run_until(&mut arora, Duration::from_secs(30), |arora| sensed(
            arora, 0.25
        ))
        .await,
        "a publish on {prefixed_form} does not move hip_yaw.position: store {:?}, topic {:?}",
        read(&arora, "hip_yaw.position"),
        positions.lock().unwrap().last().map(|m| m.data),
    );
    feed.abort();

    // The key-value and string inputs. The trajectory's publisher alternates
    // the JSON of a key-value with the JSON of a number: the device refuses
    // the number, so once the key-value has landed the key never holds the
    // number.
    let mut points = KeyValue::new();
    points.set_field_value("jointNames", Value::String("hip_yaw".into()));
    let points = points.as_value();
    let json = |value: &Value| serde_json::to_string(value).expect("a value encodes as JSON");
    let (good, bad) = (json(&points), json(&Value::F64(1.5)));
    let feeds = [
        peer.feed_with(&trajectory, move |n| msg_types::String {
            data: if n % 2 == 0 {
                good.clone()
            } else {
                bad.clone()
            },
        }),
        peer.feed(
            &text,
            msg_types::String {
                data: "hello".to_string(),
            },
        ),
    ];
    assert!(
        run_until(&mut arora, Duration::from_secs(30), |arora| {
            read(arora, "common.target_trajectory").as_ref() == Some(&points)
                && read(arora, "text") == Some(Value::String("hello".into()))
        })
        .await,
        "the key-value and string inputs do not land: {:?}, {:?}",
        read(&arora, "common.target_trajectory"),
        read(&arora, "text"),
    );
    let mut held = Vec::new();
    run_until(&mut arora, Duration::from_millis(1500), |arora| {
        held.push(read(arora, "common.target_trajectory"));
        false
    })
    .await;
    feeds.iter().for_each(JoinHandle::abort);
    assert!(
        held.iter().all(|value| value.as_ref() == Some(&points)),
        "the key-value input took another value: {held:?}"
    );
}
