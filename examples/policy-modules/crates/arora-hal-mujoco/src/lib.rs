//! A MuJoCo simulation as an Arora [`Hal`].
//!
//! The simulated robot is presented exactly as a real one is: joints and
//! sensors under the keys a robot HAL publishes, actuation under the keys a
//! robot HAL consumes, and nothing else — a policy module or a behavior cannot
//! tell whether it drives the simulator or the hardware, which is the point of
//! simulating. What only a simulator can report (the base pose, the sim clock,
//! a reset) lives apart under the `sim/` namespace, for tests and observers.
//!
//! # Keys
//!
//! Published (the sensor feed, once per control period):
//!
//! | Key | Value | Meaning |
//! |---|---|---|
//! | `<joint>.position` | `F32` | joint angle, rad |
//! | `<joint>.velocity` | `F32` | joint velocity, rad/s |
//! | `joints.names` | `ArrayString` | the joint order of the aggregate keys |
//! | `joints.position` / `joints.velocity` | `ArrayF32` | all joints, in that order |
//! | `imu.gyro` | `ArrayF32` | angular velocity in the IMU frame, rad/s |
//! | `imu.accelerometer` | `ArrayF32` | specific force in the IMU frame, m/s² |
//! | `imu.orientation` | `ArrayF32` | world-from-IMU unit quaternion `[w, x, y, z]` |
//! | `<contact>.contact` | `Boolean` | a touch sensor reports force |
//! | `<joint>.target_position` / `joints.target_position` | `F32` / `ArrayF32` | the setpoints in force, reported back |
//! | `sim/time` | `F64` | simulated seconds |
//! | `sim/base.position` / `sim/base.orientation` / `sim/base.linear_velocity` | `ArrayF32` | the base body's ground truth |
//!
//! Consumed (the actuation the runtime flushes):
//!
//! | Key | Value | Effect |
//! |---|---|---|
//! | `<joint>.target_position` | `F32` / `F64` | that joint's actuator setpoint |
//! | `joints.target_position` | `ArrayF32` / `ArrayF64` | every setpoint, in `joints.names` order |
//! | `sim/reset` | `Boolean` `true` | reset to the initial keyframe at the next tick |
//!
//! A setpoint is a position: the MJCF is expected to drive each listed joint
//! with a position actuator (MuJoCo's `<position>` servo), the actuator model
//! the policies were trained against. A model that lacks one for a listed
//! joint is refused at construction.
//!
//! # Clock
//!
//! The simulation advances in control periods (`1 / control_hz` of simulated
//! time, an integral number of physics steps). Who calls for the next period
//! is the [`Clock`]: a thread pacing itself against the wall clock, the way a
//! robot's own clock runs, or the embedder in [`lockstep`](Clock::Lockstep),
//! calling [`advance`](MujocoHal::advance) between runtime steps for a run
//! that is deterministic and as fast as the machine allows — what a test
//! wants.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arora_hal::{Hal, HalDescription, HalError, HalResult, UpdatesStream};
use arora_types::data::{Key, State, StateChange};
use arora_types::value::Value;
use async_trait::async_trait;
use futures_channel::mpsc::UnboundedSender;
use mujoco_rs::prelude::*;

/// One controlled joint: its MJCF joint and the position actuator driving it.
#[derive(Debug, Clone)]
pub struct JointSpec {
    /// The MJCF joint name; also the key entity (`<joint>.position`).
    pub joint: String,
    /// The MJCF actuator name; the same as the joint when not given.
    pub actuator: Option<String>,
}

impl JointSpec {
    /// A joint whose actuator carries the joint's name.
    pub fn named(joint: impl Into<String>) -> Self {
        Self {
            joint: joint.into(),
            actuator: None,
        }
    }

    fn actuator_name(&self) -> &str {
        self.actuator.as_deref().unwrap_or(&self.joint)
    }
}

