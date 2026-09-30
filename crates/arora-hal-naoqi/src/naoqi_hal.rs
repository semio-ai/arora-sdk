use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arora_hal::{Hal, HalDescription, HalResult, UpdatesStream};
use arora_types::data::{Key, State, StateChange};
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::mpsc::UnboundedSender;
use futures::StreamExt;
use log::{debug, info, warn};
use qi::value::{Dynamic, KeyDynValueMap, Value as QiValue};
use qi::{AnyObject, ObjectExt};
use tokio::task::AbortHandle;

use crate::config::NaoqiRobotConfig;
use crate::conversions::{arora_f64, keys, qi_f64, qi_list, JointTable};
use crate::naoqi_error::NaoqiRobotError;

type Node = qi::node::Node<qi::service_directory::Client>;

/// A NAOqi robot (NAO, Pepper) as an [`arora_hal::Hal`], driven through the `qi` framework.
///
/// The robot's topology (joints, available services) is discovered at construction; the
/// remaining state is interior-mutable so every trait method takes `&self`.
///
/// Sensors are sampled from `ALMemory` by a task at the configured period and touch events
/// are received through `ALMemory` signals; both feed the current state and the `updates()`
/// subscribers. Writes turn keys into NAOqi calls (`ALMotion.setAngles`,
/// `ALTextToSpeech.say`, `ALLeds.fadeRGB`...): `write()` awaits them, `try_send` hands them
/// to the queued-write task so the caller never waits on the robot.
pub struct NaoqiHal {
    inner: Arc<Inner>,
    /// Feeds the queued-write task; dropping it (with the HAL) closes the channel, and the
    /// task ends once the remaining backlog is flushed.
    outbound: UnboundedSender<StateChange>,
    /// The sampling and event tasks, aborted with the HAL.
    tasks: Vec<AbortHandle>,
}

struct Inner {
    config: NaoqiRobotConfig,
    /// The node connected to the robot; the sessions to its services live as long as it does.
    _node: Node,
    memory: AnyObject,
    motion: AnyObject,
    tts: AnyObject,
    leds: Option<AnyObject>,
    joints: JointTable,
    description: HalDescription,
    /// The current state: sampled sensors, received events and cached commands.
    current_state: Mutex<State>,
    /// Observers registered through `updates()`.
    subscribers: Mutex<Vec<UnboundedSender<StateChange>>>,
    /// The last base velocity command `(x, y, theta)`, merged across its keys.
    base_velocity: Mutex<[f64; 3]>,
}

/// What the sensor sampling keeps between two samples: the previous joint positions, to
/// derive velocities.
#[derive(Default)]
struct Sampling {
    previous_positions: Option<(Instant, Vec<(String, f64)>)>,
}

/// Fold `later` into `earlier`, per-key newest wins: applying the folded change equals
/// applying both in order.
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

async fn required_service(node: &Node, name: &str) -> Result<AnyObject, NaoqiRobotError> {
    node.service(name).await.map_err(|error| {
        NaoqiRobotError::Connection(format!(
            "the NAOqi service {name} is not available: {error}"
        ))
    })
}

