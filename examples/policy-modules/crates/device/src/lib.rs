//! A Microduck device: MuJoCo as the HAL, the policy module as the leaves,
//! a Groot tree as the behavior — assembled on the standard Arora runtime.
//!
//! One assembly serves two uses. Built as a [`Device`] and driven in
//! [lockstep](Device::run_for) it is a deterministic, faster-than-real-time
//! simulation for tests and batch runs; [served](serve) it is a live device
//! at real time on the standard Arora run — the operator front end, and the
//! bridge the build selects: Semio Studio under the `studio` feature, the
//! open local bridge otherwise.
//!
//! Only the HAL is simulation-specific: a device for the real robot is this
//! assembly with the robot's HAL in place of [`MujocoHal`], publishing the
//! same keys.

pub mod studio;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use arora::{Arora, AroraBuilder, HostModule};
use arora_hal::HalDescription;
use arora_hal_mujoco::{Clock, ImuSpec, JointSpec, MujocoHal, MujocoHalConfig, SimHandle};
use arora_policy::projected_gravity;
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;
use microduck_policies::microduck_policies::Module as Policies;

pub use studio::StudioModel;

/// The policy module's wasm32-wasip1 build, handed over by cargo (the
/// artifact dependency in `Cargo.toml`).
const POLICY_WASM: &[u8] = include_bytes!(env!(
    "CARGO_CDYLIB_FILE_MICRODUCK_POLICIES_microduck_policies"
));

/// The blackboard module's wasm32-wasip1 build, the same way.
const BLACKBOARD_WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_BLACKBOARD_blackboard"));

/// The workspace root, for the default asset and tree paths.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Microduck scene fetched by `tools/fetch-assets.sh`.
pub fn default_model() -> PathBuf {
    workspace_root().join("assets/microduck/model/scene.xml")
}

/// The trees shipped with the example.
pub fn tree_path(name: &str) -> PathBuf {
    workspace_root()
        .join("trees")
        .join(format!("{name}.groot.xml"))
}

/// The runtime's step period: the policies' 50 Hz, one sensor sample per tick.
pub const PERIOD: Duration = Duration::from_millis(20);

/// How the policy module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Executor {
    /// As a wasm guest on the engine's WebAssembly executor — the portable
    /// form, what a robot would be shipped.
    Wasm,
    /// In-process, the same crate linked into the host — for debugging and
    /// speed of iteration.
    Native,
}

/// The walking command, as the keys the trees bind (`F64`, the number an
/// editor writes).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Command {
    pub vx: f64,
    pub vy: f64,
    pub vyaw: f64,
}

/// What to build.
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    pub model: PathBuf,
    /// The Groot XML of the behavior.
    pub tree: String,
    pub executor: Executor,
    pub clock: Clock,
    pub record: Option<PathBuf>,
    pub command: Command,
    /// Publish the joints and the base pose under the ids Semio Studio
    /// drives this robot's model by, instead of the joint names.
    pub studio: Option<StudioModel>,
}

/// The device, with a handle onto its simulation.
pub struct Device {
    arora: Arora,
    sim: SimHandle,
}

/// What a lockstep run observed.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub simulated_seconds: f64,
    /// The base's final position, m.
    pub position: [f32; 3],
    /// Distance travelled in the ground plane from the start, m.
    pub distance: f32,
    /// The lowest base height seen, m (standing is about 0.116).
    pub min_height: f32,
    /// Whether the base ever tipped past 60° from upright.
    pub fell: bool,
}

impl Device {
    /// Build the device for a lockstep run (the configuration's clock
    /// should be [`Clock::Lockstep`]): the simulator on the model, the
    /// modules on the chosen executor, the tree loaded, the command seeded.
    pub fn build(config: DeviceConfig) -> Result<Self> {
        let (builder, sim) = assemble(config)?;
        let arora = builder.build().context("could not build the device")?;
        Ok(Self { arora, sim })
    }

    /// The simulation's handle: ground truth and the lockstep clock.
    pub fn sim(&self) -> &SimHandle {
        &self.sim
    }

    /// The device's blackboard.
    pub fn store(&self) -> &dyn DataStore {
        self.arora.store()
    }

    /// Change the walking command the trees read.
    pub fn set_command(&self, command: Command) -> Result<()> {
        self.arora.store().write(command_change(command))?;
        Ok(())
    }

    /// One lockstep step: the simulator advances one control period, then the
    /// runtime steps once over it (sensors in, tree, targets out).
    pub fn step(&mut self) -> Result<()> {
        self.sim.advance();
        self.arora
            .step(PERIOD)
            .map_err(|e| anyhow!("the runtime step failed: {e}"))?;
        if let Some(error) = self.arora.behavior_error().borrow().as_ref() {
            bail!("the behavior failed: {error}");
        }
        Ok(())
    }

    /// Run in lockstep for `seconds` of simulated time, as fast as the machine
    /// allows, and report what happened.
    pub fn run_for(&mut self, seconds: f64) -> Result<Outcome> {
        let steps = (seconds / PERIOD.as_secs_f64()).round() as usize;
        let start = self.ground_truth().0;
        let mut min_height = f32::INFINITY;
        let mut fell = false;
        for _ in 0..steps {
            self.step()?;
            let (position, gravity_z) = self.ground_truth();
            min_height = min_height.min(position[2]);
            fell |= gravity_z > microduck_policies::FALLEN_GRAVITY_Z;
        }
        let (position, _) = self.ground_truth();
        let dx = position[0] - start[0];
        let dy = position[1] - start[1];
        Ok(Outcome {
            simulated_seconds: steps as f64 * PERIOD.as_secs_f64(),
            position,
            distance: (dx * dx + dy * dy).sqrt(),
            min_height,
            fell,
        })
    }

