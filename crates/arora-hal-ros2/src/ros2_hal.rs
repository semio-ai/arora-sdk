use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream::unfold;
use futures::StreamExt;
use log::{debug, error, info, trace, warn};
use ros2_client::{Name, Node, NodeOptions, DEFAULT_PUBLISHER_QOS, DEFAULT_SUBSCRIPTION_QOS};

use arora_hal::{Hal, HalAssets, HalDescription, HalError, HalResult};
use arora_types::data::{Key, State, StateChange, Subscription};
use arora_types::value::Value;

use crate::config::{
    get_joint_ids_from_glb_file, JointIdMapping, ROS2RobotConfig, TopicConfig, TopicDirection,
    TopicMapping,
};
use crate::conversions::{
    message_to_state_change, ConvertingStateChangePublisher, FromStateChange, StateChangePublisher,
    ToStateChange,
};
use crate::ros2_error::ROS2RobotError;
use crate::{msgs, msgs::MessageType};

/// A stream of state changes converted from the messages of one subscribed topic.
type StateChangeStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<StateChange, ROS2RobotError>> + Send>>;

/// A ROS2 robot as an [`arora_hal::Hal`], using native ros2_client for ROS2
/// communication.
///
/// The topology (topics, mappings, joint IDs) is fixed at construction from a
/// [`ROS2RobotConfig`]; the remaining state is interior-mutable so every trait
/// method takes `&self`.
pub struct Ros2Hal {
    // Inner ROS2 client node
    node: Node,

    // Configuration
    config: ROS2RobotConfig,
    joint_ids_to_ros_names: HashMap<String, String>,

    // Observers registered through `updates()`; the ROS-side task feeds each one
    subscribers: Arc<Mutex<Vec<std::sync::mpsc::Sender<StateChange>>>>,

    // Publishers that also convert StateChange to typed messages
    publishers: HashMap<String, Box<dyn StateChangePublisher>>,

    // Current state cache, shared with the ROS-side subscriber task
    current_state: Arc<Mutex<State>>,

    // Handles to abort background tasks when the HAL is dropped
    spinner_abort_handle: tokio::task::AbortHandle,
    subscriber_task_abort_handle: tokio::task::AbortHandle,
}

