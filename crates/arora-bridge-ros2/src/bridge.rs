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
//! A background task owns the ROS 2 [`Node`](ros2_client::Node): it spins the
//! middleware, drives the input subscriptions, and creates publishers on
//! demand, and the async lives entirely inside it. The bridge reaches it two
//! ways. Inbound, the command channel's receiver *is* the stream the runtime
//! polls, so nothing buffers in between. Outbound, [`try_send`](Bridge::try_send)
//! merges into a latest-value-per-key map the task drains — see [`Outbound`].

use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
use log::{debug, warn};
use ros2_client::{Context, ContextOptions, Node, NodeName, NodeOptions};
use tokio::sync::mpsc as tmpsc;
use tokio_util::sync::CancellationToken;

use crate::actions;
use crate::conversions::{
    setup_key_subscriber, setup_typed_key_publisher, setup_typed_key_subscriber, topic_name,
    KeyPublisher, StateChangeStream, TypedKeyPublisher, OVERSIZED_JSON_BYTES,
};
use crate::profile;
use crate::qos::Qos;
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
    /// Delivery profile; `None` takes [`Qos::default_for`] an inbound flow
    /// (reliable — an input topic carries instructions).
    pub qos: Option<Qos>,
}

impl InputKey {
    pub fn new<S: Into<String>>(path: S, value_type: Type) -> Self {
        Self {
            path: path.into(),
            value_type,
            qos: None,
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
    /// Delivery profile; `None` takes [`Qos::default_for`] an outbound flow
    /// (sensor data — an output topic carries state).
    pub qos: Option<Qos>,
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
    /// Delivery profile; `None` takes [`Qos::default_for`] an inbound flow.
    pub qos: Option<Qos>,
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
    /// Skill endpoints: profile-declared ROS 2 actions bound to the device's
    /// task-run methods (see [`profile::ActionBinding`]). Each is checked at
    /// startup against the described methods and refused loudly when the
    /// contract does not hold.
    pub action_bindings: Vec<profile::ActionBinding>,
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
            action_bindings: Vec::new(),
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
            qos: None,
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
            qos: None,
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
            qos: None,
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
            qos: None,
        });
        self
    }

    /// Expose the device through a named [`profile::ExposureProfile`]: each
    /// endpoint becomes a typed binding on its absolute topic with its field
    /// fan-out, the profile's includes join the scalar plane's rewrite set,
    /// and its action bindings join the skill plane. Outbound endpoints route
    /// the whole message from their first route's key (field fan-in is not
    /// implemented).
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
                    qos: endpoint.qos,
                }),
                profile::Flow::Out => {
                    let Some(route) = endpoint.routes.first() else {
                        continue;
                    };
                    self.outputs.push(TypedOutput {
                        path: route.key.clone(),
                        ros_type: endpoint.ros_type,
                        topic: Some(endpoint.topic),
                        qos: endpoint.qos,
                    });
                }
            }
        }
        self.includes.extend(profile.includes);
        self.action_bindings.extend(profile.actions);
        self
    }
}

/// The outbound seam: the newest value of every key that changed since the node
/// task last drained it.
///
/// What crosses this seam is **state, not events** — the runtime hands the
/// bridge one coalesced [`StateChange`] per step, and for a given key only the
/// newest value is worth publishing. So the buffer is a map, not a queue: a key
/// written again before the drain replaces its pending value instead of queuing
/// behind it. Memory is therefore bounded by the device's key count however far
/// behind the ROS graph falls, and what does get published is always the
/// freshest value.
///
/// This is `History::KeepLast` applied one layer above DDS. QoS bounds the
/// writer's history — the samples that reached a publisher; nothing but this
/// bounds the samples that have not, and a publish path slower than the step
/// rate (a congested link, a reader that went away, a large value on the JSON
/// fallback) is exactly when they pile up.
#[derive(Default)]
struct Outbound {
    /// The pending change, merged in place. A `std` mutex: every holder does a
    /// handful of map operations and none of them awaits.
    pending: Mutex<StateChange>,
    /// Whether a value has ever been replaced before it was published — the
    /// moment the ROS graph fell behind the device, reported once.
    fell_behind: AtomicBool,
}