    /// The base position and the projected gravity's `z` (upright is −1).
    pub fn ground_truth(&self) -> ([f32; 3], f32) {
        let snapshot = self.sim.snapshot();
        let position = match snapshot.get(&Key::from("sim/base.position")) {
            Some(Some(Value::ArrayF32(p))) if p.len() == 3 => [p[0], p[1], p[2]],
            _ => [0.0; 3],
        };
        let gravity_z = match snapshot.get(&Key::from("sim/base.orientation")) {
            Some(Some(Value::ArrayF32(q))) if q.len() == 4 => {
                projected_gravity([q[0], q[1], q[2], q[3]])[2]
            }
            _ => -1.0,
        };
        (position, gravity_z)
    }
}

/// Serve the device until the process stops, on the standard Arora run: the
/// simulator paces itself at real time, the runtime steps at the policies'
/// period, and the bridge is the build's default — Semio Studio under the
/// `studio` feature (the operator is asked for the owner, the device name and
/// the model family, or they come from `DEVICE_OWNERS`, `DEVICE_NAME`,
/// `MODEL_FAMILY`), the open local bridge on `ws://127.0.0.1:9000`
/// otherwise.
pub async fn serve(config: DeviceConfig) -> Result<()> {
    let (builder, _sim) = assemble(config)?;
    builder.with_step_period(PERIOD).run().await
}

/// The device's parts on a builder: the simulator on the model, the modules
/// on the chosen executor, the tree, the seeded commands. The simulator's
/// joint order is checked against the policies' before anything runs.
fn assemble(config: DeviceConfig) -> Result<(AroraBuilder, SimHandle)> {
    let keys: Vec<String> = match &config.studio {
        Some(model) => model.joint_ids(&microduck_policies::JOINT_NAMES)?,
        None => microduck_policies::JOINT_NAMES
            .iter()
            .map(|name| name.to_string())
            .collect(),
    };
    let hal = MujocoHal::new(MujocoHalConfig {
        model: config.model.clone(),
        joints: microduck_policies::JOINT_NAMES
            .iter()
            .zip(&keys)
            .map(|(name, key)| JointSpec {
                key: Some(key.clone()),
                ..JointSpec::named(*name)
            })
            .collect(),
        imu: Some(ImuSpec {
            gyro: "imu_ang_vel".to_string(),
            accelerometer: "imu_accel".to_string(),
            // The IMU is rigid on the trunk: its orientation is the base's.
            orientation: None,
        }),
        contacts: Vec::new(),
        base_body: Some("trunk_base".to_string()),
        base_pose_keys: config.studio.as_ref().and_then(|model| model.base.clone()),
        keyframe: Some("STAND".to_string()),
        // The policies' physics step (the model sets none).
        timestep: Some(0.005),
        control_hz: 50.0,
        clock: config.clock,
        record: config.record.clone(),
        description: HalDescription {
            model_family: Some("microduck".to_string()),
            hardware_version: Some("mujoco".to_string()),
            software_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        },
    })
    .map_err(|e| {
        anyhow!(
            "could not start the simulator on {}: {e}",
            config.model.display()
        )
    })?;
    let sim = hal.handle();
    if sim.joint_names() != keys.as_slice() {
        bail!(
            "the simulator's joints {:?} are not the policies' {:?}",
            sim.joint_names(),
            keys
        );
    }

    let store = SimpleDataStore::new();
    store.write(command_change(config.command))?;
    store.write(StateChange::set(
        "command.head",
        Value::ArrayF32(vec![0.0; 4]),
    ))?;
    store.write(StateChange::set(
        "command.behavior",
        Value::String("walk".to_string()),
    ))?;

    let mut builder = Arora::builder()
        .with_data_store(Box::new(store))
        .with_hal(Box::new(hal));
    builder = match config.executor {
        Executor::Wasm => builder
            .with_declared_module::<Policies>(wasm_executor(), POLICY_WASM)
            .with_declared_module::<blackboard::blackboard::Module>(
                wasm_executor(),
                BLACKBOARD_WASM,
            ),
        Executor::Native => builder
            .with_host_module(HostModule::of::<Policies>())
            .with_host_module(HostModule::of::<blackboard::blackboard::Module>()),
    };
    Ok((builder.with_groot(config.tree), sim))
}

/// The engine's WebAssembly executor, as a module header names it.
fn wasm_executor() -> arora_types::module::low::Executor {
    arora_types::module::low::Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    }
}

/// The store change carrying a command under the keys the trees bind.
fn command_change(command: Command) -> StateChange {
    let mut change = StateChange::new();
    change
        .set
        .insert(Key::from("command.vx"), Some(Value::F64(command.vx)));
    change
        .set
        .insert(Key::from("command.vy"), Some(Value::F64(command.vy)));
    change
        .set
        .insert(Key::from("command.vyaw"), Some(Value::F64(command.vyaw)));
    change
}