impl Ros2Hal {
    /// Create a new Ros2Hal with the given configuration.
    ///
    /// Iterates through the configured topics, sets up the appropriate
    /// publishers and/or subscribers, and starts a background task to process
    /// incoming state changes from subscribers. Must be called within a Tokio
    /// runtime that outlives the HAL (the ROS node spinner and the subscriber
    /// task run on it).
    pub async fn new(config: ROS2RobotConfig) -> Result<Self, ROS2RobotError> {
        config.validate()?;

        // Determine joint ID mappings
        let joint_ros_names_to_ids = match &config.joint_ids {
            JointIdMapping::FromGLB => {
                if let Some(ref glb_path) = config.model_glb_path {
                    get_joint_ids_from_glb_file(glb_path)?
                } else {
                    return Err(ROS2RobotError::ConfigError(
                        "JointIdMapping::FromGLB specified but no model_glb_path provided"
                            .to_string(),
                    ));
                }
            }
            JointIdMapping::Override(map) => map.clone(),
            JointIdMapping::Extend(map) => {
                let mut base_map = if let Some(ref glb_path) = config.model_glb_path {
                    get_joint_ids_from_glb_file(glb_path)?
                } else {
                    return Err(ROS2RobotError::ConfigError(
                        "JointIdMapping::Extend specified but no model_glb_path provided"
                            .to_string(),
                    ));
                };
                for (k, v) in map {
                    base_map.insert(k.clone(), v.clone());
                }
                base_map
            }
        };
        let joint_ids_to_ros_names = joint_ros_names_to_ids
            .iter()
            .map(|(ros_name, arora_id)| (arora_id.clone(), ros_name.clone()))
            .collect();

        let domain_id: u16 = config.domain_id.unwrap_or(0);
        println!("Connecting to ROS2 domain {}", domain_id);

        // Create ROS2 context and node
        let context_options = ros2_client::ContextOptions::new().domain_id(domain_id);
        let ctx = ros2_client::Context::with_options(context_options)
            .map_err(|e| ROS2RobotError::InitializationError(format!("{e:?}")))?;
        let node_name = ros2_client::NodeName::new("/", "arora")
            .map_err(|e| ROS2RobotError::ConfigError(format!("Invalid node name: {}", e)))?;
        let mut node = ctx
            .new_node(node_name, NodeOptions::new().enable_rosout(true))
            .map_err(|e| ROS2RobotError::InitializationError(format!("{e:?}")))?;

        // Start the node spinner in a background task (required when waiting for subscriptions)
        let spinner = node
            .spinner()
            .map_err(|e| ROS2RobotError::InitializationError(format!("{e:?}")))?;
        let spinner_abort_handle = tokio::spawn(spinner.spin()).abort_handle();

        let joint_ids = Arc::new(joint_ros_names_to_ids);
        let mut subscription_streams = Vec::new();
        let mut publishers: HashMap<String, Box<dyn StateChangePublisher>> = HashMap::new();

        // Set up mappings from config
        for topic_config in &config.topics {
            trace!("Processing topic config: {topic_config:?}");

            // Set up subscribers for input topics
            if matches!(
                topic_config.direction,
                TopicDirection::Subscribe | TopicDirection::Both
            ) {
                debug!("Configuring subscriber for topic: {}", topic_config.name);

                let stream =
                    Self::setup_subscriber(&mut node, topic_config.clone(), joint_ids.clone())
                        .await
                        .map_err(|e| ROS2RobotError::SubscriberError {
                            topic: topic_config.name.clone(),
                            reason: format!("{e:?}"),
                        })?;
                subscription_streams.push(stream);
            }

            // Set up publishers for output topics
            if matches!(
                topic_config.direction,
                TopicDirection::Publish | TopicDirection::Both
            ) {
                debug!("Configuring publisher for topic: {}", topic_config.name);
                Self::setup_publisher(
                    &mut node,
                    &mut publishers,
                    &topic_config.name,
                    &topic_config.message_type,
                )?;
            }
        }

        let subscribers: Arc<Mutex<Vec<std::sync::mpsc::Sender<StateChange>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let current_state = Arc::new(Mutex::new(State::new()));

        // Clone the shared state for the task
        let subscribers_clone = subscribers.clone();
        let current_state_clone = current_state.clone();

        // Process state changes coming from the robot
        let subscriber_task_abort_handle = tokio::spawn(async move {
            debug!("State change processing task started.");

            let combined_subscription_stream =
                futures::stream::select_all(subscription_streams.iter_mut());
            tokio::pin!(combined_subscription_stream);

            loop {
                // Try to take next ROS message (already converted to StateChange)
                let res = combined_subscription_stream.next().await;

                match res {
                    Some(Ok(state_change)) => {
                        debug!("Received state change from subscriber: {:?}", state_change);
                        // Update current state
                        {
                            let mut current_state = current_state_clone
                                .lock()
                                .expect("current state lock poisoned");
                            current_state.apply(state_change.clone());
                            debug!("State change processing task: Local state updated.");
                        }
                        // Notify observers, dropping the disconnected ones
                        {
                            let mut subscribers =
                                subscribers_clone.lock().expect("subscribers lock poisoned");
                            subscribers.retain(|tx| tx.send(state_change.clone()).is_ok());
                            debug!("State change processing task: Observers notified.");
                        }
                    }
                    Some(Err(e)) => {
                        error!("Error from subscriber stream: {:?}", e);
                    }
                    None => {
                        warn!("Subscriber stream ended unexpectedly.");
                        break;
                    }
                }
            }

            debug!("State change processing task finished.");
        })
        .abort_handle();

        info!("Ros2Hal initialized successfully.");
        Ok(Self {
            node,
            config,
            joint_ids_to_ros_names,
            subscribers,
            publishers,
            current_state,
            spinner_abort_handle,
            subscriber_task_abort_handle,
        })
    }

    /// Dynamically dispatches to set up the correct type of subscriber based on the topic's message type.
    ///
    /// # Arguments
    ///
    /// * `topic` - The topic configuration for the subscriber.
    ///
    /// # Returns
    ///
    /// A stream of state changes, or a `ROS2RobotError` if setup fails or the message type is unsupported.
    async fn setup_subscriber(
        node: &mut Node,
        topic: TopicConfig,
        joint_ids: Arc<HashMap<String, String>>,
    ) -> Result<StateChangeStream, ROS2RobotError> {
        debug!("Setting up subscriber for topic: {}", topic.name);

        let state_change_stream = match topic.message_type.as_str() {
            msgs::JointState::MESSAGE_TYPE_STR => Self::setup_typed_subscriber::<msgs::JointState>(
                node,
                &topic.name,
                topic.mapping,
                joint_ids,
            ),
            msgs::Float64MultiArray::MESSAGE_TYPE_STR => {
                Self::setup_typed_subscriber::<msgs::Float64MultiArray>(
                    node,
                    &topic.name,
                    topic.mapping,
                    joint_ids,
                )
            }
            msgs::String::MESSAGE_TYPE_STR => Self::setup_typed_subscriber::<msgs::String>(
                node,
                &topic.name,
                topic.mapping,
                joint_ids,
            ),
            // Add more message types as needed
            _ => {
                error!(
                    "Unsupported message type for subscriber: {}",
                    topic.message_type
                );
                return Err(ROS2RobotError::UnsupportedMessageType(
                    topic.message_type.clone(),
                ));
            }
        }?;

        info!("Successfully set up subscriber for topic: {}", topic.name);
        Ok(state_change_stream)
    }

