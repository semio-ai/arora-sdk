//! [`Ros2Bridge`]: a ROS 2 graph driven as an Arora
//! [`Bridge`](arora_bridge::Bridge).
//!
//! The bridge exposes a device's keys over ROS 2 topics under a namespace, and
//! treats the ROS graph as the remote control/data plane. Runtime state flows
//! out through [`try_send`](Bridge::try_send), which hands each changed key to
//! the node task to publish to its topic. Incoming messages on the configured
//! input topics become [`BridgeOp::Update`] commands on the endpoint's inbound
//! stream (handed to the runtime once, via
//! [`take_inbound`](Bridge::take_inbound)), which the runtime applies to its
//! store.
//!
//! A background task owns the ROS 2 [`Node`](ros2_client::Node): it spins DDS,
//! drives the input subscriptions, and creates publishers on demand. The bridge
//! communicates with it over channels — the inbound channel's receiver *is*
//! the stream the runtime polls, so there is no intermediate buffer and no
//! lock; the async lives entirely inside that task.

use std::collections::HashMap;

use arora_bridge::{
    Bridge, BridgeCommand, BridgeOp, BridgeResult, DeviceInfo, Inbound, InboundStream,
};
use arora_types::data::StateChange;
use arora_types::value::Type;
use async_trait::async_trait;
use futures::channel::{mpsc as fmpsc, oneshot};
use futures::StreamExt;
use log::warn;
use ros2_client::{Context, ContextOptions, Node, NodeName, NodeOptions};
use tokio::sync::mpsc as tmpsc;
use tokio_util::sync::CancellationToken;

use crate::conversions::{setup_key_subscriber, topic_name, KeyPublisher, StateChangeStream};

/// An input key exposed as an inbound ROS 2 topic: a message received on
/// `/{namespace}/keys/{path}` becomes a [`BridgeOp::Update`] for `path`. The
/// value type selects the `std_msgs` topic type, so it must be declared here
/// (a ROS 2 topic is typed, and the subscription is created before any message
/// arrives).
#[derive(Debug, Clone)]
pub struct InputKey {
    pub path: String,
    pub value_type: Type,
}

impl InputKey {
    pub fn new<S: Into<String>>(path: S, value_type: Type) -> Self {
        Self {
            path: path.into(),
            value_type,
        }
    }
}

/// How to attach to the ROS 2 graph: a `namespace` for the topics, a DDS
/// `domain_id`, and the input keys to subscribe to. Output keys need no
/// declaration — [`send_data`](Bridge::send_data) creates a publisher from each
/// changed value's type on first use.
#[derive(Debug, Clone)]
pub struct Ros2BridgeConfig {
    pub namespace: String,
    pub domain_id: u16,
    pub inputs: Vec<InputKey>,
}

impl Ros2BridgeConfig {
    /// A config with a namespace and domain and no input keys (send-only).
    pub fn new<S: Into<String>>(namespace: S, domain_id: u16) -> Self {
        Self {
            namespace: namespace.into(),
            domain_id,
            inputs: Vec::new(),
        }
    }

    /// Add an input key to subscribe to.
    pub fn with_input<S: Into<String>>(mut self, path: S, value_type: Type) -> Self {
        self.inputs.push(InputKey::new(path, value_type));
        self
    }
}

/// A ROS 2 graph as an Arora [`Bridge`].
pub struct Ros2Bridge {
    namespace: String,
    /// Outbound state changes to publish, sent to the node task.
    outbound: tmpsc::UnboundedSender<StateChange>,
    /// The inbound command receiver, moved out (once) by [`take_inbound`].
    commands: Option<fmpsc::UnboundedReceiver<BridgeCommand>>,
    /// Stops the node task on drop.
    cancel: CancellationToken,
}

impl Ros2Bridge {
    /// Attach to the ROS 2 graph described by `config` and start the node task.
    ///
    /// Must be called from within a Tokio runtime. The node itself is built and
    /// spun in the background; a failure to create it is logged and leaves the
    /// bridge inert (no commands, dropped data) rather than failing here.
    pub async fn new(config: Ros2BridgeConfig) -> Self {
        let (cmd_tx, cmd_rx) = fmpsc::unbounded::<BridgeCommand>();
        let (out_tx, out_rx) = tmpsc::unbounded_channel::<StateChange>();
        let cancel = CancellationToken::new();
        let namespace = config.namespace.clone();

        tokio::spawn(run_node(config, cmd_tx, out_rx, cancel.clone()));

        Self {
            namespace,
            outbound: out_tx,
            commands: Some(cmd_rx),
            cancel,
        }
    }

