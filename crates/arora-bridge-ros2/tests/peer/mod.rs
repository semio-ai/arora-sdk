//! A ROS 2 peer node on the bridge's graph, for the live tests that drive the
//! bridge from outside.
//!
//! Each backend joins the peer its own way. On `zenoh` the two sessions connect
//! over loopback TCP, with no router and no multicast, so a test runs
//! everywhere. On `dds` the peers discover each other over multicast SPDP, which
//! is unreliable on macOS loopback: there a test is ignored like the other live
//! DDS tests, and runs on Linux.

// Each test binary that includes this module uses its own part of the peer.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arora_bridge_ros2::msg_types::MessageType;
use ros2_client::{Context, ContextOptions, Name, NodeName, NodeOptions};
use tokio::task::JoinHandle;

/// The peer: a ROS 2 node on the bridge's graph, and the context that holds
/// its session. Only the Zenoh backend's context answers graph queries
/// (`subscription_count`); on DDS it is held for the node's life.
pub struct Peer {
    pub context: Context,
    node: ros2_client::Node,
}

impl Peer {
    /// On `zenoh`, the bridge's session is opened from the environment
    /// (`ZENOH_CONFIG_OVERRIDE`, as `rmw_zenoh` reads it) when the bridge
    /// starts. The peer listens on a free loopback port and the environment
    /// then points every later session — the bridge's — at it.
    #[cfg(feature = "zenoh")]
    pub fn join(domain_id: u16) -> Peer {
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
    pub fn join(domain_id: u16) -> Peer {
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

    /// Publish the `n`th message `message(n)` on `topic` every 100 ms, from
    /// 0, until the returned task is aborted: a subscriber that joins late
    /// still gets one.
    pub fn feed<M: MessageType>(
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
    pub fn listen<M: MessageType>(&mut self, topic: &str) -> Arc<Mutex<Vec<M>>> {
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