impl NaoqiHal {
    /// Connects to the robot described by the configuration and starts driving it.
    ///
    /// Validates the configuration, connects to NAOqi, discovers the joints and the
    /// services, takes a first sample of the sensors and spawns the sampling, event and
    /// queued-write tasks. Must be called within a Tokio runtime that outlives the HAL.
    pub async fn new(config: NaoqiRobotConfig) -> Result<Self, NaoqiRobotError> {
        config.validate()?;
        let address: qi::Address = config.url.parse().map_err(|error| {
            NaoqiRobotError::Connection(format!("invalid address {}: {error}", config.url))
        })?;
        let credentials = config.password.as_ref().map(|password| {
            let mut credentials = KeyDynValueMap::new();
            credentials.set(
                "auth_user",
                config.user.clone().unwrap_or_else(|| "nao".to_string()),
            );
            credentials.set("auth_token", password.clone());
            credentials
        });
        let node = qi::node::init()
            .connect_to_space(address, credentials)
            .start()
            .await
            .map_err(|error| {
                NaoqiRobotError::Connection(format!("cannot connect to {}: {error}", config.url))
            })?;

        let memory = required_service(&node, "ALMemory").await?;
        let motion = required_service(&node, "ALMotion").await?;
        let tts = required_service(&node, "ALTextToSpeech").await?;
        let leds = match node.service("ALLeds").await {
            Ok(leds) => Some(leds),
            Err(error) => {
                warn!("ALLeds is not available, LED keys are ignored: {error}");
                None
            }
        };
        let description = describe(&config, &node, &memory).await;
        let names: Vec<String> = motion
            .call("getBodyNames", "Body".to_string())
            .await
            .map_err(|error| {
                NaoqiRobotError::Connection(format!("cannot list the joints: {error}"))
            })?;
        let joints = JointTable::new(names, &config.joint_ids);
        info!(
            "Connected to NAOqi at {} ({:?}, {} joints)",
            config.url,
            description
                .model_family
                .as_deref()
                .unwrap_or("unknown robot"),
            joints.len()
        );

        let inner = Arc::new(Inner {
            config,
            _node: node,
            memory,
            motion,
            tts,
            leds,
            joints,
            description,
            current_state: Mutex::new(State::new()),
            subscribers: Mutex::new(Vec::new()),
            base_velocity: Mutex::new([0.0; 3]),
        });

        // A first sample, so that the first `updates()` subscribers get a snapshot.
        let mut sampling = Sampling::default();
        if let Err(error) = inner.sample_sensors(&mut sampling).await {
            warn!("Initial sensor sampling failed: {error}");
        }

        let mut tasks = Vec::new();
        // The sensor sampling task.
        let period = Duration::from_millis(inner.config.sensor_period_ms);
        let sampler = Arc::clone(&inner);
        tasks.push(
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(period);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    if let Err(error) = sampler.sample_sensors(&mut sampling).await {
                        warn!("Sensor sampling failed: {error}");
                    }
                }
            })
            .abort_handle(),
        );
        // The touch event tasks.
        if inner.config.sensors.touch {
            for (event, key) in keys::TOUCH_EVENTS {
                let watcher = Arc::clone(&inner);
                tasks.push(tokio::spawn(watcher.watch_event(event, key)).abort_handle());
            }
        }
        // The queued-write task: receives the changes `try_send` enqueues, folds any backlog
        // into one change (per-key newest wins) so slow round-trips coalesce, and drives the
        // same write path as `write()`. It ends when the channel closes, i.e. when the HAL is
        // dropped.
        let (outbound, mut outbound_rx) = futures::channel::mpsc::unbounded::<StateChange>();
        let writer = Arc::clone(&inner);
        tokio::spawn(async move {
            while let Some(mut changes) = outbound_rx.next().await {
                while let Ok(later) = outbound_rx.try_recv() {
                    fold_newest_wins(&mut changes, later);
                }
                if let Err(error) = writer.write(changes).await {
                    warn!("Queued write failed: {error}");
                }
            }
        });

        Ok(Self {
            inner,
            outbound,
            tasks,
        })
    }
}

/// The HAL description, from the configuration or queried from the robot.
async fn describe(config: &NaoqiRobotConfig, node: &Node, memory: &AnyObject) -> HalDescription {
    let model_family = match &config.model_family {
        Some(family) => Some(family.clone()),
        None => match node.service("ALRobotModel").await {
            Ok(model) => model
                .call::<String, _, _>("getRobotType", ())
                .await
                .ok()
                .map(|robot_type| robot_type.to_lowercase()),
            Err(_) => None,
        },
    };
    let software_version = match &config.software_version {
        Some(version) => Some(version.clone()),
        None => match node.service("ALSystem").await {
            Ok(system) => system.call::<String, _, _>("systemVersion", ()).await.ok(),
            Err(_) => None,
        },
    };
    let hardware_version = match &config.hardware_version {
        Some(version) => Some(version.clone()),
        None => memory
            .call::<String, _, _>("getData", "RobotConfig/Body/BaseVersion".to_string())
            .await
            .ok(),
    };
    HalDescription {
        model_family,
        hardware_version,
        software_version,
    }
}