/// The IMU, as MJCF sensors on one site.
#[derive(Debug, Clone)]
pub struct ImuSpec {
    /// A `<gyro>` sensor name.
    pub gyro: String,
    /// An `<accelerometer>` sensor name.
    pub accelerometer: String,
    /// A `<framequat>` sensor name; with none, the base body's orientation is
    /// published instead (a robot whose IMU is rigidly mounted on its base).
    pub orientation: Option<String>,
}

/// A contact sensor: a `<touch>` sensor published as `<key>.contact`.
#[derive(Debug, Clone)]
pub struct ContactSpec {
    /// The key entity (`<key>.contact`).
    pub key: String,
    /// The MJCF touch sensor name.
    pub touch: String,
}

/// What drives the simulation forward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Clock {
    /// A thread advances one control period per wall-clock period, scaled
    /// by `speed` (2.0 runs twice real time; 0.0 runs as fast as it can).
    RealTime { speed: f64 },
    /// Nothing advances until the embedder calls [`MujocoHal::advance`].
    Lockstep,
}

/// How to build a [`MujocoHal`].
#[derive(Debug, Clone)]
pub struct MujocoHalConfig {
    /// The MJCF scene to load (meshes resolve relative to it).
    pub model: PathBuf,
    /// The controlled joints, in the order the aggregate keys use.
    pub joints: Vec<JointSpec>,
    pub imu: Option<ImuSpec>,
    pub contacts: Vec<ContactSpec>,
    /// The body whose ground truth is published under `sim/base.*`.
    pub base_body: Option<String>,
    /// The keyframe the simulation starts from and resets to; the model's
    /// default state with none.
    pub keyframe: Option<String>,
    /// The control rate: sensors are published and setpoints applied this
    /// often. Must be an integral number of physics steps.
    pub control_hz: f64,
    pub clock: Clock,
    /// Record every control period's `time, <joint>.position…, <joint>.ctrl…,
    /// base.x, base.y, base.z, base.qw, base.qx, base.qy, base.qz` as CSV,
    /// for replay and plots.
    pub record: Option<PathBuf>,
    pub description: HalDescription,
}

/// The indices of one controlled joint's views, resolved once.
struct JointHandles {
    key: String,
    joint: MjJointDataInfo,
    actuator: MjActuatorDataInfo,
}

/// Everything the physics owns: the model, its data, and the resolved
/// handles. Accessed under one mutex, one control period at a time.
struct Sim {
    data: MjData<Arc<MjModel>>,
    joints: Vec<JointHandles>,
    gyro: Option<MjSensorDataInfo>,
    accelerometer: Option<MjSensorDataInfo>,
    orientation: Option<MjSensorDataInfo>,
    contacts: Vec<(String, MjSensorDataInfo)>,
    base: Option<MjBodyDataInfo>,
    keyframe: Option<usize>,
    substeps: usize,
    recorder: Option<std::io::BufWriter<std::fs::File>>,
}

// SAFETY: MuJoCo's model and data carry no thread affinity; the C API only
// requires that one thread use a given `mjData` at a time, which the mutex
// around `Sim` guarantees. The raw pointers inside the wrappers are what
// makes them `!Send` by default.
unsafe impl Send for Sim {}

/// What the runtime wrote since the last control period.
#[derive(Default)]
struct Pending {
    /// The setpoint per controlled joint, applied at the next period.
    ctrl: Vec<f64>,
    reset: bool,
}

struct Shared {
    sim: Mutex<Sim>,
    pending: Mutex<Pending>,
    /// The last published values; what `read` answers between periods.
    snapshot: Mutex<State>,
    subscribers: Mutex<Vec<UnboundedSender<StateChange>>>,
    joint_names: Vec<String>,
    control_period: Duration,
    stop: AtomicBool,
}

/// A MuJoCo simulation presented as a robot.
pub struct MujocoHal {
    shared: Arc<Shared>,
    description: HalDescription,
    thread: Option<JoinHandle<()>>,
}

