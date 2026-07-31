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
use std::pin::Pin;
use std::sync::Arc;

use arora_bridge::{
    Bridge, BridgeCommand, BridgeOp, BridgeResult, DeviceInfo, Inbound, InboundStream,
    MethodSignature,
};
use arora_msgs_ros2::Ros2Registry;
use arora_types::data::StateChange;
use arora_types::value::Type;
use async_trait::async_trait;
use futures::channel::{mpsc as fmpsc, oneshot};
use futures::{Stream, StreamExt};
use log::warn;
use ros2_client::{Context, ContextOptions, Node, NodeName, NodeOptions};
use tokio::sync::mpsc as tmpsc;
use tokio_util::sync::CancellationToken;

use crate::actions;
use crate::conversions::{
    setup_key_subscriber, setup_typed_key_publisher, setup_typed_key_subscriber, topic_name,
    KeyPublisher, StateChangeStream, TypedKeyPublisher,
};
use crate::profile;
use crate::services;

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

/// An output key published as a **typed** ROS 2 message rather than a
/// `std_msgs` scalar: the key's value is encoded as `ros_type` (a registered
/// ROS message name, e.g. `hri_msgs/Expression`) and published on `topic`.
///
/// A key without such a declaration still publishes on the untyped path (a
/// `std_msgs` scalar, or a JSON `std_msgs/String` for composites) — this is the
/// opt-in that lets a device key ride a ROS4HRI message.
#[derive(Debug, Clone)]
pub struct TypedOutput {
    pub path: String,
    /// The registered ROS message name, e.g. `hri_msgs/Expression`.
    pub ros_type: String,
    /// The topic name. `None` uses the `/{namespace}/keys/{path}` convention;
    /// `Some` is used verbatim, so an absolute ROS name (e.g.
    /// `/robot_face/expression`) escapes the namespace prefix.
    pub topic: Option<String>,
}

/// An input key subscribed as a **typed** ROS 2 message rather than a
/// `std_msgs` scalar: each message on `topic` is decoded as `ros_type` (a
/// registered ROS message name, e.g. `hri_msgs/Expression`) against its runtime
/// type and lands on `path` as a [`BridgeOp::Update`]. The counterpart of
/// [`TypedOutput`] — how a device key receives a real ROS4HRI message.
#[derive(Debug, Clone)]
pub struct TypedInput {
    pub path: String,
    /// The registered ROS message name, e.g. `hri_msgs/Expression`.
    pub ros_type: String,
    /// The topic name. `None` uses the `/{namespace}/keys/{path}` convention;
    /// `Some` is used verbatim (an absolute ROS name escapes the namespace).
    pub topic: Option<String>,
    /// Field fan-out over device keys (see [`profile::FieldRoute`]). Empty
    /// lands the whole decoded message on `path`.
    pub routes: Vec<profile::FieldRoute>,
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
    /// Input keys subscribed as typed ROS messages (see [`TypedInput`]); a key
    /// not listed here subscribes on the untyped `std_msgs` path.
    pub typed_inputs: Vec<TypedInput>,
    /// Output keys published as typed ROS messages (see [`TypedOutput`]); a key
    /// not listed here publishes on the untyped `std_msgs` path.
    pub outputs: Vec<TypedOutput>,
    /// Bulk-key exposure on the scalar plane: keys matching an include's glob
    /// publish under its rewritten absolute topic instead of the
    /// `/{namespace}/keys/{path}` convention (see [`profile::Include`]).
    pub includes: Vec<profile::Include>,
}

impl Ros2BridgeConfig {
    /// A config with a namespace and domain and no input keys (send-only).
    pub fn new<S: Into<String>>(namespace: S, domain_id: u16) -> Self {
        Self {
            namespace: namespace.into(),
            domain_id,
            inputs: Vec::new(),
            typed_inputs: Vec::new(),
            includes: Vec::new(),
            outputs: Vec::new(),
        }
    }

    /// Add an input key to subscribe to.
    pub fn with_input<S: Into<String>>(mut self, path: S, value_type: Type) -> Self {
        self.inputs.push(InputKey::new(path, value_type));
        self
    }

