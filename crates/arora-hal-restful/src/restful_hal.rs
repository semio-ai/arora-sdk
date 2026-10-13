use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::channel::mpsc::UnboundedSender;
use futures::StreamExt;
use log::{debug, error, info, warn};
use reqwest::Client;
use serde_json::Value as JsonValue;

use arora_hal::{
    target_keys, Hal, HalAssets, HalDescription, HalError, HalResult, KeyDescriptions,
    UpdatesStream,
};
use arora_types::data::{Key, KeyMeta, State, StateChange};
use arora_types::value::{Type, Value};

use crate::config::{EndpointConfig, EndpointMapping, RESTfulRobotConfig};
use crate::conversions::hackerbot;
use crate::restful_error::RESTfulRobotError;

/// The joint names, angles and velocities grouped for one endpoint's request.
type JointGroup = (Vec<String>, Vec<f64>, Vec<f64>);

/// A RESTful API robot as an [`arora_hal::Hal`], driving the robot over HTTP.
///
/// The topology (base URL, endpoints, joint mappings) is fixed at construction
/// from a [`RESTfulRobotConfig`]; the remaining state is interior-mutable so
/// every trait method takes `&self`.
///
/// The robot's API is write-only from the HAL's point of view: writes turn
/// target values into HTTP requests and mirror targets into measured values,
/// so reads and updates reflect the commands sent, not sensor readings.
/// `write()` awaits that path directly; `try_send` hands the changes to the
/// queued-write task, which drives the same path — the caller never waits on
/// an HTTP round-trip.
pub struct RestfulHal {
    // The write path and the state it drives, shared with the queued-write task
    inner: Arc<Inner>,

    // Feeds the queued-write task; dropping it (with the HAL) closes the
    // channel, and the task ends once the remaining backlog is flushed
    outbound: UnboundedSender<StateChange>,

    // The robot's keys, from its configuration: fixed for the HAL's life
    keys: HashMap<Key, KeyMeta>,
}

/// The HAL internals behind both write entries: `write()` calls straight in,
/// the queued-write task serving `try_send` holds its own handle.
struct Inner {
    // Configuration
    config: RESTfulRobotConfig,

    // HTTP client shared by every request
    client: Client,

    // Current state cache, fed by writes
    current_state: Mutex<State>,

    // Observers registered through `updates()`, fed by writes
    subscribers: Mutex<Vec<UnboundedSender<StateChange>>>,
}

impl RestfulHal {
    /// Create a new RestfulHal with the given configuration.
    ///
    /// Validates the configuration, builds the HTTP client, and spawns the
    /// queued-write task serving `try_send`. Must be called within a Tokio
    /// runtime that outlives the HAL (the task and `write()`'s requests run
    /// on it).
    pub fn new(config: RESTfulRobotConfig) -> Result<Self, RESTfulRobotError> {
        config.validate()?;
        let client = Client::builder()
            .user_agent(concat!("arora-restful/", env!("CARGO_PKG_VERSION")))
            .use_preconfigured_tls(crate::tls::webpki_tls_config())
            .build()?;

        info!(
            "Initialized RESTful API robot HAL with base URL: {}",
            config.base_url
        );

        let keys = keys_of(&config);
        let inner = Arc::new(Inner {
            config,
            client,
            current_state: Mutex::new(State::new()),
            subscribers: Mutex::new(Vec::new()),
        });

        // The queued-write task: receives the changes `try_send` enqueues,
        // folds any backlog into one change (newest wins, per key and per target) so slow
        // round-trips coalesce, and drives the same write path as `write()`.
        // It ends when the channel closes, i.e. when the HAL is dropped.
        let (outbound, mut outbound_rx) = futures::channel::mpsc::unbounded::<StateChange>();
        let task_inner = inner.clone();
        tokio::spawn(async move {
            while let Some(mut changes) = outbound_rx.next().await {
                while let Ok(later) = outbound_rx.try_recv() {
                    arora_hal::fold_newest_wins(&mut changes, later);
                }
                if let Err(e) = task_inner.write(changes).await {
                    warn!("Queued write failed: {e}");
                }
            }
        });

        Ok(Self {
            inner,
            outbound,
            keys,
        })
    }
}