impl MujocoHal {
    /// Load the model, resolve every named joint, actuator, sensor and body,
    /// put the robot in its keyframe, and start the clock.
    pub fn new(config: MujocoHalConfig) -> HalResult<Self> {
        for spec in &config.joints {
            check_key_entity(&spec.joint)?;
        }
        for spec in &config.contacts {
            check_key_entity(&spec.key)?;
        }
        if config.joints.is_empty() {
            return Err(HalError::Other("no joints listed".to_string()));
        }

        let model = MjModel::from_xml(&config.model).map_err(|e| {
            HalError::Other(format!("could not load {}: {e}", config.model.display()))
        })?;
        let timestep = model.opt().timestep;
        let control_period_s = 1.0 / config.control_hz;
        let substeps_exact = control_period_s / timestep;
        let substeps = substeps_exact.round();
        if (substeps_exact - substeps).abs() > 1e-6 || substeps < 1.0 {
            return Err(HalError::Other(format!(
                "a control period of {control_period_s} s is not a whole number of {timestep} s physics steps"
            )));
        }
        let substeps = substeps as usize;

        let keyframe = match &config.keyframe {
            Some(name) => Some(
                model
                    .name_to_id(MjtObj::mjOBJ_KEY, name)
                    .ok_or_else(|| HalError::NoSuchKey(format!("keyframe {name}")))?,
            ),
            None => None,
        };
        let mut data = MjData::new(Arc::new(model));
        if let Some(key) = keyframe {
            data.reset_keyframe(key)
                .map_err(|e| HalError::Other(format!("could not reset to the keyframe: {e}")))?;
        }
        data.forward();

        let mut joints = Vec::with_capacity(config.joints.len());
        for spec in &config.joints {
            let joint = data
                .joint(&spec.joint)
                .ok_or_else(|| HalError::NoSuchKey(format!("joint {}", spec.joint)))?;
            let actuator = data
                .actuator(spec.actuator_name())
                .ok_or_else(|| HalError::NoSuchKey(format!("actuator {}", spec.actuator_name())))?;
            if joint.view(&data).qpos.len() != 1 {
                return Err(HalError::Other(format!(
                    "joint {} is not a hinge or slide joint (one position)",
                    spec.joint
                )));
            }
            joints.push(JointHandles {
                key: spec.joint.clone(),
                joint,
                actuator,
            });
        }
        let sensor = |name: &str| {
            data.sensor(name)
                .ok_or_else(|| HalError::NoSuchKey(format!("sensor {name}")))
        };
        let (gyro, accelerometer, orientation) = match &config.imu {
            Some(imu) => (
                Some(sensor(&imu.gyro)?),
                Some(sensor(&imu.accelerometer)?),
                imu.orientation.as_deref().map(sensor).transpose()?,
            ),
            None => (None, None, None),
        };
        let contacts = config
            .contacts
            .iter()
            .map(|spec| Ok((spec.key.clone(), sensor(&spec.touch)?)))
            .collect::<HalResult<Vec<_>>>()?;
        let base = config
            .base_body
            .as_deref()
            .map(|name| {
                data.body(name)
                    .ok_or_else(|| HalError::NoSuchKey(format!("body {name}")))
            })
            .transpose()?;

        // The initial setpoints hold the keyframe pose until a behavior writes
        // its own, so the robot does not slump while the policy loads.
        let initial_ctrl: Vec<f64> = joints
            .iter()
            .map(|handles| handles.actuator.view(&data).ctrl[0])
            .collect();

        let recorder = match &config.record {
            Some(path) => {
                let file = std::fs::File::create(path).map_err(|e| {
                    HalError::Other(format!("could not create {}: {e}", path.display()))
                })?;
                let mut writer = std::io::BufWriter::new(file);
                let mut header = vec!["time".to_string()];
                header.extend(joints.iter().map(|j| format!("{}.position", j.key)));
                header.extend(joints.iter().map(|j| format!("{}.ctrl", j.key)));
                header.extend(
                    [
                        "base.x", "base.y", "base.z", "base.qw", "base.qx", "base.qy", "base.qz",
                    ]
                    .map(String::from),
                );
                writeln!(writer, "{}", header.join(","))
                    .map_err(|e| HalError::Other(e.to_string()))?;
                Some(writer)
            }
            None => None,
        };

        let joint_names: Vec<String> = joints.iter().map(|j| j.key.clone()).collect();
        let mut sim = Sim {
            data,
            joints,
            gyro,
            accelerometer,
            orientation,
            contacts,
            base,
            keyframe,
            substeps,
            recorder,
        };
        let snapshot = {
            let mut state = State::new();
            state.set(
                "joints.names",
                Some(Value::ArrayString(joint_names.clone())),
            );
            state.apply(sim.sample());
            state
        };

        let shared = Arc::new(Shared {
            sim: Mutex::new(sim),
            pending: Mutex::new(Pending {
                ctrl: initial_ctrl,
                reset: false,
            }),
            snapshot: Mutex::new(snapshot),
            subscribers: Mutex::new(Vec::new()),
            joint_names,
            control_period: Duration::from_secs_f64(control_period_s),
            stop: AtomicBool::new(false),
        });

        let thread = match config.clock {
            Clock::Lockstep => None,
            Clock::RealTime { speed } => {
                let shared = shared.clone();
                Some(std::thread::spawn(move || pace(shared, speed)))
            }
        };

        Ok(Self {
            shared,
            description: config.description,
            thread,
        })
    }