    /// Subscribe an input key as a typed ROS message `ros_type` (a registered
    /// ROS message name, e.g. `hri_msgs/Expression`) on the default
    /// `/{namespace}/keys/{path}` topic. Each received message is decoded
    /// against that type and lands on `path`.
    pub fn with_typed_input<P: Into<String>, T: Into<String>>(
        mut self,
        path: P,
        ros_type: T,
    ) -> Self {
        self.typed_inputs.push(TypedInput {
            routes: Vec::new(),
            path: path.into(),
            ros_type: ros_type.into(),
            topic: None,
        });
        self
    }

    /// Subscribe an input key as a typed ROS message on an explicit `topic` name
    /// — an absolute ROS name (e.g. `/robot_face/expression`) escapes the
    /// `/{namespace}/keys/…` convention, as a ROS4HRI binding needs.
    pub fn with_typed_input_on<P: Into<String>, T: Into<String>, N: Into<String>>(
        mut self,
        path: P,
        ros_type: T,
        topic: N,
    ) -> Self {
        self.typed_inputs.push(TypedInput {
            routes: Vec::new(),
            path: path.into(),
            ros_type: ros_type.into(),
            topic: Some(topic.into()),
        });
        self
    }

    /// Publish an output key as a typed ROS message `ros_type` (a registered ROS
    /// message name, e.g. `hri_msgs/Expression`) on the default
    /// `/{namespace}/keys/{path}` topic. The key's value must be a structure
    /// matching that message's type.
    pub fn with_typed_output<P: Into<String>, T: Into<String>>(
        mut self,
        path: P,
        ros_type: T,
    ) -> Self {
        self.outputs.push(TypedOutput {
            path: path.into(),
            ros_type: ros_type.into(),
            topic: None,
        });
        self
    }

    /// Publish an output key as a typed ROS message on an explicit `topic` name
    /// — an absolute ROS name (e.g. `/robot_face/expression`) escapes the
    /// `/{namespace}/keys/…` convention, as a ROS4HRI binding needs.
    pub fn with_typed_output_on<P: Into<String>, T: Into<String>, N: Into<String>>(
        mut self,
        path: P,
        ros_type: T,
        topic: N,
    ) -> Self {
        self.outputs.push(TypedOutput {
            path: path.into(),
            ros_type: ros_type.into(),
            topic: Some(topic.into()),
        });
        self
    }

    /// Expose the device through a named [`profile::ExposureProfile`]: each
    /// endpoint becomes a typed binding on its absolute topic with its field
    /// fan-out, and the profile's includes join the scalar plane's rewrite
    /// set. Outbound endpoints route the whole message from their first
    /// route's key (field fan-in is not implemented).
    pub fn with_profile(mut self, profile: profile::ExposureProfile) -> Self {
        for endpoint in profile.endpoints {
            match endpoint.flow {
                profile::Flow::In => self.typed_inputs.push(TypedInput {
                    // The path names the binding in logs; routed fields land
                    // on their own keys.
                    path: endpoint.topic.clone(),
                    ros_type: endpoint.ros_type,
                    topic: Some(endpoint.topic),
                    routes: endpoint.routes,
                }),
                profile::Flow::Out => {
                    let Some(route) = endpoint.routes.first() else {
                        continue;
                    };
                    self.outputs.push(TypedOutput {
                        path: route.key.clone(),
                        ros_type: endpoint.ros_type,
                        topic: Some(endpoint.topic),
                    });
                }
            }
        }
        self.includes.extend(profile.includes);
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
    #[cfg(feature = "dds")]
    {
        ctx.new_node(node_name, NodeOptions::new().enable_rosout(true))
            .map_err(|e| format!("failed to create ROS 2 node: {e:?}"))
    }
    // The Zenoh backend's `new_node` is infallible (returns `Node`, not a Result),
    // and its `NodeOptions` has no rosout toggle.
    #[cfg(feature = "zenoh")]
    {
        Ok(ctx.new_node(node_name, NodeOptions::new()))
    }
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
        typed_inputs,
        outputs,
        includes,
    } = config;

    let mut node = match build_node(&namespace, domain_id) {
        Ok(node) => node,
        Err(e) => {
            warn!("Ros2Bridge could not start (namespace {namespace}): {e}");
            return;
        }
    };

    // The DDS backend needs a background spinner so discovery, subscriptions,
    // and publishers make progress; the Zenoh backend drives them on its own
    // async session, so there is nothing to spin.
    #[cfg(feature = "dds")]
    let spinner_task = match node.spinner() {
        Ok(spinner) => Some(tokio::spawn(spinner.spin())),
        Err(e) => {
            warn!("Ros2Bridge could not create a spinner (namespace {namespace}): {e:?}");
            None
        }
    };
    #[cfg(feature = "zenoh")]
    let spinner_task: Option<tokio::task::JoinHandle<()>> = None;