    /// Sets up a type-specific subscriber for a given topic.
    ///
    /// Creates a subscriber on the node and wraps it into a stream that
    /// converts received messages into state changes.
    fn setup_typed_subscriber<T: MessageType + ToStateChange + 'static + std::fmt::Debug>(
        node: &mut Node,
        topic_name: &str,
        topic_mapping: TopicMapping,
        joint_ids: Arc<HashMap<String, String>>,
    ) -> Result<StateChangeStream, ROS2RobotError> {
        debug!("Setting up typed subscriber for topic: {}", topic_name);

        let ros_topic_name = Name::parse(topic_name).map_err(|e| {
            ROS2RobotError::ConfigError(format!("Invalid topic name '{}': {}", topic_name, e))
        })?;

        let topic = node
            .create_topic(
                &ros_topic_name,
                T::message_type_name(),
                &DEFAULT_SUBSCRIPTION_QOS,
            )
            .map_err(|e| ROS2RobotError::SubscriberError {
                topic: topic_name.to_owned(),
                reason: format!("{e:?}"),
            })?;

        let subscription = node.create_subscription::<T>(&topic, None).map_err(|e| {
            ROS2RobotError::SubscriberError {
                topic: topic_name.to_owned(),
                reason: format!("{e:?}"),
            }
        })?;

        let topic_name = topic_name.to_owned();
        let stream = unfold(subscription, move |subscription| {
            let topic_name = topic_name.clone();
            let topic_mapping = topic_mapping.clone();
            let joint_ids = joint_ids.clone();
            async move {
                match subscription.async_take().await {
                    Ok((msg, _info)) => {
                        debug!("Received message on topic '{}': {:?}", topic_name, msg);
                        let state_change = message_to_state_change::<T>(
                            msg,
                            &topic_mapping,
                            &topic_name,
                            &joint_ids,
                        );
                        debug!(
                            "Converted message to state change for topic '{}': {:?}",
                            topic_name, state_change
                        );
                        Some((state_change, subscription))
                    }
                    Err(e) => Some((
                        Err(ROS2RobotError::SubscriberError {
                            topic: topic_name,
                            reason: format!("{e:?}"),
                        }),
                        subscription,
                    )),
                }
            }
        });

        Ok(stream.boxed())
    }

    /// Dynamically dispatches to set up the correct type of publisher based on the topic's message type.
    fn setup_publisher(
        node: &mut Node,
        publishers: &mut HashMap<String, Box<dyn StateChangePublisher>>,
        topic_name: &str,
        msg_type: &str,
    ) -> Result<(), ROS2RobotError> {
        debug!("Setting up publisher for topic: {}", topic_name);

        match msg_type {
            msgs::JointState::MESSAGE_TYPE_STR => {
                Self::setup_typed_publisher::<msgs::JointState>(node, publishers, topic_name)?;
            }
            msgs::Float64MultiArray::MESSAGE_TYPE_STR => {
                Self::setup_typed_publisher::<msgs::Float64MultiArray>(
                    node, publishers, topic_name,
                )?;
            }
            msgs::JointAnglesWithSpeed::MESSAGE_TYPE_STR => {
                Self::setup_typed_publisher::<msgs::JointAnglesWithSpeed>(
                    node, publishers, topic_name,
                )?;
            }
            msgs::JointTrajectory::MESSAGE_TYPE_STR => {
                Self::setup_typed_publisher::<msgs::JointTrajectory>(node, publishers, topic_name)?;
            }
            msgs::String::MESSAGE_TYPE_STR => {
                Self::setup_typed_publisher::<msgs::String>(node, publishers, topic_name)?;
            }
            // Add more message types as needed
            _ => {
                error!("Unsupported message type for publisher: {}", msg_type);
                return Err(ROS2RobotError::UnsupportedMessageType(msg_type.to_owned()));
            }
        }

        info!("Successfully set up publisher for topic: {}", topic_name);
        Ok(())
    }

    /// Sets up a type-specific publisher for a given topic.
    ///
    /// Creates a native ros2_client publisher wrapped in a converting publisher.
    fn setup_typed_publisher<T: MessageType + FromStateChange + serde::Serialize + 'static>(
        node: &mut Node,
        publishers: &mut HashMap<String, Box<dyn StateChangePublisher>>,
        topic_name: &str,
    ) -> Result<(), ROS2RobotError> {
        debug!("Setting up typed publisher for topic: {}", topic_name);

        let ros_topic_name = Name::parse(topic_name).map_err(|e| {
            ROS2RobotError::ConfigError(format!("Invalid topic name '{}': {}", topic_name, e))
        })?;

        let topic = node
            .create_topic(
                &ros_topic_name,
                T::message_type_name(),
                &DEFAULT_PUBLISHER_QOS,
            )
            .map_err(|e| ROS2RobotError::PublisherError {
                topic: topic_name.to_owned(),
                reason: format!("{e:?}"),
            })?;

        let publisher = node.create_publisher::<T>(&topic, None).map_err(|e| {
            ROS2RobotError::PublisherError {
                topic: topic_name.to_owned(),
                reason: format!("{e:?}"),
            }
        })?;

        let typed_publisher = ConvertingStateChangePublisher::new(publisher, topic_name.to_owned());

        debug!("Typed publisher created for topic: {}", topic_name);

        publishers.insert(topic_name.to_owned(), Box::new(typed_publisher));

        Ok(())
    }
}