    /// Advance one control period now: apply the pending setpoints, step the
    /// physics, publish the sensors. The [`lockstep`](Clock::Lockstep) clock;
    /// harmless but pointless under a real-time one.
    pub fn advance(&self) {
        advance(&self.shared);
    }

    /// The control period, in simulated time.
    pub fn control_period(&self) -> Duration {
        self.shared.control_period
    }

    /// The controlled joints, in the order of the aggregate keys.
    pub fn joint_names(&self) -> &[String] {
        &self.shared.joint_names
    }

    /// The last published values.
    pub fn snapshot(&self) -> State {
        self.shared.snapshot.lock().unwrap().clone()
    }
}

/// A handle onto a [`MujocoHal`]'s simulation that outlives handing the HAL
/// to a device by value: what a lockstep driver advances, and what a test
/// reads ground truth from.
#[derive(Clone)]
pub struct SimHandle {
    shared: Arc<Shared>,
}

impl SimHandle {
    /// Advance one control period — see [`MujocoHal::advance`].
    pub fn advance(&self) {
        advance(&self.shared);
    }

    /// The control period, in simulated time.
    pub fn control_period(&self) -> Duration {
        self.shared.control_period
    }

    /// The last published values.
    pub fn snapshot(&self) -> State {
        self.shared.snapshot.lock().unwrap().clone()
    }

    /// The controlled joints, in the order of the aggregate keys.
    pub fn joint_names(&self) -> &[String] {
        &self.shared.joint_names
    }
}

impl MujocoHal {
    /// A handle onto this simulation, independent of the HAL's ownership.
    pub fn handle(&self) -> SimHandle {
        SimHandle {
            shared: self.shared.clone(),
        }
    }
}