    /// The topic namespace this bridge exposes the device's keys under.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

impl Drop for Ros2Bridge {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[async_trait]
impl Bridge for Ros2Bridge {
    fn take_inbound(&mut self) -> InboundStream {
        // A ROS 2 graph is a data consumer: the claim opens the stream (DDS
        // does not expose a clean per-subscriber claim/release toggle), then
        // every command the node task enqueues follows, in order.
        let commands = self
            .commands
            .take()
            .expect("Ros2Bridge inbound stream already taken");
        Box::pin(
            futures::stream::once(async { Inbound::DataRequested(true) })
                .chain(commands.map(Inbound::Command)),
        )
    }

    fn try_send(&mut self, change: &StateChange) {
        // Hand the change to the node task, which publishes each changed key to
        // its topic. `unset` keys have no ROS 2 representation and are ignored.
        // A failed send means the node task stopped; drop it (the drop of the
        // bridge cancels the task).
        let _ = self.outbound.send(change.clone());
    }

    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
        // ROS 2 has no device-registration concept.
        Ok(None)
    }

    async fn update_device_info(
        &self,
        info: Option<DeviceInfo>,
    ) -> BridgeResult<Option<DeviceInfo>> {
        Ok(info)
    }
}

/// Build the ROS 2 context and node for the given namespace and domain.
fn build_node(namespace: &str, domain_id: u16) -> Result<Node, String> {
    let ctx = Context::with_options(ContextOptions::new().domain_id(domain_id))
        .map_err(|e| format!("failed to create ROS 2 context: {e:?}"))?;
    let node_name = NodeName::new(&format!("/{namespace}"), "arora_bridge")
        .map_err(|e| format!("invalid node name: {e:?}"))?;
    ctx.new_node(node_name, NodeOptions::new().enable_rosout(true))
        .map_err(|e| format!("failed to create ROS 2 node: {e:?}"))
}

/// The node task: owns the DDS node, drives input subscriptions into
/// [`BridgeCommand`]s, and publishes outbound state changes.
async fn run_node(
    config: Ros2BridgeConfig,
    cmd_tx: fmpsc::UnboundedSender<BridgeCommand>,
    mut outbound_rx: tmpsc::UnboundedReceiver<StateChange>,
    cancel: CancellationToken,
) {
    let Ros2BridgeConfig {
        namespace,
        domain_id,
        inputs,
    } = config;

    let mut node = match build_node(&namespace, domain_id) {
        Ok(node) => node,
        Err(e) => {
            warn!("Ros2Bridge could not start (namespace {namespace}): {e}");
            return;
        }
    };

    // Spin DDS in the background so discovery, subscriptions, and publishers
    // make progress.
    let spinner_task = match node.spinner() {
        Ok(spinner) => Some(tokio::spawn(spinner.spin())),
        Err(e) => {
            warn!("Ros2Bridge could not create a spinner (namespace {namespace}): {e:?}");
            None
        }
    };

    // Subscribe to every declared input key; each yields single-key state
    // changes we turn into `Update` commands.
    let mut sub_streams: Vec<StateChangeStream> = Vec::new();
    for input in &inputs {
        match setup_key_subscriber(&mut node, &namespace, &input.path, &input.value_type) {
            Ok(stream) => sub_streams.push(stream),
            Err(e) => warn!(
                "Ros2Bridge could not subscribe to key '{}': {e}",
                input.path
            ),
        }
    }
    let mut inbound = futures::stream::select_all(sub_streams);

    // Publishers are created lazily from the first value written to each key.
    let mut publishers: HashMap<String, KeyPublisher> = HashMap::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            maybe_change = outbound_rx.recv() => {
                match maybe_change {
                    Some(change) => {
                        publish_change(&mut node, &namespace, &mut publishers, &change).await;
                    }
                    // All senders dropped (the bridge was dropped).
                    None => break,
                }
            }
            Some(change) = inbound.next() => {
                let (reply_tx, _reply_rx) = oneshot::channel();
                if cmd_tx
                    .unbounded_send(BridgeCommand::new(BridgeOp::Update(change), reply_tx))
                    .is_err()
                {
                    // The runtime dropped its command stream.
                    break;
                }
            }
        }
    }

    if let Some(task) = spinner_task {
        task.abort();
    }
}

/// Publish each set key of a change to its topic, creating a publisher on first
/// use. `unset` keys have no ROS 2 representation and are ignored.
async fn publish_change(
    node: &mut Node,
    namespace: &str,
    publishers: &mut HashMap<String, KeyPublisher>,
    change: &StateChange,
) {
    for (key, maybe_value) in &change.set {
        let Some(value) = maybe_value else { continue };
        if !publishers.contains_key(&key.path) {
            let topic = topic_name(namespace, &key.path);
            match KeyPublisher::create(node, &topic, value) {
                Ok(publisher) => {
                    publishers.insert(key.path.clone(), publisher);
                }
                Err(e) => {
                    warn!(
                        "Ros2Bridge could not create a publisher for key '{}': {e}",
                        key.path
                    );
                    continue;
                }
            }
        }
        if let Some(publisher) = publishers.get(&key.path) {
            publisher.publish(value).await;
        }
    }
}