impl Outbound {
    /// Merge a step's change in, later wins: a set overrides an earlier unset
    /// of the same key, and an unset overrides an earlier set — the same rule
    /// the runtime coalesces its own step with.
    fn merge(&self, change: &StateChange) {
        let mut superseded = false;
        {
            let mut pending = self.pending.lock().unwrap();
            for (key, value) in &change.set {
                pending.unset.remove(key);
                superseded |= pending.set.insert(key.clone(), value.clone()).is_some();
            }
            for key in &change.unset {
                pending.set.remove(key);
                superseded |= !pending.unset.insert(key.clone());
            }
        }
        // A value replaced before it was published means the ROS graph is not
        // keeping up with the device. Said once, outside the lock: from there
        // on it is the steady state, and this runs on the device's step.
        if superseded && !self.fell_behind.swap(true, Ordering::Relaxed) {
            debug!(
                "the ROS 2 graph is draining slower than this device writes; from here on only \
                 the newest value of each key is published"
            );
        }
    }

    /// Take everything pending, leaving the buffer empty.
    fn take(&self) -> StateChange {
        std::mem::replace(&mut self.pending.lock().unwrap(), StateChange::new())
    }
}

/// A ROS 2 graph as an Arora [`Bridge`].
pub struct Ros2Bridge {
    namespace: String,
    /// The newest value of every key waiting to be published, shared with the
    /// node task.
    outbound: Arc<Outbound>,
    /// Wakes the node task when [`Outbound`] has something. Capacity one: a
    /// full channel already says "there is work", so a failed send is success.
    wake: tmpsc::Sender<()>,
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
        let (wake_tx, wake_rx) = tmpsc::channel::<()>(1);
        let outbound = Arc::new(Outbound::default());
        let cancel = CancellationToken::new();
        let namespace = config.namespace.clone();

        tokio::spawn(run_node(
            config,
            cmd_tx,
            outbound.clone(),
            wake_rx,
            cancel.clone(),
        ));