impl Drop for MujocoHal {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The real-time clock: one control period per wall-clock period over
/// `speed`, without drift (each deadline is computed from the start, not from
/// the previous wake-up), until the HAL is dropped.
fn pace(shared: Arc<Shared>, speed: f64) {
    let start = Instant::now();
    let mut periods: u32 = 0;
    while !shared.stop.load(Ordering::Relaxed) {
        advance(&shared);
        periods += 1;
        if speed > 0.0 {
            let deadline = start + shared.control_period.mul_f64(periods as f64 / speed);
            let now = Instant::now();
            if deadline > now {
                std::thread::sleep(deadline - now);
            }
        }
    }
}

/// One control period: setpoints in, physics, sensors out.
fn advance(shared: &Shared) {
    let (ctrl, reset) = {
        let mut pending = shared.pending.lock().unwrap();
        (pending.ctrl.clone(), std::mem::take(&mut pending.reset))
    };
    let change = {
        let mut sim = shared.sim.lock().unwrap();
        if reset {
            sim.reset();
            // A reset returns to the keyframe's setpoints too.
            let keyframe_ctrl: Vec<f64> = sim
                .joints
                .iter()
                .map(|handles| handles.actuator.view(&sim.data).ctrl[0])
                .collect();
            shared.pending.lock().unwrap().ctrl = keyframe_ctrl;
        } else {
            sim.apply(&ctrl);
        }
        sim.step();
        sim.sample()
    };
    shared.snapshot.lock().unwrap().apply(change.clone());
    shared
        .subscribers
        .lock()
        .unwrap()
        .retain(|tx| tx.unbounded_send(change.clone()).is_ok());
}

impl Sim {
    fn reset(&mut self) {
        match self.keyframe {
            Some(key) => self
                .data
                .reset_keyframe(key)
                .expect("the keyframe resolved at construction"),
            None => self.data.reset(),
        }
        self.data.forward();
    }

    fn apply(&mut self, ctrl: &[f64]) {
        for (handles, value) in self.joints.iter().zip(ctrl) {
            handles.actuator.view_mut(&mut self.data).ctrl[0] = *value;
        }
    }

    fn step(&mut self) {
        for _ in 0..self.substeps {
            self.data.step();
        }
    }

    /// The sensor feed for the current state, and a recorder row.
    fn sample(&mut self) -> StateChange {
        let data = &self.data;
        let mut change = StateChange::new();
        let mut positions = Vec::with_capacity(self.joints.len());
        let mut velocities = Vec::with_capacity(self.joints.len());
        for handles in &self.joints {
            let view = handles.joint.view(data);
            let position = view.qpos[0] as f32;
            let velocity = view.qvel[0] as f32;
            positions.push(position);
            velocities.push(velocity);
            change.set.insert(
                Key::from(format!("{}.position", handles.key)),
                Some(Value::F32(position)),
            );
            change.set.insert(
                Key::from(format!("{}.velocity", handles.key)),
                Some(Value::F32(velocity)),
            );
        }
        change.set.insert(
            Key::from("joints.position"),
            Some(Value::ArrayF32(positions.clone())),
        );
        change.set.insert(
            Key::from("joints.velocity"),
            Some(Value::ArrayF32(velocities)),
        );
        // The setpoints in force, reported the way a robot reports its
        // commanded targets: what a behavior's `targets` out-parameter starts
        // from before it has written anything.
        let targets: Vec<f32> = self
            .joints
            .iter()
            .map(|handles| handles.actuator.view(data).ctrl[0] as f32)
            .collect();
        for (handles, target) in self.joints.iter().zip(&targets) {
            change.set.insert(
                Key::from(format!("{}.target_position", handles.key)),
                Some(Value::F32(*target)),
            );
        }
        change.set.insert(
            Key::from("joints.target_position"),
            Some(Value::ArrayF32(targets)),
        );

        let sensor_f32 = |info: &MjSensorDataInfo| -> Vec<f32> {
            info.view(data).data.iter().map(|v| *v as f32).collect()
        };
        if let Some(gyro) = &self.gyro {
            change.set.insert(
                Key::from("imu.gyro"),
                Some(Value::ArrayF32(sensor_f32(gyro))),
            );
        }
        if let Some(accelerometer) = &self.accelerometer {
            change.set.insert(
                Key::from("imu.accelerometer"),
                Some(Value::ArrayF32(sensor_f32(accelerometer))),
            );
        }
        let base_quat = self.base.as_ref().map(|base| {
            base.view(data)
                .xquat
                .iter()
                .map(|v| *v as f32)
                .collect::<Vec<f32>>()
        });
        let orientation = match (&self.orientation, &base_quat) {
            (Some(sensor), _) => Some(sensor_f32(sensor)),
            (None, Some(quat)) => Some(quat.clone()),
            (None, None) => None,
        };
        if let Some(orientation) = orientation {
            change.set.insert(
                Key::from("imu.orientation"),
                Some(Value::ArrayF32(orientation)),
            );
        }
        for (key, touch) in &self.contacts {
            let force = touch.view(data).data[0];
            change.set.insert(
                Key::from(format!("{key}.contact")),
                Some(Value::Boolean(force > 0.0)),
            );
        }

        change
            .set
            .insert(Key::from("sim/time"), Some(Value::F64(data.time())));
        let mut base_position = [0.0f32; 3];
        if let Some(base) = &self.base {
            let view = base.view(data);
            for (i, v) in view.xpos.iter().enumerate() {
                base_position[i] = *v as f32;
            }
            change.set.insert(
                Key::from("sim/base.position"),
                Some(Value::ArrayF32(base_position.to_vec())),
            );
            change.set.insert(
                Key::from("sim/base.orientation"),
                Some(Value::ArrayF32(
                    base_quat.clone().expect("computed with the base"),
                )),
            );
            change.set.insert(
                Key::from("sim/base.linear_velocity"),
                Some(Value::ArrayF32(
                    view.subtree_linvel.iter().map(|v| *v as f32).collect(),
                )),
            );
        }

        if let Some(recorder) = &mut self.recorder {
            let mut row = vec![format!("{:.4}", data.time())];
            row.extend(positions.iter().map(|p| format!("{p:.5}")));
            row.extend(
                self.joints
                    .iter()
                    .map(|handles| format!("{:.5}", handles.actuator.view(data).ctrl[0])),
            );
            row.extend(base_position.iter().map(|p| format!("{p:.4}")));
            row.extend(
                base_quat
                    .as_deref()
                    .unwrap_or(&[1.0, 0.0, 0.0, 0.0])
                    .iter()
                    .map(|q| format!("{q:.5}")),
            );
            // A recording that cannot be written is not worth stopping the
            // robot for; the failure is logged and recording stops.
            if let Err(error) = writeln!(recorder, "{}", row.join(",")) {
                log::error!("trajectory recording failed: {error}");
                self.recorder = None;
            }
        }
        change
    }
}

/// A key entity is alphanumeric with underscores; a joint or contact name
/// that is not would be silently unreachable under the key it names.
fn check_key_entity(name: &str) -> HalResult<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(HalError::Other(format!(
            "{name:?} cannot name a key entity (alphanumeric and underscore only)"
        )));
    }
    Ok(())
}