#[async_trait]
impl Hal for Ros2Hal {
    /// Describe the device from the robot configuration.
    async fn describe(&self) -> HalDescription {
        debug!("describe called.");
        HalDescription {
            model_family: self.config.model_family.clone(),
            hardware_version: self.config.hardware_version.clone(),
            software_version: self.config.software_version.clone(),
        }
    }

    /// Retrieves the current values for the given keys from the local state
    /// cache. Absent keys read as `None`.
    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
        debug!("Received read request for {} keys", keys.len());
        let state = self.current_state.lock().expect("state lock poisoned");
        Ok(keys
            .iter()
            .map(|key| state.get(key).cloned().flatten())
            .collect())
    }

    /// Retrieves all key/value pairs currently held in the local state cache.
    async fn read_all(&self) -> HalResult<State> {
        debug!("Received read_all request");
        let state = self.current_state.lock().expect("state lock poisoned");
        debug!(
            "Returning all {} key-value pairs from local state",
            state.storage.len()
        );
        Ok(state.clone())
    }

    /// Applies actuator/state changes to the robot.
    ///
    /// Publishes target values to every matching ROS2 topic and updates the
    /// local state cache. Per-topic failures are logged as warnings; the call
    /// only errors when every matching topic failed to publish.
    async fn write(&self, changes: StateChange) -> HalResult<()> {
        debug!(
            "Received write with state changes: {} set, {} unset",
            changes.set.len(),
            changes.unset.len()
        );

        let mut failures: Vec<String> = Vec::new();
        let mut published = 0usize;
        let mut matched = 0usize;

        for topic_config in &self.config.topics {
            // Skip topics not configured for publishing
            if !matches!(
                topic_config.direction,
                TopicDirection::Publish | TopicDirection::Both
            ) {
                continue;
            }

            let topic_name = &topic_config.name;
            let topic_mapping = &topic_config.mapping;

            // Skip if it is clear that this state change does not affect the topic
            if let TopicMapping::StandardMessage { field_mappings } = topic_mapping {
                if !field_mappings
                    .values()
                    .any(|v| changes.contains(&Key::from(v.clone())))
                {
                    continue;
                }
            }
            matched += 1;

            // Check if we have a publisher for this topic
            let Some(publisher) = self.publishers.get(topic_name) else {
                let msg = format!("No publisher found for topic: {topic_name}");
                error!("{msg}");
                failures.push(msg);
                continue;
            };
            debug!("Found publisher for topic '{}'", topic_name);

            // Publish using the trait-based publisher (handles conversion internally)
            publisher.wait_for_subscription(&self.node).await; // make sure it is ready first
            match publisher
                .publish_state_change(&changes, topic_mapping, &self.joint_ids_to_ros_names)
                .await
            {
                Ok(()) => {
                    debug!("Successfully published message to topic '{}'", topic_name);
                    published += 1;
                }
                Err(e) => {
                    let msg = format!("Failed to publish to topic '{topic_name}': {e}");
                    error!("{msg}");
                    failures.push(msg);
                }
            }
        }

        if matched == 0 {
            warn!("No suitable topic mapping found for state change");
        }

        // Update local state cache
        {
            let mut state = self.current_state.lock().expect("state lock poisoned");
            state.apply(changes);
        }

        if failures.is_empty() || published > 0 {
            for failure in &failures {
                warn!("Partial write failure: {failure}");
            }
            Ok(())
        } else {
            Err(HalError::Other(format!(
                "write failed on every matching topic: {}",
                failures.join("; ")
            )))
        }
    }

    /// A feed of the changes the robot reports.
    ///
    /// Each subscription immediately receives the current state as one
    /// `StateChange` (when non-empty), then every subsequent change.
    fn updates(&self) -> Subscription {
        debug!("Received updates request");
        let (tx, rx) = std::sync::mpsc::channel();

        let snapshot = self
            .current_state
            .lock()
            .expect("state lock poisoned")
            .clone();
        if !snapshot.is_empty() {
            let _ = tx.send(StateChange {
                set: snapshot.storage,
                unset: Default::default(),
            });
        }

        self.subscribers
            .lock()
            .expect("subscribers lock poisoned")
            .push(tx);
        Subscription::new(rx)
    }
}

#[async_trait]
impl HalAssets for Ros2Hal {
    /// Get the GLB model file as raw bytes.
    async fn model_glb(&self) -> HalResult<Option<Vec<u8>>> {
        debug!("model_glb called.");

        if let Some(ref glb_path) = self.config.model_glb_path {
            match std::fs::read(glb_path) {
                Ok(bytes) => {
                    debug!(
                        "Successfully read GLB file: {} ({} bytes)",
                        glb_path,
                        bytes.len()
                    );
                    Ok(Some(bytes))
                }
                Err(e) => {
                    error!("Failed to read GLB file {}: {}", glb_path, e);
                    Err(HalError::Other(format!("Failed to read GLB file: {}", e)))
                }
            }
        } else {
            debug!("No GLB model path configured");
            Ok(None)
        }
    }
}