impl Inner {
    /// The `ALMemory` keys sampled periodically, with the Arora keys they feed.
    fn sampled_memory_keys(&self) -> Vec<(String, String)> {
        let mut keys = Vec::new();
        if self.config.sensors.joints {
            keys.extend(self.joints.memory_keys());
        }
        let fixed = [
            (self.config.sensors.battery, keys::BATTERY_MEMORY_KEYS),
            (self.config.sensors.inertial, keys::INERTIAL_MEMORY_KEYS),
            (self.config.sensors.sonar, keys::SONAR_MEMORY_KEYS),
        ];
        for (enabled, table) in fixed {
            if enabled {
                keys.extend(
                    table
                        .iter()
                        .map(|(memory, arora)| (memory.to_string(), arora.to_string())),
                );
            }
        }
        keys
    }

    /// Samples the sensors in one `ALMemory.getListData` call and publishes what changed.
    async fn sample_sensors(&self, sampling: &mut Sampling) -> Result<(), NaoqiRobotError> {
        let keys = self.sampled_memory_keys();
        if keys.is_empty() {
            return Ok(());
        }
        let names: Vec<String> = keys.iter().map(|(memory, _)| memory.clone()).collect();
        let Dynamic(values) = self
            .memory
            .call::<Dynamic<QiValue<'static>>, _, _>("getListData", names)
            .await?;
        let values = qi_list(values).ok_or_else(|| {
            NaoqiRobotError::Conversion("ALMemory.getListData did not return a list".to_string())
        })?;
        if values.len() != keys.len() {
            return Err(NaoqiRobotError::Conversion(format!(
                "ALMemory.getListData returned {} values for {} keys",
                values.len(),
                keys.len()
            )));
        }

        let now = Instant::now();
        let mut change = StateChange::new();
        let mut positions = Vec::new();
        for ((_, arora_key), value) in keys.iter().zip(values) {
            // Keys the robot does not have come back as invalid values: skip them.
            let Some(number) = qi_f64(&value) else {
                continue;
            };
            let key = Key::from(arora_key.as_str());
            if key.get_component() == Some(keys::POSITION) {
                positions.push((key.get_entity().to_string(), number));
            }
            if arora_key == keys::BATTERY_CURRENT {
                change.set.insert(
                    Key::from(keys::BATTERY_CHARGING),
                    Some(Value::Boolean(number > 0.0)),
                );
            }
            change.set.insert(key, Some(Value::F64(number)));
        }
        // Joint velocities are derived from consecutive position samples.
        if let Some((previous_time, previous_positions)) = &sampling.previous_positions {
            let elapsed = now.duration_since(*previous_time).as_secs_f64();
            if elapsed > 0.0 {
                for (joint, position) in &positions {
                    if let Some((_, previous)) = previous_positions
                        .iter()
                        .find(|(previous_joint, _)| previous_joint == joint)
                    {
                        change.set.insert(
                            Key::from(format!("{joint}.{}", keys::VELOCITY)),
                            Some(Value::F64((position - previous) / elapsed)),
                        );
                    }
                }
            }
        }
        sampling.previous_positions = Some((now, positions));
        self.publish(change);
        Ok(())
    }

    /// Drives a boolean key from an `ALMemory` event: its current value, then every raise.
    async fn watch_event(self: Arc<Self>, event: &'static str, key: &'static str) {
        if let Ok(Dynamic(value)) = self
            .memory
            .call::<Dynamic<QiValue<'static>>, _, _>("getData", event.to_string())
            .await
        {
            if let Some(number) = qi_f64(&value) {
                self.publish(StateChange::set(key, Value::Boolean(number != 0.0)));
            }
        }
        let subscriber: AnyObject = match self.memory.call("subscriber", event.to_string()).await {
            Ok(subscriber) => subscriber,
            Err(error) => {
                warn!("Cannot subscribe to the {event} event, {key} is not driven: {error}");
                return;
            }
        };
        let mut raises = match subscriber
            .subscribe::<_, Dynamic<QiValue<'static>>>("signal")
            .await
        {
            Ok(raises) => raises,
            Err(error) => {
                warn!("Cannot subscribe to the {event} event, {key} is not driven: {error}");
                return;
            }
        };
        while let Some(Dynamic(value)) = raises.next().await {
            if let Some(number) = qi_f64(&value) {
                self.publish(StateChange::set(key, Value::Boolean(number != 0.0)));
            }
        }
        debug!("The {event} event stream ended");
    }