/// The keys of the robot `config` describes: for each joint of a POST
/// joint-positions endpoint — the joints a write commands — its position and
/// velocity targets in both forms (`<joint>/target.position`,
/// `<joint>.target_position`, `<joint>/target.velocity`,
/// `<joint>.target_velocity`) as `F64` inputs, and its mirrored
/// `<joint>.position` and `<joint>.velocity` as `F64` outputs. A target
/// carries an `F64` or is not requested, so that is the type the inputs take;
/// positions are in radians, which the robot's conversions turn into its
/// degrees. A velocity target is the speed of the position target written in
/// the same change, and requests nothing on its own.
///
/// A joint's keys are those of a joint at the root, which every client
/// addresses; a name with a `.` or a `/`, or an empty one, makes no such key.
fn keys_of(config: &RESTfulRobotConfig) -> HashMap<Key, KeyMeta> {
    let position = KeyMeta::new().of_type(Type::F64).in_unit("rad");
    let velocity = KeyMeta::new().of_type(Type::F64);
    let position_target = position.clone().editable();
    let velocity_target = velocity
        .clone()
        .editable()
        .described("The speed of the position target written in the same change.");
    let mut keys = HashMap::new();
    let joints = config
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.method == reqwest::Method::POST)
        .filter_map(|endpoint| match &endpoint.mapping {
            Some(EndpointMapping::JointPositions { joint_mapping }) => Some(joint_mapping.keys()),
            _ => None,
        })
        .flatten()
        .filter(|joint| !joint.is_empty() && !joint.contains(['.', '/']));
    for joint in joints {
        for (component, target, measured) in [
            ("position", &position_target, &position),
            ("velocity", &velocity_target, &velocity),
        ] {
            for key in target_keys(joint, component) {
                keys.insert(key, target.clone());
            }
            keys.insert(Key::new(format!("{joint}.{component}")), measured.clone());
        }
    }
    keys
}

impl Inner {
    /// Plan the HTTP requests for the joint-position targets in `changes`.
    ///
    /// A target is a position or velocity target key in either form
    /// ([`arora_hal::set_targets`]) — `<joint>/target.position` or
    /// `<joint>.target_position`, `<joint>/target.velocity` or
    /// `<joint>.target_velocity` — its entity being the joint's name; its
    /// measured value is mirrored under `<joint>.position` or
    /// `<joint>.velocity`. Where `changes` sets both forms of one target, the
    /// `<joint>/target.…` value is the one requested and mirrored.
    ///
    /// Returns the resulting state changes (the targets plus their mirrored
    /// measured values) and one `(url, payload)` request per matching
    /// POST endpoint. Both are empty when the changes carry no joint targets.
    fn plan_joint_position_requests(
        &self,
        changes: &StateChange,
    ) -> (StateChange, Vec<(String, JsonValue)>) {
        // Extract joint positions and velocities from the keys
        let mut joint_positions: HashMap<String, f64> = HashMap::new();
        let mut joint_velocities: HashMap<String, f64> = HashMap::new();
        let mut result_changes = StateChange::new();
        for target in arora_hal::set_targets(changes) {
            let Some(Value::F64(value)) = target.value else {
                continue;
            };
            let joints = match target.component() {
                "position" => &mut joint_positions,
                "velocity" => &mut joint_velocities,
                _ => continue,
            };
            joints.insert(target.entity().to_string(), *value);
            result_changes
                .set
                .insert(target.key.clone(), Some(Value::F64(*value)));
            result_changes
                .set
                .insert(target.measured, Some(Value::F64(*value)));
        }

        if joint_positions.is_empty() {
            return (StateChange::new(), Vec::new());
        }

        // Default velocities if not specified
        for joint_name in joint_positions.keys() {
            if !joint_velocities.contains_key(joint_name) {
                joint_velocities.insert(joint_name.clone(), 70.0);
            }
        }

        // Group joints by endpoint
        let mut endpoint_to_joints: HashMap<String, JointGroup> = HashMap::new();

        let joint_pos_endpoints: Vec<_> = self
            .config
            .endpoints
            .iter()
            .filter(|e| {
                matches!(e.mapping, Some(EndpointMapping::JointPositions { .. }))
                    && e.method == reqwest::Method::POST
            })
            .collect();

        for (joint_name, angle) in joint_positions {
            for endpoint in &joint_pos_endpoints {
                if let Some(EndpointMapping::JointPositions { joint_mapping }) = &endpoint.mapping {
                    if joint_mapping.contains_key(&joint_name) {
                        let velocity = joint_velocities.get(&joint_name).copied().unwrap_or(70.0);
                        let entry = endpoint_to_joints.entry(endpoint.path.clone()).or_default();
                        entry.0.push(joint_name.clone());
                        entry.1.push(angle);
                        entry.2.push(velocity);
                        break;
                    }
                }
            }
        }

        // Convert each endpoint's joints to a request
        let mut requests = Vec::new();
        for (endpoint_path, (joints, angles, velocities)) in endpoint_to_joints {
            if let Some(endpoint) = self.find_endpoint_by_path(&endpoint_path) {
                if let Some(payload) = hackerbot::convert_joint_positions_to_request(
                    &joints,
                    &angles,
                    &velocities,
                    endpoint,
                ) {
                    requests.push((self.build_url(&endpoint.path), payload));
                }
            }
        }

        (result_changes, requests)
    }