// Implement Drop to ensure cleanup
impl Drop for Ros2Hal {
    fn drop(&mut self) {
        debug!("Dropping Ros2Hal, aborting background tasks");
        self.spinner_abort_handle.abort();
        self.subscriber_task_abort_handle.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{configs::nao, JointStateConversion};

    #[tokio::test]
    async fn test_model_glb_with_nao_config() {
        // Create a NAO configuration
        let config = nao::create_config();

        // Create a HAL with this config
        let hal = Ros2Hal::new(config).await;

        // Skip test if ROS2 environment is not available
        if hal.is_err() {
            println!("Skipping test - ROS2 environment not available");
            return;
        }

        let hal = hal.unwrap();

        // Call model_glb
        let result = hal.model_glb().await;

        // Assert the result is successful
        assert!(result.is_ok(), "model_glb should succeed");

        let glb_data = result.unwrap();

        // Assert we got Some data
        assert!(glb_data.is_some(), "NAO config should return GLB data");

        let bytes = glb_data.unwrap();

        // Check that we got some data
        assert!(!bytes.is_empty(), "GLB data should not be empty");

        // Check that it starts with the glTF magic number
        assert_eq!(
            &bytes[0..4],
            b"glTF",
            "GLB file should start with 'glTF' magic number"
        );

        println!("Successfully retrieved {} bytes of GLB data", bytes.len());
    }

    #[tokio::test]
    async fn test_model_glb_with_no_path() {
        // Create a config with no model_glb_path
        let config = ROS2RobotConfig {
            domain_id: None,
            topics: vec![TopicConfig::new::<msgs::JointState>(
                "/joint_states",
                TopicDirection::Subscribe,
                TopicMapping::JointState {
                    conversion: JointStateConversion::Standard,
                },
            )],
            joint_ids: JointIdMapping::Override(HashMap::new()),
            model_glb_path: None,
            ..Default::default()
        };

        // Create a HAL with this config
        let hal = Ros2Hal::new(config).await;

        // Skip test if ROS2 environment is not available
        if hal.is_err() {
            println!("Skipping test - ROS2 environment not available");
            return;
        }

        let hal = hal.unwrap();

        // Call model_glb
        let result = hal.model_glb().await;

        // Assert the result is successful
        assert!(result.is_ok(), "model_glb should succeed even with no path");

        let glb_data = result.unwrap();

        // Assert we got None
        assert!(
            glb_data.is_none(),
            "Should return None when no model path is configured"
        );
    }

    #[test]
    fn test_nao_config_has_model_path() {
        // Test that NAO config has a model path set
        let config = nao::create_config();

        assert!(
            config.model_glb_path.is_some(),
            "NAO config should have a model_glb_path"
        );

        let path = config.model_glb_path.unwrap();
        assert!(
            path.ends_with("nao.glb"),
            "NAO config path should end with 'nao.glb', got: {}",
            path
        );

        // Test that the file exists and can be read
        let glb_data = std::fs::read(&path);
        assert!(
            glb_data.is_ok(),
            "Should be able to read GLB file at: {}",
            path
        );

        let bytes = glb_data.unwrap();
        assert!(!bytes.is_empty(), "GLB file should not be empty");
        assert_eq!(
            &bytes[0..4],
            b"glTF",
            "GLB file should start with 'glTF' magic number"
        );

        println!("NAO model GLB path: {}", path);
        println!("GLB file size: {} bytes", bytes.len());
    }

    /// Creates a HAL, subscribes to its updates, publishes JointState
    /// messages from a separate node, and verifies the HAL reports them.
    #[tokio::test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
                  has no unicast-peer/interface config); these run on Linux CI. To run locally, \
                  ensure an active multicast-capable interface and use `--ignored`."
    )]
    async fn test_hal_updates_with_fake_ros() {
        use crate::get_now;
        use rand::Rng;
        use ros2_client::{Name as RosName, NodeName as RosNodeName, DEFAULT_PUBLISHER_QOS};
        use std::time::Duration;

        // Use an isolated domain to avoid interacting with any locally-running ROS graph.
        let domain_id: u16 = rand::rng().random_range(1..=200);

        // Create a minimal config similar to NAO but with our isolated domain
        let config = ROS2RobotConfig {
            domain_id: Some(domain_id),
            topics: vec![TopicConfig::new::<msgs::JointState>(
                "/joint_states",
                TopicDirection::Subscribe,
                TopicMapping::JointState {
                    conversion: JointStateConversion::Standard,
                },
            )],
            joint_ids: JointIdMapping::Override(HashMap::from([
                ("HeadYaw".to_string(), "head_yaw".to_string()),
                ("HeadPitch".to_string(), "head_pitch".to_string()),
            ])),
            ..Default::default()
        };

        let hal = match Ros2Hal::new(config).await {
            Ok(hal) => hal,
            Err(e) => {
                eprintln!("Skipping test - failed to create HAL: {e:?}");
                return;
            }
        };

        // Subscribe to updates: the state is empty, so nothing arrives yet
        let subscription = hal.updates();
        assert!(
            subscription.try_recv().is_none(),
            "No update expected while the state is empty"
        );

        // Create a separate publisher node to simulate external ROS traffic
        let context_options = ros2_client::ContextOptions::new().domain_id(domain_id);
        let pub_ctx = ros2_client::Context::with_options(context_options).unwrap();
        let pub_node_name = RosNodeName::new("/", &format!("fake_robot_publisher_{domain_id}"))
            .expect("valid node name");
        let mut pub_node = pub_ctx.new_node(pub_node_name, NodeOptions::new()).unwrap();
        tokio::spawn(pub_node.spinner().unwrap().spin());

        let topic_name = RosName::parse("/joint_states").expect("valid topic name");
        let pub_topic = pub_node
            .create_topic(
                &topic_name,
                msgs::JointState::message_type_name(),
                &DEFAULT_PUBLISHER_QOS,
            )
            .expect("create publisher topic");
        let publisher = pub_node
            .create_publisher::<msgs::JointState>(&pub_topic, None)
            .expect("create publisher");
        tokio::time::timeout(
            Duration::from_secs(5),
            publisher.wait_for_subscription(&pub_node),
        )
        .await
        .expect(
            "DDS discovery timed out (5s): the external publisher never matched the \
             HAL's subscription. On macOS this is usually multicast SPDP \
             discovery failing on loopback — see test diagnostics.",
        );

        // Publish a JointState message (simulating robot publishing its state)
        let msg = msgs::JointState {
            header: msgs::Header {
                stamp: get_now(),
                frame_id: "base_link".to_string(),
            },
            name: vec!["HeadYaw".to_string(), "HeadPitch".to_string()],
            position: vec![0.5, -0.3],
            velocity: vec![0.1, 0.2],
            effort: vec![],
        };

        tokio::spawn(async move {
            loop {
                publisher
                    .async_publish(msg.clone())
                    .await
                    .expect("publish joint state");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        // Wait for the next state change: it should reflect the published joint states
        let state_change = tokio::task::spawn_blocking(move || subscription.recv())
            .await
            .expect("receiver task panicked")
            .expect("channel closed unexpectedly");

        println!(
            "Received state change with {} keys: {:?}",
            state_change.set.len(),
            state_change.set.keys().collect::<Vec<_>>()
        );
        assert!(!state_change.is_empty(), "Expected non-empty state change");

        // Verify we received the expected state changes
        let head_yaw_pos = Key::from("head_yaw.position".to_string());
        let head_pitch_pos = Key::from("head_pitch.position".to_string());

        assert!(
            state_change.set.contains_key(&head_yaw_pos),
            "Expected head_yaw.position in state change, got keys: {:?}",
            state_change.set.keys().collect::<Vec<_>>()
        );
        assert!(
            state_change.set.contains_key(&head_pitch_pos),
            "Expected head_pitch.position in state change"
        );

        assert_eq!(
            state_change.set.get(&head_yaw_pos),
            Some(&Some(Value::from(0.5))),
            "head_yaw.position should be 0.5"
        );
        assert_eq!(
            state_change.set.get(&head_pitch_pos),
            Some(&Some(Value::from(-0.3))),
            "head_pitch.position should be -0.3"
        );

        println!("Successfully received state change: {:?}", state_change);
    }

    /// Creates a HAL with a Publish-direction topic, calls `write()` with a
    /// StateChange, and verifies an external subscriber receives the correctly
    /// converted ROS2 message.
    #[tokio::test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
                  has no unicast-peer/interface config); these run on Linux CI. To run locally, \
                  ensure an active multicast-capable interface and use `--ignored`."
    )]
    async fn test_hal_publish_via_write() {
        use rand::Rng;
        use ros2_client::{Name as RosName, NodeName as RosNodeName, DEFAULT_PUBLISHER_QOS};
        use std::time::Duration;

        // Use an isolated domain to avoid interacting with any locally-running ROS graph.
        let domain_id: u16 = rand::rng().random_range(1..=200);

        // Create a config with a Publish-direction topic
        let config = ROS2RobotConfig {
            domain_id: Some(domain_id),
            topics: vec![TopicConfig::new::<msgs::JointState>(
                "/joint_commands",
                TopicDirection::Publish,
                TopicMapping::JointState {
                    conversion: JointStateConversion::Standard,
                },
            )],
            joint_ids: JointIdMapping::Override(HashMap::from([
                ("HeadYaw".to_string(), "head_yaw".to_string()),
                ("HeadPitch".to_string(), "head_pitch".to_string()),
            ])),
            ..Default::default()
        };

        let hal = Ros2Hal::new(config).await.expect("failed to create HAL");

        // Create a separate subscriber node to receive messages from the HAL
        let context_options = ros2_client::ContextOptions::new().domain_id(domain_id);
        let sub_ctx = ros2_client::Context::with_options(context_options)
            .expect("failed to create subscriber context");

        let sub_node_name = RosNodeName::new("/", &format!("test_subscriber_{domain_id}"))
            .expect("valid node name");
        let mut sub_node = sub_ctx
            .new_node(sub_node_name, NodeOptions::new())
            .expect("failed to create subscriber node");
        tokio::spawn(sub_node.spinner().unwrap().spin());

        let topic_name = RosName::parse("/joint_commands").expect("valid topic name");
        // Use DEFAULT_PUBLISHER_QOS for the subscriber topic to match the publisher's QoS
        // (particularly Reliability::Reliable vs BestEffort)
        let sub_topic = sub_node
            .create_topic(
                &topic_name,
                msgs::JointState::message_type_name(),
                &DEFAULT_PUBLISHER_QOS,
            )
            .expect("create subscriber topic");
        let subscriber = sub_node
            .create_subscription::<msgs::JointState>(&sub_topic, None)
            .expect("create subscriber");

        // Give time for DDS discovery between subscriber and the HAL's publisher
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Create a state change to publish
        // Note: FromStateChange expects "target_position" component for publishing
        let mut state_change = StateChange::new();
        state_change.set.insert(
            Key::from("head_yaw.target_position".to_string()),
            Some(Value::from(1.23)),
        );
        state_change.set.insert(
            Key::from("head_pitch.target_position".to_string()),
            Some(Value::from(-0.45)),
        );

        // Spawn a task to repeatedly publish via the HAL's write method
        let state_change_clone = state_change.clone();
        tokio::spawn(async move {
            loop {
                let _ = hal.write(state_change_clone.clone()).await;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        // Wait for the message to be received by the external subscriber with 10s timeout
        // Use a loop because async_take returns immediately if no message is available
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let msg = loop {
            if tokio::time::Instant::now() >= deadline {
                panic!("timeout waiting for JointState message");
            }
            match subscriber.async_take().await {
                Ok((msg, _info)) => break msg,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };

        println!("Received JointState message: {:?}", msg);

        // Verify the message content
        // The conversion maps joint_id (head_yaw) back to ROS name (HeadYaw)
        assert!(
            msg.name.contains(&"HeadYaw".to_string()),
            "Message should contain HeadYaw joint, got: {:?}",
            msg.name
        );
        assert!(
            msg.name.contains(&"HeadPitch".to_string()),
            "Message should contain HeadPitch joint, got: {:?}",
            msg.name
        );

        // Find indices and check positions
        let head_yaw_idx = msg
            .name
            .iter()
            .position(|n| n == "HeadYaw")
            .expect("HeadYaw index");
        let head_pitch_idx = msg
            .name
            .iter()
            .position(|n| n == "HeadPitch")
            .expect("HeadPitch index");

        assert!(
            (msg.position[head_yaw_idx] - 1.23).abs() < 0.001,
            "HeadYaw position should be 1.23, got: {}",
            msg.position[head_yaw_idx]
        );
        assert!(
            (msg.position[head_pitch_idx] - (-0.45)).abs() < 0.001,
            "HeadPitch position should be -0.45, got: {}",
            msg.position[head_pitch_idx]
        );

        println!("Successfully published and received JointState: {:?}", msg);
    }

    /// Verifies the HAL subscribes to a String topic and converts the message
    /// to a StateChange visible through `updates()`.
    #[tokio::test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
                  has no unicast-peer/interface config); these run on Linux CI. To run locally, \
                  ensure an active multicast-capable interface and use `--ignored`."
    )]
    async fn test_hal_subscribe_string_topic() {
        use rand::Rng;
        use ros2_client::{Name as RosName, NodeName as RosNodeName, DEFAULT_PUBLISHER_QOS};
        use std::time::Duration;

        // Use an isolated domain to avoid interacting with any locally-running ROS graph.
        let domain_id: u16 = rand::rng().random_range(1..=200);

        // Create a config with a Subscribe-direction String topic
        // field_mappings maps ROS field "data" to the Arora key "text"
        let config = ROS2RobotConfig {
            domain_id: Some(domain_id),
            topics: vec![TopicConfig::new::<msgs::String>(
                "/speech_status",
                TopicDirection::Subscribe,
                TopicMapping::StandardMessage {
                    field_mappings: {
                        let mut mappings = std::collections::HashMap::new();
                        mappings.insert("data".to_string(), "text".to_string());
                        mappings
                    },
                },
            )],
            joint_ids: JointIdMapping::Override(HashMap::new()),
            ..Default::default()
        };

        let hal = Ros2Hal::new(config).await.expect("failed to create HAL");

        // Subscribe to updates: the state is empty, so nothing arrives yet
        let subscription = hal.updates();
        assert!(
            subscription.try_recv().is_none(),
            "No update expected while the state is empty"
        );

        // Create a separate publisher node to simulate external ROS traffic
        let context_options = ros2_client::ContextOptions::new().domain_id(domain_id);
        let pub_ctx = ros2_client::Context::with_options(context_options)
            .expect("failed to create publisher context");

        let pub_node_name = RosNodeName::new("/", &format!("string_publisher_{domain_id}"))
            .expect("valid node name");
        let mut pub_node = pub_ctx
            .new_node(pub_node_name, NodeOptions::new())
            .expect("failed to create publisher node");
        tokio::spawn(pub_node.spinner().unwrap().spin());

        let topic_name = RosName::parse("/speech_status").expect("valid topic name");
        let pub_topic = pub_node
            .create_topic(
                &topic_name,
                msgs::String::message_type_name(),
                &DEFAULT_PUBLISHER_QOS,
            )
            .expect("create publisher topic");
        let publisher = pub_node
            .create_publisher::<msgs::String>(&pub_topic, None)
            .expect("create publisher");
        publisher.wait_for_subscription(&pub_node).await;

        // Publish a String message in a spawned task
        let test_message = "Hello from ROS2!";
        let msg = msgs::String {
            data: test_message.to_string(),
        };
        tokio::spawn(async move {
            loop {
                publisher
                    .async_publish(msg.clone())
                    .await
                    .expect("publish string");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        // Wait for the state change to be received
        let state_change = tokio::task::spawn_blocking(move || subscription.recv())
            .await
            .expect("receiver task panicked")
            .expect("channel closed unexpectedly");

        println!(
            "Received state change with {} keys: {:?}",
            state_change.set.len(),
            state_change.set.keys().collect::<Vec<_>>()
        );

        // Verify we received the expected state change with "text" key (mapped from ROS "data")
        let text_key = Key::from("text".to_string());

        assert!(
            state_change.set.contains_key(&text_key),
            "Expected 'text' in state change, got keys: {:?}",
            state_change.set.keys().collect::<Vec<_>>()
        );

        assert_eq!(
            state_change.set.get(&text_key),
            Some(&Some(Value::from(test_message.to_string()))),
            "'text' should be '{}'",
            test_message
        );

        println!(
            "Successfully received String state change: {:?}",
            state_change
        );
    }

    /// Creates a HAL with a Publish-direction String topic, calls `write()`
    /// with a StateChange, and verifies an external subscriber receives the
    /// correctly converted ROS2 message.
    #[tokio::test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
                  has no unicast-peer/interface config); these run on Linux CI. To run locally, \
                  ensure an active multicast-capable interface and use `--ignored`."
    )]
    async fn test_hal_publish_string_via_write() {
        use rand::Rng;
        use ros2_client::{Name as RosName, NodeName as RosNodeName, DEFAULT_PUBLISHER_QOS};
        use std::time::Duration;

        // Use an isolated domain to avoid interacting with any locally-running ROS graph.
        let domain_id: u16 = rand::rng().random_range(1..=200);

        // Create a config with a Publish-direction String topic
        // field_mappings maps ROS field "data" to the Arora key "text"
        let config = ROS2RobotConfig {
            domain_id: Some(domain_id),
            topics: vec![TopicConfig::new::<msgs::String>(
                "/speech",
                TopicDirection::Publish,
                TopicMapping::StandardMessage {
                    field_mappings: {
                        let mut mappings = std::collections::HashMap::new();
                        mappings.insert("data".to_string(), "text".to_string());
                        mappings
                    },
                },
            )],
            joint_ids: JointIdMapping::Override(HashMap::new()),
            ..Default::default()
        };

        let hal = Ros2Hal::new(config).await.expect("failed to create HAL");

        // Create a separate subscriber node to receive messages from the HAL
        let context_options = ros2_client::ContextOptions::new().domain_id(domain_id);
        let sub_ctx = ros2_client::Context::with_options(context_options)
            .expect("failed to create subscriber context");

        let sub_node_name = RosNodeName::new("/", &format!("string_subscriber_{domain_id}"))
            .expect("valid node name");
        let mut sub_node = sub_ctx
            .new_node(sub_node_name, NodeOptions::new())
            .expect("failed to create subscriber node");
        tokio::spawn(sub_node.spinner().unwrap().spin());

        let topic_name = RosName::parse("/speech").expect("valid topic name");
        // Use DEFAULT_PUBLISHER_QOS for the subscriber topic to match the publisher's QoS
        let sub_topic = sub_node
            .create_topic(
                &topic_name,
                msgs::String::message_type_name(),
                &DEFAULT_PUBLISHER_QOS,
            )
            .expect("create subscriber topic");
        let subscriber = sub_node
            .create_subscription::<msgs::String>(&sub_topic, None)
            .expect("create subscriber");

        // Give time for DDS discovery between subscriber and the HAL's publisher
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Create a state change to publish
        // Use "text" key which will be mapped to ROS "data" field
        let test_message = "Hello from Arora!";
        let mut state_change = StateChange::new();
        state_change.set.insert(
            Key::from("text".to_string()),
            Some(Value::from(test_message.to_string())),
        );

        // Spawn a task to repeatedly publish via the HAL's write method
        tokio::spawn(async move {
            loop {
                let _ = hal.write(state_change.clone()).await;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        // Wait for the message to be received by the external subscriber with 10s timeout
        // Use a loop because async_take returns immediately if no message is available
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let msg = loop {
            if tokio::time::Instant::now() >= deadline {
                panic!("timeout waiting for String message");
            }
            match subscriber.async_take().await {
                Ok((msg, _info)) => break msg,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };

        println!("Received String message: {:?}", msg);

        // Verify the message content
        assert_eq!(
            msg.data, test_message,
            "Message data should be '{}', got: '{}'",
            test_message, msg.data
        );

        println!("Successfully published and received String: {:?}", msg);
    }
}
