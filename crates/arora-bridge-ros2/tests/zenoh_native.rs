//! A native ROS 2 subscriber receives what a typed key publisher sends.
//!
//! The subscriber is `ros2 topic echo` — `rclpy` on a real ROS 2 Jazzy stack
//! with `rmw_zenoh_cpp` — because that is the one peer the in-tree tests cannot
//! stand in for: two `ros2-client` endpoints match each other under any type
//! hash, so an all-ours test is blind to a publisher no native node can hear.
//!
//! Needs Docker and an image with ROS 2 Jazzy and `rmw_zenoh`, named by
//! `ROS2_ZENOH_IMAGE`. Any image built like this will do:
//!
//! ```text
//! FROM ros:jazzy-ros-base
//! RUN apt-get update && apt-get install -y --no-install-recommends \
//!       ros-jazzy-rmw-zenoh-cpp
//! ```
//!
//! Run with `--ignored`; without the image it fails loudly rather than passing
//! by skipping.

#![cfg(feature = "zenoh")]

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arora_bridge_ros2::conversions::setup_typed_key_publisher;
use arora_bridge_ros2::{cdr, Qos};
use arora_msgs_ros2::builtin_interfaces::Time;
use arora_msgs_ros2::sensor_msgs::CompressedImage;
use arora_msgs_ros2::std_msgs::Header;
use arora_types::value::Value;
use arora_types::value_serde::bridge::to_value_seeded;
use arora_types::AroraType;
use ros2_client::{
    Context, ContextOptions, MessageTypeName, Name, NodeName, NodeOptions, QosProfile,
};

/// The router's Zenoh port as published on this host; 7447 is left alone so a
/// router someone already runs is neither joined nor collided with.
const HOST_PORT: u16 = 7451;
const ROS_SETUP: &str =
    "source /opt/ros/jazzy/setup.bash && export RMW_IMPLEMENTATION=rmw_zenoh_cpp";

/// A running `rmw_zenohd` in a container, removed when dropped — a panic in the
/// test still tears it down.
struct Router {
    name: String,
}