/// A setpoint as a number, whichever float it was written as.
fn scalar(value: &Value) -> Option<f64> {
    match value {
        Value::F32(v) => Some(*v as f64),
        Value::F64(v) => Some(*v),
        _ => None,
    }
}

/// Setpoints as numbers, whichever float array they were written as.
fn array(value: &Value) -> Option<Vec<f64>> {
    match value {
        Value::ArrayF32(v) => Some(v.iter().map(|x| *x as f64).collect()),
        Value::ArrayF64(v) => Some(v.clone()),
        _ => None,
    }
}

impl MujocoHal {
    /// Take the setpoints and the reset out of a change; everything else is
    /// what hardware ignores when it has no actuator for it.
    fn absorb(&self, changes: &StateChange) {
        let mut pending = self.shared.pending.lock().unwrap();
        let joint_index: HashMap<&str, usize> = self
            .shared
            .joint_names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i))
            .collect();
        for (key, value) in &changes.set {
            let Some(value) = value else { continue };
            if !key.get_namespace().is_empty() {
                if key.get_path() == "sim/reset" && matches!(value, Value::Boolean(true)) {
                    pending.reset = true;
                }
                continue;
            }
            if key.get_component() != Some("target_position") {
                continue;
            }
            if key.get_entity() == "joints" {
                match array(value) {
                    Some(targets) if targets.len() == pending.ctrl.len() => {
                        pending.ctrl.copy_from_slice(&targets)
                    }
                    Some(targets) => log::warn!(
                        "joints.target_position has {} values for {} joints; ignored",
                        targets.len(),
                        pending.ctrl.len()
                    ),
                    None => log::warn!("joints.target_position is not a float array; ignored"),
                }
            } else if let Some(&index) = joint_index.get(key.get_entity()) {
                match scalar(value) {
                    Some(target) => pending.ctrl[index] = target,
                    None => log::warn!("{} is not a float; ignored", key.get_path()),
                }
            }
        }
    }
}