    /// Send one planned POST request, mapping any HTTP failure to an error.
    async fn send_request(&self, url: &str, payload: &JsonValue) -> Result<(), RESTfulRobotError> {
        debug!(
            "Sending POST request to {} with payload: {:?}",
            url, payload
        );
        match self.client.post(url).json(payload).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    Err(RESTfulRobotError::RequestError(format!(
                        "API request to {} failed with status {}: {}",
                        url, status, text
                    )))
                }
            }
            Err(e) => Err(RESTfulRobotError::RequestError(format!(
                "Failed to send API request to {}: {:?}",
                url, e
            ))),
        }
    }

    /// Notify all subscribers of state changes, dropping the disconnected ones.
    fn notify_subscribers(&self, changes: &StateChange) {
        if changes.is_empty() {
            return;
        }
        let mut subscribers = self.subscribers.lock().expect("subscribers lock poisoned");
        subscribers.retain(|tx| tx.unbounded_send(changes.clone()).is_ok());
    }

    /// Builds the full URL for an endpoint path.
    fn build_url(&self, endpoint_path: &str) -> String {
        format!(
            "{}{}",
            self.config.base_url,
            crate::utils::normalize_endpoint_path(endpoint_path)
        )
    }

    /// Finds an endpoint configuration by its path.
    fn find_endpoint_by_path(&self, path: &str) -> Option<&EndpointConfig> {
        self.config.endpoints.iter().find(|e| e.path == path)
    }

    /// The write path shared by [`Hal::write`] and the queued-write task.
    async fn write(&self, changes: StateChange) -> HalResult<()> {
        debug!(
            "Received write with state changes: {} set, {} unset",
            changes.set.len(),
            changes.unset.len()
        );

        let (result_changes, requests) = self.plan_joint_position_requests(&changes);

        // Update the local state cache: the changes themselves, plus the
        // mirrored measured values.
        {
            let mut state = self.current_state.lock().expect("state lock poisoned");
            state.apply(changes);
            state.apply(result_changes.clone());
        }

        let mut failures: Vec<String> = Vec::new();
        let mut published = 0usize;
        for (url, payload) in &requests {
            match self.send_request(url, payload).await {
                Ok(()) => {
                    debug!("Successfully sent request to '{}'", url);
                    published += 1;
                }
                Err(e) => {
                    error!("{e}");
                    failures.push(e.to_string());
                }
            }
        }

        if failures.is_empty() || published > 0 {
            for failure in &failures {
                warn!("Partial write failure: {failure}");
            }
            // The robot accepted (some of) the commands: observers see the
            // resulting changes, faked measured values included.
            self.notify_subscribers(&result_changes);
            Ok(())
        } else {
            Err(HalError::Other(format!(
                "write failed on every matching endpoint: {}",
                failures.join("; ")
            )))
        }
    }
}

#[async_trait]
impl Hal for RestfulHal {
    /// Describe the device from the robot configuration.
    async fn describe(&self) -> HalDescription {
        debug!("describe called.");
        HalDescription {
            model_family: self.inner.config.model_family.clone(),
            hardware_version: self.inner.config.hardware_version.clone(),
            software_version: self.inner.config.software_version.clone(),
        }
    }