impl Router {
    fn start(image: &str) -> Router {
        let name = format!("arora-zenoh-native-{}", std::process::id());
        let started = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &name,
                "-p",
                &format!("{HOST_PORT}:7447"),
                image,
            ])
            .args([
                "bash",
                "-lc",
                &format!("{ROS_SETUP} && ros2 run rmw_zenoh_cpp rmw_zenohd"),
            ])
            .output()
            .expect("docker is installed");
        assert!(
            started.status.success(),
            "could not start the router container from {image}: {}",
            String::from_utf8_lossy(&started.stderr)
        );
        let router = Router { name };
        let deadline = Instant::now() + Duration::from_secs(60);
        while !router.logs().contains("Started Zenoh router") {
            assert!(
                Instant::now() < deadline,
                "the router never came up:\n{}",
                router.logs()
            );
            std::thread::sleep(Duration::from_millis(500));
        }
        router
    }

    fn logs(&self) -> String {
        let out = Command::new("docker")
            .args(["logs", &self.name])
            .output()
            .expect("docker logs");
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    /// Run a `ros2` command inside the container, as a native node joined to
    /// its router, returning stdout.
    fn ros2(&self, args: &str, timeout: Duration) -> String {
        let script = format!(
            "{ROS_SETUP} && export ZENOH_CONFIG_OVERRIDE='mode=\"client\";\
             connect/endpoints=[\"tcp/127.0.0.1:7447\"]' && timeout {} ros2 {args}",
            timeout.as_secs()
        );
        let out = Command::new("docker")
            .args(["exec", &self.name, "bash", "-lc", &script])
            .stderr(Stdio::null())
            .output()
            .expect("docker exec");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Router {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .stdout(Stdio::null())
            .status();
    }
}

/// A small `sensor_msgs/CompressedImage` value, as the bridge's registry types it.
fn a_compressed_image() -> Value {
    let (ty, registry) = CompressedImage::arora_type_with_registry();
    let message = CompressedImage {
        header: Header {
            stamp: Time { sec: 1, nanosec: 2 },
            frame_id: "face".into(),
        },
        format: "rgba8; png compressed rgba8".into(),
        data: vec![0x89, b'P', b'N', b'G'],
    };
    to_value_seeded(&message, &ty, &registry).expect("a CompressedImage seeds as its own type")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs docker and a ROS 2 Jazzy image with rmw_zenoh (ROS2_ZENOH_IMAGE); run with \
            --ignored"]
async fn a_native_subscriber_receives_a_typed_key_publisher() {
    let image = std::env::var("ROS2_ZENOH_IMAGE")
        .expect("ROS2_ZENOH_IMAGE names a ROS 2 Jazzy + rmw_zenoh image (see the module docs)");
    let router = Arc::new(Router::start(&image));

    // The device side: a ros2-client node joined to the same router, exactly as
    // the bridge joins one (ADR-0009 reads this variable).
    std::env::set_var(
        "ZENOH_CONFIG_OVERRIDE",
        format!("mode=\"client\";connect/endpoints=[\"tcp/127.0.0.1:{HOST_PORT}\"]"),
    );
    let ctx =
        Context::with_options(ContextOptions::new().domain_id(0)).expect("open a zenoh context");
    let mut node = ctx.new_node(
        NodeName::new("/", "interop_device").unwrap(),
        NodeOptions::new(),
    );
    let registry = Arc::new(arora_msgs_ros2::registry());

    // What the bridge does for a declared typed output: keyed on the real hash.
    let typed = setup_typed_key_publisher(
        &mut node,
        "/interop/face",
        "sensor_msgs/CompressedImage",
        registry.clone(),
        Qos::SensorData,
    )
    .expect("the typed key publisher");

    // The control: the same message on a sibling topic under the placeholder
    // hash — what every typed output announced before the bridge computed one.
    let control_topic = node.create_topic(
        &Name::parse("/interop/placeholder").unwrap(),
        MessageTypeName::new("sensor_msgs", "CompressedImage"),
        &QosProfile::default(),
    );
    let control = node
        .create_raw_publisher(&control_topic, None)
        .expect("the control publisher");

    let value = a_compressed_image();
    let message_type = registry
        .get_by_name("sensor_msgs/CompressedImage")
        .unwrap()
        .clone();
    let control_bytes = cdr::encode(&message_type, registry.types(), &value).expect("encode");

    let stop = Arc::new(AtomicBool::new(false));
    let feed = tokio::spawn({
        let stop = stop.clone();
        async move {
            while !stop.load(Ordering::Relaxed) {
                typed.publish(&value).await;
                let _ = control.async_publish(&control_bytes).await;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    });

    let ros2 = |args: String, secs: u64| {
        let router = router.clone();
        tokio::task::spawn_blocking(move || router.ros2(&args, Duration::from_secs(secs)))
    };

    // Both topics exist on the graph — so a silent control is silent for the
    // hash, not for want of discovery.
    let listed = ros2("topic list".into(), 20).await.unwrap();
    assert!(
        listed.contains("/interop/face"),
        "the typed topic is not on the graph:\n{listed}"
    );
    assert!(
        listed.contains("/interop/placeholder"),
        "the control topic is not on the graph:\n{listed}"
    );

    let echo = |topic: &str, secs: u64| {
        ros2(
            format!("topic echo --once --qos-reliability best_effort --no-arr {topic}"),
            secs,
        )
    };
    let received = echo("/interop/face", 30).await.unwrap();
    let silent = echo("/interop/placeholder", 12).await.unwrap();

    stop.store(true, Ordering::Relaxed);
    let _ = feed.await;

    assert!(
        received.contains("format: rgba8; png compressed rgba8"),
        "rclpy received nothing on the typed topic — the publisher is not keyed on the hash a \
         native subscriber listens for:\n{received}"
    );
    assert!(
        !silent.contains("format:"),
        "the placeholder-hash control was received by a native subscriber, so the hash is not \
         what makes the difference:\n{silent}"
    );
}