    // The registry of ROS message types, shared by the typed topic, service,
    // and action planes.
    let registry = Arc::new(arora_msgs_ros2::registry());

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
    // Typed input keys subscribe as a registered ROS message, decoded against
    // its runtime type into a single-key change — how a device key receives a
    // real ROS4HRI message.
    for input in &typed_inputs {
        let topic = input
            .topic
            .clone()
            .unwrap_or_else(|| topic_name(&namespace, &input.path));
        match setup_typed_key_subscriber(
            &mut node,
            &topic,
            &input.ros_type,
            input.path.clone(),
            input.routes.clone(),
            registry.clone(),
        ) {
            Ok(stream) => sub_streams.push(stream),
            Err(e) => warn!(
                "Ros2Bridge could not subscribe to typed key '{}': {e}",
                input.path
            ),
        }
    }
    let mut inbound = futures::stream::select_all(sub_streams);

    // Expose every ROS-representable module method as a service under
    // `/{namespace}/methods/{name}` — and every *task-run* method (one
    // returning the behavior-tree `Status`) as an action under
    // `/{namespace}/actions/{name}`. Discovered from the runtime — like
    // outbound topics, a device's methods are its own surface, so nothing is
    // declared.
    let (discovered, discovered_actions) = discover(&cmd_tx, &namespace, &registry).await;
    let mut service_streams: Vec<Pin<Box<dyn Stream<Item = ()> + Send>>> = Vec::new();
    for service in discovered {
        if let Some(stream) = service_stream(&mut node, service, registry.clone(), cmd_tx.clone()) {
            service_streams.push(stream);
        }
    }
    let mut service_requests = futures::stream::select_all(service_streams);

    // One task per action, each owning its endpoints and goal book; the node
    // task forwards every outbound state change so the tasks can watch their
    // runs' status/feedback/result keys.
    let mut action_change_txs: Vec<tmpsc::UnboundedSender<StateChange>> = Vec::new();
    for action in discovered_actions {
        match create_raw_action_server(&mut node, &action) {
            Ok(server) => {
                let (change_tx, change_rx) = tmpsc::unbounded_channel();
                action_change_txs.push(change_tx);
                tokio::spawn(actions::action_task(
                    server,
                    action,
                    registry.clone(),
                    cmd_tx.clone(),
                    change_rx,
                ));
            }
            Err(e) => warn!("Ros2Bridge could not create action '{}': {e}", action.name),
        }
    }