#[async_trait]
impl Hal for MujocoHal {
    async fn describe(&self) -> HalDescription {
        self.description.clone()
    }

    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
        let snapshot = self.shared.snapshot.lock().unwrap();
        Ok(keys
            .iter()
            .map(|key| snapshot.get(key).cloned().flatten())
            .collect())
    }

    async fn read_all(&self) -> HalResult<State> {
        Ok(self.snapshot())
    }

    async fn write(&self, changes: StateChange) -> HalResult<()> {
        self.absorb(&changes);
        Ok(())
    }

    /// Records the setpoints for the next control period; never blocks on the
    /// physics, which runs on its own clock.
    fn try_send(&self, changes: &StateChange) {
        self.absorb(changes);
    }

    /// The feed opens with the whole current state — the joint order included
    /// — then carries every control period's sensors.
    fn updates(&self) -> UpdatesStream {
        let (tx, rx) = futures_channel::mpsc::unbounded();
        let snapshot = self.snapshot();
        let mut first = StateChange::new();
        for (key, value) in snapshot.iter() {
            first.set.insert(key.clone(), value.clone());
        }
        let _ = tx.unbounded_send(first);
        self.shared.subscribers.lock().unwrap().push(tx);
        Box::pin(rx) as Pin<Box<dyn futures_core::Stream<Item = StateChange> + Send>>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, StreamExt};

    /// A pendulum on a position servo: one hinge, one actuator, an IMU site
    /// on the swinging body, a touch sensor on a pad it can hit.
    const MODEL: &str = r#"
<mujoco model="pendulum">
  <option timestep="0.002"/>
  <worldbody>
    <geom type="plane" size="1 1 0.1"/>
    <body name="base" pos="0 0 0.5">
      <joint name="pivot" type="hinge" axis="0 1 0" damping="0.3"/>
      <geom type="capsule" fromto="0 0 0 0 0 -0.3" size="0.02" mass="0.2"/>
      <site name="imu" pos="0 0 -0.3"/>
    </body>
    <body name="pad" pos="0.35 0 0.2">
      <geom type="box" size="0.05 0.05 0.02"/>
      <site name="pad_site" type="box" size="0.05 0.05 0.02"/>
    </body>
  </worldbody>
  <actuator>
    <position name="pivot" joint="pivot" kp="30"/>
  </actuator>
  <sensor>
    <gyro name="gyro" site="imu"/>
    <accelerometer name="accel" site="imu"/>
    <framequat name="quat" objtype="site" objname="imu"/>
    <touch name="pad_touch" site="pad_site"/>
  </sensor>
  <keyframe>
    <key name="home" qpos="0.1" ctrl="0.1"/>
  </keyframe>