    /// Applies a change to the current state and notifies subscribers of what actually
    /// changed.
    fn publish(&self, change: StateChange) {
        let mut delta = StateChange::new();
        {
            let mut state = lock(&self.current_state);
            for (key, value) in change.set {
                if state.get(&key) != Some(&value) {
                    state.set(key.clone(), value.clone());
                    delta.set.insert(key, value);
                }
            }
            for key in change.unset {
                if state.get(&key).is_some() {
                    state.unset(&key);
                    delta.unset.insert(key);
                }
            }
        }
        if delta.is_empty() {
            return;
        }
        lock(&self.subscribers).retain(|tx| tx.unbounded_send(delta.clone()).is_ok());
    }

    /// Turns a change into NAOqi commands. Returns `Ok` when every command succeeded, or when
    /// there was nothing to command (partial failures are logged), and `Err` when every
    /// command failed.
    async fn write(self: &Arc<Self>, changes: StateChange) -> Result<(), NaoqiRobotError> {
        let mut joint_targets: Vec<(String, f32)> = Vec::new();
        let mut stiffness_targets: Vec<(String, f32)> = Vec::new();
        let mut text = None;
        let mut led_colors: Vec<(String, i32)> = Vec::new();
        let mut led_intensities: Vec<(String, f32)> = Vec::new();
        let mut base_velocity = None;

        for (key, value) in &changes.set {
            let Some(value) = value else {
                continue;
            };
            let entity = key.get_entity();
            let attributes = key.get_attributes();
            match (entity, attributes.as_slice()) {
                (keys::TEXT, []) => match value {
                    Value::String(sentence) => text = Some(sentence.clone()),
                    other => warn!("Ignoring a non-string text: {other}"),
                },
                (joint, [keys::TARGET_POSITION]) => {
                    if let Some(name) = self.joints.naoqi_name(joint) {
                        match arora_f64(value) {
                            Some(angle) => joint_targets.push((name.to_string(), angle as f32)),
                            None => warn!("Ignoring a non-numeric target position for {joint}"),
                        }
                    }
                }
                (joint, [keys::TARGET_STIFFNESS]) => {
                    if let Some(name) = self.joints.naoqi_name(joint) {
                        match arora_f64(value) {
                            Some(stiffness) => {
                                stiffness_targets.push((name.to_string(), stiffness as f32))
                            }
                            None => warn!("Ignoring a non-numeric target stiffness for {joint}"),
                        }
                    }
                }
                (keys::LED_ENTITY, [group, "color"]) => match arora_f64(value) {
                    Some(rgb) => led_colors.push((group.to_string(), rgb as i32)),
                    None => warn!("Ignoring a non-numeric color for the {group} LEDs"),
                },
                (keys::LED_ENTITY, [group, "intensity"]) => match arora_f64(value) {
                    Some(intensity) => led_intensities.push((group.to_string(), intensity as f32)),
                    None => warn!("Ignoring a non-numeric intensity for the {group} LEDs"),
                },
                ("velocity", ["x"]) | ("velocity", ["y"]) | ("rotation", ["z"]) => {
                    if let Some(component) = arora_f64(value) {
                        let mut velocity = lock(&self.base_velocity);
                        let index = match (entity, attributes[0]) {
                            ("velocity", "x") => 0,
                            ("velocity", "y") => 1,
                            _ => 2,
                        };
                        velocity[index] = component;
                        base_velocity = Some(*velocity);
                    }
                }
                _ => {}
            }
        }

        // Cache every written key, so that reads reflect the commands sent.
        {
            let mut state = lock(&self.current_state);
            for (key, value) in &changes.set {
                state.set(key.clone(), value.clone());
            }
            for key in &changes.unset {
                state.unset(key);
            }
        }

        let mut attempted = 0;
        let mut failures = Vec::new();
        if !joint_targets.is_empty() {
            attempted += 1;
            let (names, angles): (Vec<String>, Vec<f32>) = joint_targets.into_iter().unzip();
            let speed = self.config.joint_speed_fraction as f32;
            if let Err(error) = self
                .motion
                .call::<(), _, _>("setAngles", (names, angles, speed))
                .await
            {
                failures.push(format!("ALMotion.setAngles: {error}"));
            }
        }
        if !stiffness_targets.is_empty() {
            attempted += 1;
            let (names, values): (Vec<String>, Vec<f32>) = stiffness_targets.into_iter().unzip();
            if let Err(error) = self
                .motion
                .call::<(), _, _>("setStiffnesses", (names, values))
                .await
            {
                failures.push(format!("ALMotion.setStiffnesses: {error}"));
            }
        }
        if let Some(sentence) = text {
            // Saying blocks until the sentence is spoken: it runs on its own, and the key is
            // unset once spoken so that the same sentence can be said again.
            let inner = Arc::clone(self);
            tokio::spawn(async move {
                match inner.tts.call::<(), _, _>("say", sentence).await {
                    Ok(()) => {
                        let mut change = StateChange::new();
                        change.unset.insert(Key::from(keys::TEXT));
                        inner.publish(change);
                    }
                    Err(error) => warn!("ALTextToSpeech.say failed: {error}"),
                }
            });
        }
        if let Some(leds) = &self.leds {
            for (group, rgb) in led_colors {
                attempted += 1;
                if let Err(error) = leds
                    .call::<(), _, _>("fadeRGB", (group.clone(), rgb, 0.0f32))
                    .await
                {
                    failures.push(format!("ALLeds.fadeRGB({group}): {error}"));
                }
            }
            for (group, intensity) in led_intensities {
                attempted += 1;
                if let Err(error) = leds
                    .call::<(), _, _>("setIntensity", (group.clone(), intensity))
                    .await
                {
                    failures.push(format!("ALLeds.setIntensity({group}): {error}"));
                }
            }
        }
        if let Some([x, y, theta]) = base_velocity {
            attempted += 1;
            if let Err(error) = self
                .motion
                .call::<(), _, _>("moveToward", (x as f32, y as f32, theta as f32))
                .await
            {
                failures.push(format!("ALMotion.moveToward: {error}"));
            }
        }

        if attempted > 0 && failures.len() == attempted {
            return Err(NaoqiRobotError::Write(failures.join("; ")));
        }
        for failure in failures {
            warn!("Partial write failure: {failure}");
        }
        Ok(())
    }
}