        Self {
            namespace,
            outbound,
            wake: wake_tx,
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
        // Merge the change into the pending map and nudge the node task, which
        // publishes each changed key to its topic. Never blocks and never
        // grows: a key written faster than ROS drains it keeps its newest
        // value only. A failed wake means either that the task already has
        // work queued or that it stopped — neither needs handling here (the
        // drop of the bridge cancels the task).
        self.outbound.merge(change);
        let _ = self.wake.try_send(());
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
    outbound: Arc<Outbound>,
    mut wake_rx: tmpsc::Receiver<()>,
    cancel: CancellationToken,
) {
    let Ros2BridgeConfig {
        namespace,
        domain_id,
        inputs,
        typed_inputs,
        outputs,
        includes,
        action_bindings,
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
    // Every topic this bridge subscribes to. Publishing on one would hand the
    // device its own command back: the sample re-enters as an inbound update,
    // is written to the store, comes out again on the next step, and circulates
    // for as long as the graph is up — reviving stale values whenever a real
    // writer and the echo cross. (DDS delivers a participant's own writes to
    // its own readers, and the Zenoh backend declares its subscribers with no
    // origin filter, so neither middleware saves us from it.)
    let mut subscribed: HashSet<String> = HashSet::new();
    for input in &inputs {
        subscribed.insert(topic_name(&namespace, &input.path));
        let qos = input.qos.unwrap_or(Qos::default_for(profile::Flow::In));
        match setup_key_subscriber(&mut node, &namespace, &input.path, &input.value_type, qos) {
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
        subscribed.insert(topic.clone());
        match setup_typed_key_subscriber(
            &mut node,
            &topic,
            &input.ros_type,
            input.path.clone(),
            input.routes.clone(),
            registry.clone(),
            input.qos.unwrap_or(Qos::default_for(profile::Flow::In)),
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
    // declared. Profile-declared skill endpoints join the same action plane,
    // checked against the discovered signatures.
    let (discovered, discovered_actions) =
        discover(&cmd_tx, &namespace, &registry, &action_bindings).await;
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
        match create_raw_action_server(&mut node, &action, &registry) {
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
    // Keys whose outbound topic turned out to be one we subscribe to, remembered
    // so the decision is made (and logged) once per key.
    let mut echoed: HashSet<String> = HashSet::new();
    // Keys already reported for publishing image-sized JSON, so a face that
    // renders every frame onto the scalar plane says it once, not 15 times a
    // second.
    let mut oversized: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            woken = wake_rx.recv() => {
                // The bridge was dropped along with its wake channel; `cancel`
                // normally gets here first.
                if woken.is_none() {
                    break;
                }
                let change = outbound.take();
                if change.is_empty() {
                    continue;
                }
                // The action tasks watch their runs' keys on the same outbound
                // state the topics mirror.
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
                    &subscribed,
                    &mut echoed,
                    &mut oversized,
                    &change,
                )
                .await;
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
/// (behavior-tree-`Status`-returning) methods to actions — the profile's
/// skill bindings joining the latter, each checked against the described
/// signatures. Methods whose types aren't representable on their plane, and
/// bindings whose contract does not hold, are logged and skipped (never
/// silently dropped). Empty if the runtime never answers (e.g. it stopped) —
/// the bridge then serves no methods.
async fn discover(
    cmd_tx: &fmpsc::UnboundedSender<BridgeCommand>,
    namespace: &str,
    registry: &Ros2Registry,
    action_bindings: &[profile::ActionBinding],
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
    let (mut resolved_actions, skipped_actions) =
        actions::resolve(namespace, &signatures, registry);
    if !skipped_actions.is_empty() {
        warn!(
            "Ros2Bridge skips task-run methods whose goal types are not ROS 2-representable: {}",
            skipped_actions.join(", ")
        );
    }
    let (bound_actions, refused) = actions::resolve_bound(action_bindings, &signatures, registry);
    for error in refused {
        warn!("Ros2Bridge refuses a {error}");
    }
    resolved_actions.extend(bound_actions);
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
///
/// The runtime-typed endpoints are keyed on the REP-2016 hashes the action's
/// `.action` generates — what a native `rmw_zenoh` client addresses them by —
/// when they can be: a bound action's goal, result and feedback are registry
/// messages, so its hashes reproduce `rosidl`'s. A synthesized action's result
/// and feedback are typed from what the run writes, unknown when the server is
/// made, so it keeps the placeholder — reachable by `ros2-client` peers, which
/// no ROS package could give a native client a goal for anyway.
fn create_raw_action_server(
    node: &mut Node,
    action: &actions::MethodAction,
    registry: &Ros2Registry,
) -> Result<ros2_client::RawActionServer, String> {
    let name = services::parse_name(&action.name)?;
    let hashes = action_type_hashes(action, registry);
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
        match hashes {
            Some(hashes) => node
                .create_raw_action_server_with_type_hashes(&name, &action.action_type, qos, &hashes)
                .map_err(|e| format!("{e:?}")),
            None => node
                .create_raw_action_server(&name, &action.action_type, qos)
                .map_err(|e| format!("{e:?}")),
        }
    }
    #[cfg(feature = "zenoh")]
    {
        match hashes {
            Some(hashes) => node
                .create_raw_action_server_with_type_hashes(&name, &action.action_type, &hashes)
                .map_err(|e| format!("{e:?}")),
            None => node
                .create_raw_action_server(&name, &action.action_type)
                .map_err(|e| format!("{e:?}")),
        }
    }
}

/// The REP-2016 hashes of a bound action's `_SendGoal`, `_GetResult` and
/// `_FeedbackMessage`, from its registry goal, result and feedback types;
/// `None` for a synthesized action, or one whose types the hasher cannot
/// describe (said once, as for publishers).
fn action_type_hashes(
    action: &actions::MethodAction,
    registry: &Ros2Registry,
) -> Option<ros2_client::ActionTypeHashes> {
    let actions::Wire::Bound(bound) = &action.wire else {
        return None;
    };
    let feedback = bound.feedback.as_ref()?;
    let qualified = format!(
        "{}/action/{}",
        action.action_type.package_name(),
        action.action_type.type_name()
    );
    match arora_msgs_ros2::action_hashes(
        &qualified,
        &bound.goal_type,
        &bound.result_type,
        &feedback.message,
        registry.types(),
    ) {
        Ok(hashes) => Some(ros2_client::ActionTypeHashes {
            send_goal: hashes.send_goal,
            get_result: hashes.get_result,
            feedback_message: hashes.feedback_message,
        }),
        Err(e) => {
            warn!(
                "Ros2Bridge action '{}' keeps the placeholder type hash — native rmw_zenoh clients will not reach it: {e}",
                action.name
            );
            None
        }
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
    // Keyed on the hash the service's `.srv` would generate, so a native
    // rmw_zenoh client reaches it once such a package exists; a type the
    // hasher cannot describe keeps the placeholder and says so.
    let hash = arora_msgs_ros2::service_rihs01(
        &format!("arora/srv/{}", service.service_type.type_name()),
        &service.request_type,
        &service.response_type,
        registry.types(),
    );
    if let Err(e) = &hash {
        warn!(
            "Ros2Bridge service '{}' keeps the placeholder type hash — native rmw_zenoh clients will not reach it: {e}",
            service.name
        );
    }
    #[cfg(feature = "dds")]
    let server = match &hash {
        Ok(hash) => node.create_raw_server_with_type_hash(
            &ros_name,
            &service.service_type,
            service_qos(),
            service_qos(),
            hash,
        ),
        Err(_) => node.create_raw_server(
            &ros_name,
            &service.service_type,
            service_qos(),
            service_qos(),
        ),
    };
    #[cfg(feature = "zenoh")]
    let server = match &hash {
        Ok(hash) => node.create_raw_server_with_type_hash(&ros_name, &service.service_type, hash),
        Err(_) => node.create_raw_server(&ros_name, &service.service_type),
    };
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
    subscribed: &HashSet<String>,
    echoed: &mut HashSet<String>,
    oversized: &mut HashSet<String>,
    change: &StateChange,
) {
    // Resolving a key's topic costs a format and a glob walk, so the answer is
    // cached: after the first sight of a key it is either in one of the
    // publisher maps or in `echoed`.
    for (key, maybe_value) in &change.set {
        let Some(value) = maybe_value else { continue };
        if echoed.contains(&key.path) {
            continue;
        }

        // A typed output rides its declared ROS message; its publisher is
        // created lazily like the untyped one.
        if let Some(binding) = typed_outputs.get(&key.path) {
            if !typed_publishers.contains_key(&key.path) {
                let topic = binding
                    .topic
                    .clone()
                    .unwrap_or_else(|| topic_name(namespace, &key.path));
                if subscribed.contains(&topic) {
                    warn!(
                        "Ros2Bridge does not publish key '{}': its topic '{topic}' is one this \
                         bridge subscribes to, and echoing it back would feed the device its own \
                         commands",
                        key.path
                    );
                    echoed.insert(key.path.clone());
                    continue;
                }
                match setup_typed_key_publisher(
                    node,
                    &topic,
                    &binding.ros_type,
                    registry.clone(),
                    binding.qos.unwrap_or(Qos::default_for(profile::Flow::Out)),
                ) {
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
            // absolute profile topic instead of the namespace convention, and
            // its delivery profile with it.
            let matched = includes
                .iter()
                .filter(|include| include.flow == profile::Flow::Out)
                .find_map(|include| Some((include.rewrite(&key.path)?, include.qos)));
            let (topic, qos) = match matched {
                Some((topic, qos)) => (topic, qos),
                None => (topic_name(namespace, &key.path), None),
            };
            if subscribed.contains(&topic) {
                warn!(
                    "Ros2Bridge does not publish key '{}': its topic '{topic}' is one this bridge \
                     subscribes to, and echoing it back would feed the device its own commands",
                    key.path
                );
                echoed.insert(key.path.clone());
                continue;
            }
            let qos = qos.unwrap_or(Qos::default_for(profile::Flow::Out));
            match KeyPublisher::create(node, &topic, value, qos) {
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
            // The value still ships — dropping a key the device exposes would be
            // worse — but an image-sized JSON sample is a configuration mistake
            // that costs memory per sample in the middleware, so say so.
            let json_bytes = publisher.publish(value).await;
            if json_bytes > OVERSIZED_JSON_BYTES && oversized.insert(key.path.clone()) {
                warn!(
                    "key '{}' publishes {json_bytes} bytes of JSON on the scalar plane, far past \
                     what a ROS 2 topic should carry: it fragments in the middleware and costs \
                     memory per sample. Declare it as a typed output — an image belongs on \
                     sensor_msgs/CompressedImage.",
                    key.path
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::data::Key;
    use arora_types::value::Value;

    fn set(key: &str, value: f64) -> StateChange {
        StateChange::set(Key::from(key), Value::F64(value))
    }

    /// The seam keeps state, not history: a key written twice before the node
    /// task drains publishes once, at its newest value. This is what bounds the
    /// bridge's memory by the device's key count.
    #[test]
    fn a_key_written_twice_before_the_drain_keeps_only_the_newest_value() {
        let outbound = Outbound::default();
        outbound.merge(&set("rig/jaw", 0.1));
        outbound.merge(&set("rig/jaw", 0.2));
        outbound.merge(&set("rig/brow", 0.9));

        let drained = outbound.take();
        assert_eq!(drained.set.len(), 2, "one entry per key, not per write");
        assert_eq!(
            drained.set[&Key::from("rig/jaw")],
            Some(Value::F64(0.2)),
            "the later write wins"
        );
        assert_eq!(drained.set[&Key::from("rig/brow")], Some(Value::F64(0.9)));
    }

    /// Later wins across the two halves too, so a key cannot come out of the
    /// seam both set and unset.
    #[test]
    fn a_set_and_an_unset_of_one_key_resolve_to_whichever_came_last() {
        let unset = |key: &str| {
            let mut change = StateChange::new();
            change.unset.insert(Key::from(key));
            change
        };

        let outbound = Outbound::default();
        outbound.merge(&set("rig/jaw", 0.1));
        outbound.merge(&unset("rig/jaw"));
        let drained = outbound.take();
        assert!(drained.set.is_empty(), "the unset erased the pending set");
        assert!(drained.unset.contains(&Key::from("rig/jaw")));

        let outbound = Outbound::default();
        outbound.merge(&unset("rig/jaw"));
        outbound.merge(&set("rig/jaw", 0.3));
        let drained = outbound.take();
        assert!(drained.unset.is_empty(), "the set erased the pending unset");
        assert_eq!(drained.set[&Key::from("rig/jaw")], Some(Value::F64(0.3)));
    }

    /// Draining empties the seam: what has been published is not published
    /// again on the next wake.
    #[test]
    fn taking_the_pending_change_leaves_the_seam_empty() {
        let outbound = Outbound::default();
        outbound.merge(&set("rig/jaw", 0.1));
        assert!(!outbound.take().is_empty());
        assert!(outbound.take().is_empty());
    }
}