</mujoco>
"#;

    fn config(dir: &std::path::Path) -> MujocoHalConfig {
        let model = dir.join("pendulum.xml");
        std::fs::write(&model, MODEL).unwrap();
        MujocoHalConfig {
            model,
            joints: vec![JointSpec::named("pivot")],
            imu: Some(ImuSpec {
                gyro: "gyro".into(),
                accelerometer: "accel".into(),
                orientation: Some("quat".into()),
            }),
            contacts: vec![ContactSpec {
                key: "pad".into(),
                touch: "pad_touch".into(),
            }],
            base_body: Some("base".into()),
            keyframe: Some("home".into()),
            control_hz: 50.0,
            clock: Clock::Lockstep,
            record: None,
            description: HalDescription::default(),
        }
    }

    fn drain(feed: &mut UpdatesStream) -> Vec<StateChange> {
        let mut out = Vec::new();
        while let Some(Some(change)) = feed.next().now_or_never() {
            out.push(change);
        }
        out
    }

    #[test]
    fn starts_in_the_keyframe_and_publishes_the_robot_keys() {
        let dir = tempdir();
        let hal = MujocoHal::new(config(&dir)).unwrap();
        let mut feed = hal.updates();
        let first = drain(&mut feed).remove(0);
        assert_eq!(
            first.set.get(&Key::from("joints.names")),
            Some(&Some(Value::ArrayString(vec!["pivot".into()])))
        );
        match first.set.get(&Key::from("pivot.position")) {
            Some(Some(Value::F32(p))) => assert!((p - 0.1).abs() < 1e-6, "{p}"),
            other => panic!("pivot.position missing: {other:?}"),
        }
        for key in [
            "imu.gyro",
            "imu.accelerometer",
            "imu.orientation",
            "sim/base.position",
        ] {
            assert!(first.contains(&Key::from(key)), "{key} is published");
        }
        assert_eq!(
            first.set.get(&Key::from("pad.contact")),
            Some(&Some(Value::Boolean(false)))
        );
    }

    #[test]
    fn a_setpoint_moves_the_joint_and_sim_time_advances_in_lockstep() {
        let dir = tempdir();
        let hal = MujocoHal::new(config(&dir)).unwrap();
        let mut feed = hal.updates();
        drain(&mut feed);
        hal.try_send(&StateChange::set("pivot.target_position", Value::F32(0.8)));
        for _ in 0..100 {
            hal.advance();
        }
        let changes = drain(&mut feed);
        assert_eq!(changes.len(), 100, "one change per control period");
        let last = changes.last().unwrap();
        match last.set.get(&Key::from("sim/time")) {
            Some(Some(Value::F64(t))) => assert!((t - 2.0).abs() < 1e-6, "{t}"),
            other => panic!("sim/time missing: {other:?}"),
        }
        match last.set.get(&Key::from("pivot.position")) {
            Some(Some(Value::F32(p))) => assert!((p - 0.8).abs() < 0.05, "servo reached {p}"),
            other => panic!("pivot.position missing: {other:?}"),
        }
        // The aggregate form drives the same actuator.
        hal.try_send(&StateChange::set(
            "joints.target_position",
            Value::ArrayF32(vec![-0.5]),
        ));
        for _ in 0..100 {
            hal.advance();
        }
        match hal.snapshot().get(&Key::from("pivot.position")) {
            Some(Some(Value::F32(p))) => assert!((p + 0.5).abs() < 0.05, "servo reached {p}"),
            other => panic!("pivot.position missing: {other:?}"),
        }
    }

    #[test]
    fn reset_returns_to_the_keyframe() {
        let dir = tempdir();
        let hal = MujocoHal::new(config(&dir)).unwrap();
        hal.try_send(&StateChange::set("pivot.target_position", Value::F64(1.0)));
        for _ in 0..50 {
            hal.advance();
        }
        hal.try_send(&StateChange::set("sim/reset", Value::Boolean(true)));
        hal.advance();
        match hal.snapshot().get(&Key::from("pivot.position")) {
            // One period after the reset the servo still holds the keyframe.
            Some(Some(Value::F32(p))) => assert!((p - 0.1).abs() < 0.02, "{p}"),
            other => panic!("pivot.position missing: {other:?}"),
        }
    }

    #[test]
    fn a_missing_actuator_or_a_bad_rate_is_refused() {
        let dir = tempdir();
        let mut bad = config(&dir);
        bad.joints = vec![JointSpec::named("nope")];
        assert!(matches!(MujocoHal::new(bad), Err(HalError::NoSuchKey(_))));
        let mut bad = config(&dir);
        bad.control_hz = 333.0;
        assert!(matches!(MujocoHal::new(bad), Err(HalError::Other(_))));
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arora-hal-mujoco-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