#[async_trait]
impl Hal for NaoqiHal {
    async fn describe(&self) -> HalDescription {
        self.inner.description.clone()
    }

    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
        let state = lock(&self.inner.current_state);
        Ok(keys
            .iter()
            .map(|key| state.get(key).cloned().flatten())
            .collect())
    }

    async fn read_all(&self) -> HalResult<State> {
        Ok(lock(&self.inner.current_state).clone())
    }

    async fn write(&self, changes: StateChange) -> HalResult<()> {
        self.inner.write(changes).await.map_err(Into::into)
    }

    /// Hands the change to the queued-write task; never blocks.
    fn try_send(&self, changes: &StateChange) {
        if self.outbound.unbounded_send(changes.clone()).is_err() {
            warn!("Dropping a state change: the queued-write task is gone");
        }
    }

    /// A feed of the changes the robot reports. Each feed first receives the current state
    /// as one `StateChange` (when non-empty), then every subsequent change.
    fn updates(&self) -> UpdatesStream {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let snapshot = lock(&self.inner.current_state).clone();
        if !snapshot.is_empty() {
            let _ = tx.unbounded_send(StateChange {
                set: snapshot.storage,
                unset: Default::default(),
            });
        }
        lock(&self.inner.subscribers).push(tx);
        Box::pin(rx)
    }
}

impl Drop for NaoqiHal {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
