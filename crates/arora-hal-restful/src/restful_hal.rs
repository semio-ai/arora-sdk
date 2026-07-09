use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::channel::mpsc::UnboundedSender;
use futures::StreamExt;
use log::{debug, error, info, warn};
use reqwest::Client;
use serde_json::Value as JsonValue;

use arora_hal::{Hal, HalAssets, HalDescription, HalError, HalResult, UpdatesStream};
use arora_types::data::{Key, State, StateChange};
use arora_types::value::Value;

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

/// Fold `later` into `earlier`, per-key newest wins: applying the folded
/// change equals applying both in order.
fn fold_newest_wins(earlier: &mut StateChange, later: StateChange) {
    for key in later.unset {
        earlier.set.remove(&key);
        earlier.unset.insert(key);
    }
    for (key, value) in later.set {
        earlier.unset.remove(&key);
        earlier.set.insert(key, value);
    }
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

        let inner = Arc::new(Inner {
            config,
            client,
            current_state: Mutex::new(State::new()),
            subscribers: Mutex::new(Vec::new()),
        });

        // The queued-write task: receives the changes `try_send` enqueues,
        // folds any backlog into one change (per-key newest wins) so slow
        // round-trips coalesce, and drives the same write path as `write()`.
        // It ends when the channel closes, i.e. when the HAL is dropped.
        let (outbound, mut outbound_rx) = futures::channel::mpsc::unbounded::<StateChange>();
        let task_inner = inner.clone();
        tokio::spawn(async move {
            while let Some(mut changes) = outbound_rx.next().await {
                while let Ok(later) = outbound_rx.try_recv() {
                    fold_newest_wins(&mut changes, later);
                }
                if let Err(e) = task_inner.write(changes).await {
                    warn!("Queued write failed: {e}");
                }
            }
        });

        Ok(Self { inner, outbound })
    }
}

impl Inner {
    /// Plan the HTTP requests for the joint-position targets in `changes`.
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
        for (key, value) in &changes.set {
            let Some(value) = value else { continue };
            if key.get_component() == Some("target_position") {
                if let Value::F64(angle) = value {
                    let joint_name = key.get_entity();
                    joint_positions.insert(joint_name.to_string(), *angle);
                    result_changes
                        .set
                        .insert(key.clone(), Some(Value::F64(*angle)));
                    result_changes.set.insert(
                        key.clone().with_component("position"),
                        Some(Value::F64(*angle)),
                    );
                }
            } else if key.get_component() == Some("target_velocity") {
                if let Value::F64(velocity) = value {
                    let joint_name = key.get_entity();
                    joint_velocities.insert(joint_name.to_string(), *velocity);
                    result_changes
                        .set
                        .insert(key.clone(), Some(Value::F64(*velocity)));
                    result_changes.set.insert(
                        key.clone().with_component("velocity"),
                        Some(Value::F64(*velocity)),
                    );
                }
            }
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
    /// folds the backlog (per-key newest wins) and drives the same path as
    /// [`write`](Hal::write). Never waits on the HTTP round-trip.
    fn try_send(&self, changes: &StateChange) {
        if changes.is_empty() {
            return;
        }
        if self.outbound.unbounded_send(changes.clone()).is_err() {
            warn!("Dropping a state change: the queued-write task is gone");
        }
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

#[async_trait]
impl HalAssets for RestfulHal {
    /// RESTful robot configurations carry no GLB model; the HAL reports none.
    async fn model_glb(&self) -> HalResult<Option<Vec<u8>>> {
        debug!("model_glb called.");
        Ok(None)
    }
}

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

    #[tokio::test]
    async fn test_describe_reports_config_fields() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");
        let description = hal.describe().await;
        assert_eq!(description.model_family.as_deref(), Some("hackerbot"));
        assert_eq!(description.hardware_version.as_deref(), Some("v1"));
        assert_eq!(description.software_version, None);
    }

    #[tokio::test]
    async fn test_model_glb_is_none() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");
        assert_eq!(hal.model_glb().await.unwrap(), None);
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

    #[tokio::test]
    async fn test_read_absent_is_none() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");
        assert_eq!(hal.read(&[Key::from("nope")]).await.unwrap(), vec![None]);
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

    /// Writes without joint targets don't notify subscribers: the robot's API
    /// took no command, so there is nothing the hardware reports.
    #[tokio::test]
    async fn test_non_joint_write_does_not_notify() {
        let hal = RestfulHal::new(head_config("http://localhost:1")).expect("create HAL");
        let mut feed = hal.updates();

        hal.write(StateChange::set("text", Value::from("hello".to_string())))
            .await
            .unwrap();

        assert!(drain(&mut feed).is_empty());
    }
}