    /// Retrieves the current values for the given keys from the local state
    /// cache. Absent keys read as `None`.
    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
        debug!("Received read request for {} keys", keys.len());
        let state = self
            .inner
            .current_state
            .lock()
            .expect("state lock poisoned");
        Ok(keys
            .iter()
            .map(|key| state.get(key).cloned().flatten())
            .collect())
    }

    /// Retrieves all key/value pairs currently held in the local state cache.
    async fn read_all(&self) -> HalResult<State> {
        debug!("Received read_all request");
        let state = self
            .inner
            .current_state
            .lock()
            .expect("state lock poisoned");
        debug!(
            "Returning all {} key-value pairs from local state",
            state.storage.len()
        );
        Ok(state.clone())
    }

    /// Applies actuator/state changes to the robot.
    ///
    /// Caches the changes, POSTs the joint-position targets to every matching
    /// API endpoint, and mirrors the targets into measured values (the API
    /// reports no readings) — observers see the targets plus the mirrored
    /// values. Per-endpoint failures are logged as warnings; the call only
    /// errors when every matching endpoint failed.
    async fn write(&self, changes: StateChange) -> HalResult<()> {
        self.inner.write(changes).await
    }

    /// Enqueues the changes onto the queued-write task and returns; the task
    /// folds the backlog (newest wins, per key and per target) and drives the same path as
    /// [`write`](Hal::write). Never waits on the HTTP round-trip.
    fn try_send(&self, changes: &StateChange) {
        if changes.is_empty() {
            return;
        }
        if self.outbound.unbounded_send(changes.clone()).is_err() {
            warn!("Dropping a state change: the queued-write task is gone");
        }
    }

    /// The robot's keys, from its configuration ([`keys_of`]): one
    /// description, as they are fixed for the HAL's life.
    fn describe_keys(&self) -> KeyDescriptions {
        Box::pin(futures::stream::iter([self.keys.clone()]))
    }

    /// A feed of the changes the robot reports.
    ///
    /// Each feed immediately receives the current state as one `StateChange`
    /// (when non-empty), then every subsequent change.
    fn updates(&self) -> UpdatesStream {
        debug!("Received updates request");
        let (tx, rx) = futures::channel::mpsc::unbounded();

        let snapshot = self
            .inner
            .current_state
            .lock()
            .expect("state lock poisoned")
            .clone();
        if !snapshot.is_empty() {
            let _ = tx.unbounded_send(StateChange {
                set: snapshot.storage,
                unset: Default::default(),
            });
        }

        self.inner
            .subscribers
            .lock()
            .expect("subscribers lock poisoned")
            .push(tx);
        Box::pin(rx)
    }
}

/// RESTful robot configurations carry no model; the HAL states none.
impl HalAssets for RestfulHal {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    use futures::{FutureExt, StreamExt};
    use reqwest::Method;

    use super::*;

    /// Drain everything the feed has already buffered, without blocking.
    fn drain(feed: &mut UpdatesStream) -> Vec<StateChange> {
        let mut out = Vec::new();
        while let Some(Some(change)) = feed.next().now_or_never() {
            out.push(change);
        }
        out
    }

    fn head_config(base_url: &str) -> RESTfulRobotConfig {
        RESTfulRobotConfig {
            model_family: Some("hackerbot".to_string()),
            hardware_version: Some("v1".to_string()),
            base_url: base_url.to_string(),
            endpoints: vec![EndpointConfig {
                path: "/api/v1/head".to_string(),
                method: Method::POST,
                mapping: Some(EndpointMapping::JointPositions {
                    joint_mapping: HashMap::from([
                        ("head_yaw".to_string(), "yaw".to_string()),
                        ("head_pitch".to_string(), "pitch".to_string()),
                    ]),
                }),
            }],
            ..Default::default()
        }
    }