    // Publishers are created lazily from the first value written to each key.
    let mut publishers: HashMap<String, KeyPublisher> = HashMap::new();
    // Output keys the caller declared as typed ROS messages, indexed by path,
    // with their own lazily-created raw publishers.
    let typed_outputs: HashMap<String, TypedOutput> =
        outputs.into_iter().map(|o| (o.path.clone(), o)).collect();
    let mut typed_publishers: HashMap<String, TypedKeyPublisher> = HashMap::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            maybe_change = outbound_rx.recv() => {
                match maybe_change {
                    Some(change) => {
                        // The action tasks watch their runs' keys on the same
                        // outbound stream the topics mirror.
                        for tx in &action_change_txs {
                            let _ = tx.send(change.clone());
                        }
                        publish_change(
                            &mut node,
                            &namespace,
                            &mut publishers,
                            &typed_outputs,
                            &mut typed_publishers,
                            &includes,
                            &registry,
                            &change,
                        )
                        .await;
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
            // A method service received a request; it is decoded, dispatched as a
            // `Call`, and answered inside its own stream — nothing to do here but
            // keep driving the services.
            Some(()) = service_requests.next() => {}
        }
    }

    if let Some(task) = spinner_task {
        task.abort();
    }
}

/// Ask the runtime for every callable method's signature and resolve both
/// planes: the ROS 2-representable methods to services, and the task-run
/// (behavior-tree-`Status`-returning) methods to actions. Methods whose types
/// aren't representable on their plane are logged and skipped (never silently
/// dropped). Empty if the runtime never answers (e.g. it stopped) — the bridge
/// then serves no methods.
async fn discover(
    cmd_tx: &fmpsc::UnboundedSender<BridgeCommand>,
    namespace: &str,
    registry: &Ros2Registry,
) -> (Vec<services::MethodService>, Vec<actions::MethodAction>) {
    let (reply_tx, reply_rx) = oneshot::channel();
    if cmd_tx
        .unbounded_send(BridgeCommand::new(
            BridgeOp::DescribeMethods { prefix: None },
            reply_tx,
        ))
        .is_err()
    {
        return (Vec::new(), Vec::new());
    }
    let signatures: Vec<MethodSignature> = match reply_rx.await {
        Ok(Ok(result)) => arora_types::value_serde::from_value(result.ret).unwrap_or_else(|e| {
            warn!("Ros2Bridge could not decode method signatures: {e}");
            Vec::new()
        }),
        Ok(Err(e)) => {
            warn!("Ros2Bridge describe-methods failed: {e}");
            Vec::new()
        }
        Err(_) => Vec::new(),
    };
    let (resolved, skipped) = services::resolve(namespace, &signatures, registry);
    if !skipped.is_empty() {
        warn!(
            "Ros2Bridge skips methods whose types are not ROS 2-representable: {}",
            skipped.join(", ")
        );
    }
    let (resolved_actions, skipped_actions) = actions::resolve(namespace, &signatures, registry);
    if !skipped_actions.is_empty() {
        warn!(
            "Ros2Bridge skips task-run methods whose goal types are not ROS 2-representable: {}",
            skipped_actions.join(", ")
        );
    }
    (resolved, resolved_actions)
}

/// The QoS every bridge-served ROS 2 service endpoint uses: reliable,
/// transient-local, a short history — the profile ros2-client's own service and
/// action examples run (`DEFAULT_SUBSCRIPTION_QOS` is best-effort, which drops
/// service requests with no redelivery).
#[cfg(feature = "dds")]
fn service_qos() -> ros2_client::ros2::QosPolicies {
    use ros2_client::ros2::{policy, QosPolicyBuilder};
    QosPolicyBuilder::new()
        .reliability(policy::Reliability::Reliable {
            max_blocking_time: ros2_client::ros2::Duration::from_millis(100),
        })
        .history(policy::History::KeepLast { depth: 4 })
        .durability(policy::Durability::TransientLocal)
        .build()
}

/// Create the five ROS 2 endpoints for one action, per backend. The status
/// topic is transient-local with a history of one (late-joining clients read
/// the current goal states, per the ROS actions design); the services and the
/// feedback topic ride the reliable [`service_qos`].
fn create_raw_action_server(
    node: &mut Node,
    action: &actions::MethodAction,
) -> Result<ros2_client::RawActionServer, String> {
    let name = services::parse_name(&action.name)?;
    #[cfg(feature = "dds")]
    {
        use ros2_client::ros2::{policy, QosPolicyBuilder};
        let status_qos = QosPolicyBuilder::new()
            .durability(policy::Durability::TransientLocal)
            .history(policy::History::KeepLast { depth: 1 })
            .reliability(policy::Reliability::Reliable {
                max_blocking_time: ros2_client::ros2::Duration::from_millis(100),
            })
            .build();
        let qos = ros2_client::action::ActionServerQosPolicies {
            goal_service: service_qos(),
            result_service: service_qos(),
            cancel_service: service_qos(),
            feedback_publisher: service_qos(),
            status_publisher: status_qos,
        };
        node.create_raw_action_server(&name, &action.action_type, qos)
            .map_err(|e| format!("{e:?}"))
    }
    #[cfg(feature = "zenoh")]
    {
        node.create_raw_action_server(&name, &action.action_type)
            .map_err(|e| format!("{e:?}"))
    }
}

/// Create the ROS 2 service for one method and return a stream that serves its
/// requests. Each request is decoded, dispatched as a [`BridgeOp::Call`], and
/// answered **inside** the stream, so the caller only drives it. `None` if the
/// service could not be created.
fn service_stream(
    node: &mut Node,
    service: services::MethodService,
    registry: Arc<Ros2Registry>,
    cmd_tx: fmpsc::UnboundedSender<BridgeCommand>,
) -> Option<Pin<Box<dyn Stream<Item = ()> + Send>>> {
    let ros_name = match services::parse_name(&service.name) {
        Ok(name) => name,
        Err(e) => {
            warn!("Ros2Bridge {e}");
            return None;
        }
    };
    #[cfg(feature = "dds")]
    let server = node.create_raw_server(
        &ros_name,
        &service.service_type,
        service_qos(),
        service_qos(),
    );
    #[cfg(feature = "zenoh")]
    let server = node.create_raw_server(&ros_name, &service.service_type);
    let server = match server {
        Ok(server) => server,
        Err(e) => {
            warn!(
                "Ros2Bridge could not create service '{}': {e:?}",
                service.name
            );
            return None;
        }
    };
    let stream = futures::stream::unfold(
        (server, service, registry, cmd_tx),
        |(server, service, registry, cmd_tx)| async move {
            match server.async_receive_request().await {
                Ok((request_id, request)) => {
                    if let Some(response) =
                        build_response(&service, &registry, &cmd_tx, &request).await
                    {
                        let _ = server.send_response(request_id, &response);
                    }
                    Some(((), (server, service, registry, cmd_tx)))
                }
                Err(e) => {
                    warn!(
                        "Ros2Bridge service '{}' stopped receiving: {e:?}",
                        service.name
                    );
                    None
                }
            }
        },
    );
    Some(Box::pin(stream))
}

/// Turn one raw request into its raw response: decode to a value, dispatch it as
/// a [`BridgeOp::Call`] to the runtime, and encode the returned value. `None`
/// (no response sent) if any step fails — a decode error, a call error, or the
/// runtime dropping the reply.
async fn build_response(
    service: &services::MethodService,
    registry: &Ros2Registry,
    cmd_tx: &fmpsc::UnboundedSender<BridgeCommand>,
    request: &[u8],
) -> Option<Vec<u8>> {
    let value = match services::decode_request(service, request, registry) {
        Ok(value) => value,
        Err(e) => {
            warn!("Ros2Bridge {e}");
            return None;
        }
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    if cmd_tx
        .unbounded_send(BridgeCommand::new(
            BridgeOp::Call(services::call_of(service, value)),
            reply_tx,
        ))
        .is_err()
    {
        return None;
    }
    let result = match reply_rx.await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            warn!("Ros2Bridge method '{}' call failed: {e}", service.name);
            return None;
        }
        Err(_) => return None,
    };
    let response = services::response_value(service, result);
    match services::encode_response(service, &response, registry) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            warn!("Ros2Bridge {e}");
            None
        }
    }
}