    /// A minimal HTTP server on a std thread: answers each request with the
    /// status `status_for` picks from the path, and reports each
    /// `(path, JSON body)` on a channel.
    fn spawn_http_server(
        status_for: fn(&str) -> u16,
    ) -> (String, std::sync::mpsc::Receiver<(String, JsonValue)>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("test server address");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let mut reader = BufReader::new(stream);

                // Request line: "POST /api/v1/head HTTP/1.1"
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();

                // Headers, keeping content-length
                let mut content_length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_length = value.trim().parse().unwrap_or(0);
                    }
                }

                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).is_err() {
                    continue;
                }
                let json = serde_json::from_slice(&body).unwrap_or(JsonValue::Null);
                let status = status_for(&path);
                let _ = tx.send((path, json));

                let response = format!(
                    "HTTP/1.1 {status} Whatever\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                let _ = reader.get_mut().write_all(response.as_bytes());
            }
        });
        (format!("http://{addr}"), rx)
    }

    /// The keys a write reads and the HAL reports: each joint of a POST
    /// joint-positions endpoint, and no other.
    #[tokio::test]
    async fn test_describes_each_commanded_joint_s_keys() {
        let mut config = head_config("http://localhost:1");
        config.endpoints.push(EndpointConfig {
            path: "/api/v1/arm".to_string(),
            method: Method::GET,
            mapping: Some(EndpointMapping::JointPositions {
                joint_mapping: HashMap::from([("elbow".to_string(), "elbow".to_string())]),
            }),
        });
        if let Some(EndpointMapping::JointPositions { joint_mapping }) =
            &mut config.endpoints[0].mapping
        {
            joint_mapping.insert("not.a.joint".to_string(), "x".to_string());
            joint_mapping.insert("arm/elbow".to_string(), "x".to_string());
        }
        let hal = RestfulHal::new(config).expect("create HAL");
        let mut feed = hal.describe_keys();
        let description = feed.next().await.expect("one description");
        assert_eq!(feed.next().await, None, "the keys are fixed");

        let position = KeyMeta::new().of_type(Type::F64).in_unit("rad");
        let velocity = KeyMeta::new().of_type(Type::F64);
        let speed = "The speed of the position target written in the same change.";
        let mut expected = HashMap::new();
        for joint in ["head_yaw", "head_pitch"] {
            for (component, measured, target) in [
                ("position", &position, position.clone().editable()),
                (
                    "velocity",
                    &velocity,
                    velocity.clone().editable().described(speed),
                ),
            ] {
                expected.insert(
                    Key::from(format!("{joint}/target.{component}")),
                    target.clone(),
                );
                expected.insert(Key::from(format!("{joint}.target_{component}")), target);
                expected.insert(Key::from(format!("{joint}.{component}")), measured.clone());
            }
        }
        assert_eq!(description, expected);
    }

    /// Through an Arora device: the device lists the HAL's keys with their
    /// meta, takes a remote write to an input and sends it to the robot, and
    /// refuses one to an output or to a key the HAL does not describe.
    #[tokio::test]
    async fn test_a_device_opens_the_described_inputs() {
        let (base_url, requests) = spawn_http_server(|_| 200);
        let hal = RestfulHal::new(head_config(&base_url)).expect("create HAL");
        let mut device = arora::Arora::builder()
            .with_hal(Box::new(hal))
            .build()
            .expect("build the device");
        let caller = device.caller();

        let keys = settle(&mut device, caller.list_keys(Some("head_yaw".to_string())))
            .await
            .expect("list the keys");
        let listed: Vec<(&str, bool, Option<Type>)> = keys
            .iter()
            .map(|(path, meta)| (path.as_str(), meta.editable, meta.ty.clone()))
            .collect();
        assert_eq!(
            listed,
            vec![
                ("head_yaw.position", false, Some(Type::F64)),
                ("head_yaw.target_position", true, Some(Type::F64)),
                ("head_yaw.target_velocity", true, Some(Type::F64)),
                ("head_yaw.velocity", false, Some(Type::F64)),
                ("head_yaw/target.position", true, Some(Type::F64)),
                ("head_yaw/target.velocity", true, Some(Type::F64)),
            ]
        );

        settle(
            &mut device,
            caller.update(StateChange::set(
                "head_yaw/target.position",
                Value::F64(0.5),
            )),
        )
        .await
        .expect("an input");
        let (path, body) = tokio::task::spawn_blocking(move || {
            requests
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the robot is asked")
        })
        .await
        .expect("join");
        assert_eq!(path, "/api/v1/head");
        assert_eq!(body["yaw"], serde_json::json!(0.5f64.to_degrees() + 180.0));

        for (key, value, why) in [
            ("head_yaw.position", Value::F64(0.5), "not an input"),
            ("head_roll.target_position", Value::F64(0.5), "not an input"),
            ("head_yaw.target_position", Value::I64(1), "declared type"),
        ] {
            let refused = settle(&mut device, caller.update(StateChange::set(key, value)))
                .await
                .expect_err("refused");
            assert!(refused.to_string().contains(why), "{key}: {refused}");
        }
    }

    /// Step `device` until `reply` is answered.
    async fn settle<T>(
        device: &mut arora::Arora,
        reply: impl std::future::Future<Output = T>,
    ) -> T {
        let mut reply = std::pin::pin!(reply);
        for _ in 0..8 {
            device
                .step(std::time::Duration::from_millis(10))
                .expect("step");
            if let Some(answer) = reply.as_mut().now_or_never() {
                return answer;
            }
        }
        panic!("no answer after eight steps");
    }

    #[tokio::test]
    async fn test_describe_reports_config_fields() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");
        let description = hal.describe().await;
        assert_eq!(description.model_family.as_deref(), Some("hackerbot"));
        assert_eq!(description.hardware_version.as_deref(), Some("v1"));
        assert_eq!(description.software_version, None);
    }

    #[tokio::test]
    async fn test_states_no_model() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");
        assert!(hal.models().is_empty());
        assert_eq!(hal.servable_glb(arora_hal::DEVICE).unwrap(), None);
    }

    #[test]
    fn test_invalid_config_fails_construction() {
        let mut config = head_config("http://localhost:1");
        config.base_url = String::new();
        assert!(matches!(
            RestfulHal::new(config),
            Err(RESTfulRobotError::ConfigError(_))
        ));
    }

    /// A write with no joint targets issues no request; the values land in the
    /// state cache and read/read_all return them.
    #[tokio::test]
    async fn test_write_caches_state_for_read() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");

        hal.write(StateChange::set("text", Value::from("hello".to_string())))
            .await
            .expect("a write without joint targets should succeed offline");

        assert_eq!(
            hal.read(&[Key::from("text")]).await.unwrap(),
            vec![Some(Value::from("hello".to_string()))]
        );
        let all = hal.read_all().await.unwrap();
        assert_eq!(
            all.get(&Key::from("text")),
            Some(&Some(Value::from("hello".to_string())))
        );

        // An unset removes the key from the cache.
        let mut unset = StateChange::new();
        unset.unset.insert(Key::from("text"));
        hal.write(unset).await.unwrap();
        assert_eq!(hal.read(&[Key::from("text")]).await.unwrap(), vec![None]);
    }

    /// A joint-target write POSTs the converted payload to the matching
    /// endpoint, mirrors the target into the measured position, and notifies
    /// subscribers.
    #[tokio::test]
    async fn test_write_joint_targets_posts_and_mirrors() {
        let (base_url, requests) = spawn_http_server(|_| 200);
        let hal = RestfulHal::new(head_config(&base_url)).expect("create HAL");
        let mut feed = hal.updates();
        assert!(
            drain(&mut feed).is_empty(),
            "No update expected while the state is empty"
        );

        hal.write(StateChange::set(
            "head_yaw.target_position",
            Value::from(0.5),
        ))
        .await
        .expect("write should succeed");

        // The endpoint received the converted hackerbot payload.
        let (path, payload) = requests.try_recv().expect("one request expected");
        assert_eq!(path, "/api/v1/head");
        assert_eq!(payload["method"], serde_json::json!("look"));
        let yaw = payload["yaw"].as_f64().expect("yaw should be a number");
        assert!((yaw - (0.5f64.to_degrees() + 180.0)).abs() < 1e-9);

        // The cache holds the target and the mirrored measured position.
        assert_eq!(
            hal.read(&[
                Key::from("head_yaw.target_position"),
                Key::from("head_yaw.position")
            ])
            .await
            .unwrap(),
            vec![Some(Value::from(0.5)), Some(Value::from(0.5))]
        );

        // The subscriber saw the target and the mirrored position.
        let change = feed
            .next()
            .now_or_never()
            .flatten()
            .expect("a state change expected");
        assert_eq!(
            change.set.get(&Key::from("head_yaw.position")),
            Some(&Some(Value::from(0.5)))
        );
        assert_eq!(
            change.set.get(&Key::from("head_yaw.target_position")),
            Some(&Some(Value::from(0.5)))
        );
    }

    /// The `<joint>/target.…` keys request and mirror as the
    /// `<joint>.target_…` keys do.
    #[tokio::test]
    async fn test_write_entity_form_targets_posts_and_mirrors() {
        let (base_url, requests) = spawn_http_server(|_| 200);
        let hal = RestfulHal::new(head_config(&base_url)).expect("create HAL");
        let mut feed = hal.updates();

        let mut changes = StateChange::set("head_yaw/target.position", Value::from(0.5));
        changes.set.insert(
            Key::from("head_yaw/target.velocity"),
            Some(Value::from(2.0)),
        );
        hal.write(changes).await.expect("write should succeed");

        let (path, payload) = requests.try_recv().expect("one request expected");
        assert_eq!(path, "/api/v1/head");
        let yaw = payload["yaw"].as_f64().expect("yaw should be a number");
        assert!((yaw - (0.5f64.to_degrees() + 180.0)).abs() < 1e-9);
        assert_eq!(payload["speed"].as_f64(), Some(2.0 * 75.0));

        let change = feed
            .next()
            .now_or_never()
            .flatten()
            .expect("a state change expected");
        for (key, value) in [
            ("head_yaw/target.position", 0.5),
            ("head_yaw.position", 0.5),
            ("head_yaw/target.velocity", 2.0),
            ("head_yaw.velocity", 2.0),
        ] {
            assert_eq!(
                change.set.get(&Key::from(key)),
                Some(&Some(Value::from(value))),
                "{key}"
            );
        }
        assert_eq!(change.set.len(), 4);
    }

    /// A change that sets both forms of one target requests and mirrors the
    /// `<joint>/target.…` value.
    #[tokio::test]
    async fn test_write_both_forms_applies_the_entity_form() {
        let (base_url, requests) = spawn_http_server(|_| 200);
        let hal = RestfulHal::new(head_config(&base_url)).expect("create HAL");
        let mut feed = hal.updates();

        let mut changes = StateChange::new();
        for (key, value) in [
            ("head_yaw.target_position", 0.2),
            ("head_yaw/target.position", 0.5),
            ("head_yaw.target_velocity", 1.0),
            ("head_yaw/target.velocity", 2.0),
        ] {
            changes.set.insert(Key::from(key), Some(Value::from(value)));
        }
        hal.write(changes).await.expect("write should succeed");

        let (_, payload) = requests.try_recv().expect("one request expected");
        let yaw = payload["yaw"].as_f64().expect("yaw should be a number");
        assert!((yaw - (0.5f64.to_degrees() + 180.0)).abs() < 1e-9);
        assert_eq!(payload["speed"].as_f64(), Some(2.0 * 75.0));
        assert!(requests.try_recv().is_err(), "one request only");

        let change = feed
            .next()
            .now_or_never()
            .flatten()
            .expect("a state change expected");
        assert_eq!(
            change.set.get(&Key::from("head_yaw.position")),
            Some(&Some(Value::from(0.5)))
        );
        assert_eq!(
            change.set.get(&Key::from("head_yaw.velocity")),
            Some(&Some(Value::from(2.0)))
        );
        assert_eq!(change.set.get(&Key::from("head_yaw.target_position")), None);
        assert_eq!(
            hal.read(&[Key::from("head_yaw.position")]).await.unwrap(),
            vec![Some(Value::from(0.5))]
        );
    }

    /// `try_send` returns without waiting: the queued-write task POSTs the
    /// change and notifies subscribers, same as `write`.
    #[tokio::test]
    async fn test_try_send_posts_and_mirrors() {
        use std::time::Duration;

        let (base_url, requests) = spawn_http_server(|_| 200);
        let hal = RestfulHal::new(head_config(&base_url)).expect("create HAL");
        let mut feed = hal.updates();

        hal.try_send(&StateChange::set(
            "head_yaw.target_position",
            Value::from(0.5),
        ));

        // The endpoint receives the converted payload once the task flushes.
        let (path, payload) =
            tokio::task::spawn_blocking(move || requests.recv_timeout(Duration::from_secs(10)))
                .await
                .expect("receiving thread panicked")
                .expect("one request expected");
        assert_eq!(path, "/api/v1/head");
        assert_eq!(payload["method"], serde_json::json!("look"));

        // The subscriber sees the target and the mirrored measured position.
        let change = tokio::time::timeout(Duration::from_secs(10), feed.next())
            .await
            .expect("a state change expected")
            .expect("feed closed unexpectedly");
        assert_eq!(
            change.set.get(&Key::from("head_yaw.target_position")),
            Some(&Some(Value::from(0.5)))
        );
        assert_eq!(
            change.set.get(&Key::from("head_yaw.position")),
            Some(&Some(Value::from(0.5)))
        );
    }

    /// When every matching endpoint fails, write errors; the cache still holds
    /// the changes (they were accepted locally before the requests).
    #[tokio::test]
    async fn test_write_all_endpoints_failing_errors() {
        // A bound-then-dropped listener yields a port that refuses connections.
        let dead_url = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", listener.local_addr().unwrap())
        };
        let hal = RestfulHal::new(head_config(&dead_url)).expect("create HAL");
        let mut feed = hal.updates();

        let result = hal
            .write(StateChange::set(
                "head_yaw.target_position",
                Value::from(0.5),
            ))
            .await;
        assert!(matches!(result, Err(HalError::Other(_))));

        // Subscribers were not notified of a command the robot never took.
        assert!(drain(&mut feed).is_empty());

        // The cache was updated regardless.
        assert_eq!(
            hal.read(&[Key::from("head_yaw.target_position")])
                .await
                .unwrap(),
            vec![Some(Value::from(0.5))]
        );
    }

    /// A non-success HTTP status counts as an endpoint failure.
    #[tokio::test]
    async fn test_write_error_status_errors() {
        let (base_url, _requests) = spawn_http_server(|_| 500);
        let hal = RestfulHal::new(head_config(&base_url)).expect("create HAL");

        let result = hal
            .write(StateChange::set(
                "head_yaw.target_position",
                Value::from(0.5),
            ))
            .await;
        assert!(matches!(result, Err(HalError::Other(_))));
    }

    /// When one endpoint succeeds and another fails, the write succeeds and
    /// the failure is only logged.
    #[tokio::test]
    async fn test_write_partial_failure_is_ok() {
        // The head endpoint accepts, the arm endpoint errors.
        let (base_url, requests) =
            spawn_http_server(|path| if path.ends_with("/head") { 200 } else { 500 });

        let mut config = head_config(&base_url);
        config.endpoints.push(EndpointConfig {
            path: "/api/v1/arm".to_string(),
            method: Method::POST,
            mapping: Some(EndpointMapping::JointPositions {
                joint_mapping: HashMap::from([("joint1".to_string(), "joint1".to_string())]),
            }),
        });
        let hal = RestfulHal::new(config).expect("create HAL");

        let mut changes = StateChange::new();
        changes.set.insert(
            Key::from("head_yaw.target_position"),
            Some(Value::from(0.5)),
        );
        changes
            .set
            .insert(Key::from("joint1.target_position"), Some(Value::from(0.1)));

        hal.write(changes)
            .await
            .expect("a partially-failing write should succeed");

        // Both endpoints were hit, in either order.
        let mut paths = vec![
            requests.try_recv().expect("first request expected").0,
            requests.try_recv().expect("second request expected").0,
        ];
        paths.sort();
        assert_eq!(paths, ["/api/v1/arm", "/api/v1/head"]);
    }

    /// Each new subscription first receives the current state when non-empty.
    #[tokio::test]
    async fn test_updates_first_message_is_current_state() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");

        // Empty state: no first message.
        let mut early = hal.updates();
        assert!(drain(&mut early).is_empty());

        hal.write(StateChange::set("text", Value::from("hello".to_string())))
            .await
            .unwrap();
        // A write without joint targets notifies no subscriber: the robot's
        // API took no command, so there is nothing the hardware reports.
        assert!(drain(&mut early).is_empty());

        // Non-empty state: the snapshot arrives first.
        let mut late = hal.updates();
        let first = late
            .next()
            .now_or_never()
            .flatten()
            .expect("current state expected first");
        assert_eq!(
            first.set.get(&Key::from("text")),
            Some(&Some(Value::from("hello".to_string())))
        );
        assert!(first.unset.is_empty());
    }
}