/// Publish each set key of a change to its topic, creating a publisher on first
/// use. A key declared in `typed_outputs` rides a typed ROS message (its value
/// encoded against that message's runtime type); every other key takes the
/// untyped `std_msgs` path. `unset` keys have no ROS 2 representation and are
/// ignored.
#[allow(clippy::too_many_arguments)]
async fn publish_change(
    node: &mut Node,
    namespace: &str,
    publishers: &mut HashMap<String, KeyPublisher>,
    typed_outputs: &HashMap<String, TypedOutput>,
    typed_publishers: &mut HashMap<String, TypedKeyPublisher>,
    includes: &[profile::Include],
    registry: &Arc<Ros2Registry>,
    change: &StateChange,
) {
    for (key, maybe_value) in &change.set {
        let Some(value) = maybe_value else { continue };

        // A typed output rides its declared ROS message; its publisher is
        // created lazily like the untyped one.
        if let Some(binding) = typed_outputs.get(&key.path) {
            if !typed_publishers.contains_key(&key.path) {
                let topic = binding
                    .topic
                    .clone()
                    .unwrap_or_else(|| topic_name(namespace, &key.path));
                match setup_typed_key_publisher(node, &topic, &binding.ros_type, registry.clone()) {
                    Ok(publisher) => {
                        typed_publishers.insert(key.path.clone(), publisher);
                    }
                    Err(e) => {
                        warn!(
                            "Ros2Bridge could not create a typed publisher for key '{}': {e}",
                            key.path
                        );
                        continue;
                    }
                }
            }
            if let Some(publisher) = typed_publishers.get(&key.path) {
                publisher.publish(value).await;
            }
            continue;
        }

        if !publishers.contains_key(&key.path) {
            // An include's rewrite (first match wins) puts the key on its
            // absolute profile topic instead of the namespace convention.
            let topic = includes
                .iter()
                .filter(|include| include.flow == profile::Flow::Out)
                .find_map(|include| include.rewrite(&key.path))
                .unwrap_or_else(|| topic_name(namespace, &key.path));
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
