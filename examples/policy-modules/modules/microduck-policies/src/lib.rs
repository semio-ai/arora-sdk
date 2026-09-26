//! Microduck's learned policies as an Arora module.
//!
//! Each exported function is a behavior leaf: the tree binds its inputs to
//! the robot's sensor keys and its `targets` output to the actuation key, and
//! re-ticks it every step while it returns `Running`. Inside, the leaf keeps
//! the observation the network was trained on, runs it at the network's own
//! 50 Hz whatever rate the tree ticks at, and turns the action into joint
//! targets the way the robot's daemon does. The networks are Pollen
//! Robotics' published ONNX files, embedded at build time.
//!
//! The contract the module implements (observation layout, joint order,
//! default pose, scale and filters) is documented in
//! [`docs/study.md`](../../../docs/study.md#microduck); [`docs/how-it-works.md`](../../../docs/how-it-works.md)
//! explains the pattern, and [`docs/reproducing.md`](../../../docs/reproducing.md)
//! what to change for another robot.

use std::sync::Mutex;

use arora_behavior::Status;
use arora_policy::{projected_gravity, Decimator, OnnxPolicy};

/// The policies' joint order — the MJCF actuator order and the order the
/// device's HAL must publish `joints.position` in. The daemon's own wire order
/// inserts a mouth at index 9; the policies never see it.
pub const JOINT_NAMES: [&str; 14] = [
    "left_hip_yaw",
    "left_hip_roll",
    "left_hip_pitch",
    "left_knee",
    "left_ankle",
    "neck_pitch",
    "head_pitch",
    "head_yaw",
    "head_roll",
    "right_hip_yaw",
    "right_hip_roll",
    "right_hip_pitch",
    "right_knee",
    "right_ankle",
];

/// The standing pose the actions are offsets from (the `STAND` keyframe).
pub const DEFAULT_POSE: [f32; 14] = [
    0.0, -0.0873, -0.4579, -0.0049, 0.4530, // left leg
    0.3491, 0.3491, 0.0, 0.0, // neck pitch, head pitch, yaw, roll
    0.0, 0.0873, 0.4579, 0.0049, -0.4530, // right leg
];

/// The network's control period.
pub const CONTROL_PERIOD_NS: u64 = 20_000_000;

/// Below this twist magnitude the walking gait hands over to the standing
/// network, as the daemon does.
const STANDING_THRESHOLD: f32 = 0.05;

/// Projected gravity `z` above which the robot is on its side or back
/// (upright is −1) — the daemon's fall threshold, applied here without its
/// 200 ms debounce.
pub const FALLEN_GRAVITY_Z: f32 = -0.5;

/// Not ticked for this long, the module starts over — the previous action,
/// the filter anchor and the smoothed command are those of a run that ended.
/// The daemon resets its controller after the same pause.
const RESET_AFTER_PAUSE_NS: u64 = 200_000_000;

/// The daemon's command smoothing: each control tick the twist and head move
/// this fraction of the way to what was requested.
const COMMAND_ALPHA: f32 = 0.2;

const OBS_LEN: usize = 61;
const ACTION_LEN: usize = 14;
const HEAD: std::ops::Range<usize> = 5..9;

/// The daemon's deployment tuning: how an action becomes targets. The scale
/// and the filters are the daemon's; the servo gain it also switches (200
/// walking, 160 standing) is fixed in the MJCF and not reproduced.
struct Tuning {
    action_scale: f32,
    /// First-order low-pass on the leg targets (`None` for none).
    legs_lowpass: Option<f32>,
    head_lowpass: Option<f32>,
}

const WALKING: Tuning = Tuning {
    action_scale: 0.9,
    legs_lowpass: Some(0.7),
    head_lowpass: Some(0.5),
};
const STANDING: Tuning = Tuning {
    action_scale: 1.0,
    legs_lowpass: Some(0.7),
    head_lowpass: Some(0.5),
};

/// The networks, by role.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Net {
    Walking,
    Standing,
    SitStand,
    GroundPick,
    Roulade,
    KickLeft,
    KickRight,
}

impl Net {
    fn bytes(self) -> &'static [u8] {
        macro_rules! policy {
            ($name:literal) => {
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../assets/microduck/policies/",
                    $name,
                    ".onnx"
                ))
            };
        }
        match self {
            Net::Walking => policy!("alpha_walking"),
            Net::Standing => policy!("alpha_stand"),
            Net::SitStand => policy!("alpha_sitstand"),
            Net::GroundPick => policy!("alpha_ground_pick"),
            Net::Roulade => policy!("roulade"),
            Net::KickLeft => policy!("ball_kick_left"),
            Net::KickRight => policy!("ball_kick_right"),
        }
    }
}

/// An episodic skill: which network, for how long, and what command it sees.
struct Skill {
    net: Net,
    duration_ns: u64,
    command: SkillCommand,
}

enum SkillCommand {
    /// A zero command throughout.
    Zero,
    /// `twist = [cos 2πφ, sin 2πφ, 0]`, φ advancing over `period_ns`; the
    /// skill ends when φ reaches `end_phase` (the ground pick).
    Phase { period_ns: u64, end_phase: f32 },
}

fn skill_named(name: &str) -> Option<Skill> {
    let seconds = |s: f32| (s * 1e9) as u64;
    Some(match name {
        "kick_left" => Skill {
            net: Net::KickLeft,
            duration_ns: seconds(0.5),
            command: SkillCommand::Zero,
        },
        "kick_right" => Skill {
            net: Net::KickRight,
            duration_ns: seconds(0.5),
            command: SkillCommand::Zero,
        },
        "roulade" => Skill {
            net: Net::Roulade,
            duration_ns: seconds(1.0),
            command: SkillCommand::Zero,
        },
        "ground_pick" => Skill {
            net: Net::GroundPick,
            duration_ns: seconds(2.8),
            command: SkillCommand::Phase {
                period_ns: seconds(4.0),
                end_phase: 0.7,
            },
        },
        _ => return None,
    })
}

/// What the tree hands a policy leaf every tick.
struct Inputs<'a> {
    dt_ns: u64,
    gyro: &'a [f32],
    orientation: &'a [f32],
    joint_positions: &'a [f32],
    joint_velocities: &'a [f32],
}

impl Inputs<'_> {
    fn check(&self) -> Result<(), &'static str> {
        if self.gyro.len() != 3 {
            return Err("gyro must have 3 values");
        }
        if self.orientation.len() != 4 {
            return Err("orientation must be a quaternion [w, x, y, z]");
        }
        if self.joint_positions.len() != ACTION_LEN || self.joint_velocities.len() != ACTION_LEN {
            return Err("joint positions and velocities must have 14 values, in the policy order");
        }
        Ok(())
    }
}

/// The 13-value command block: twist, head, body pose.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Command {
    twist: [f32; 3],
    head: [f32; 4],
    body_z: f32,
    body_roll: f32,
    body_pitch: f32,
}

impl Command {
    /// The daemon's smoothing toward `target`: the twist and the head move a
    /// fraction of the way each control tick; the body pose is taken as is.
    fn smoothed_toward(self, target: Command) -> Command {
        let mut next = target;
        for i in 0..3 {
            next.twist[i] = self.twist[i] + COMMAND_ALPHA * (target.twist[i] - self.twist[i]);
        }
        for i in 0..4 {
            next.head[i] = self.head[i] + COMMAND_ALPHA * (target.head[i] - self.head[i]);
        }
        next
    }
}

/// What persists across leaves and networks: the controller's memory, the
/// daemon's `Controller` state. The previous action feeds every network's
/// observation, the previous targets anchor the low-pass, the smoothed command
/// is what the gaits see. Reset only after a pause.
struct Memory {
    last_action: [f32; ACTION_LEN],
    previous_targets: Option<[f32; ACTION_LEN]>,
    command: Command,
    last_tick_ns: u64,
}

/// One run of a leaf: its clock and its held targets. Replaced when another
/// leaf takes over; the memory above carries across.
struct Run {
    leaf: Leaf,
    net: Net,
    decimator: Decimator,
    /// Time in the run, on the module's own clock (the sum of every `dt` it
    /// was handed).
    elapsed_ns: u64,
    targets: [f32; ACTION_LEN],
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Leaf {
    Walk,
    Stand,
    Sit,
    Rise,
    Skill(String),
}

/// The module's state: the networks (loaded on first use), the controller
/// memory, and the current run.
struct Runtime {
    policies: Vec<(Net, OnnxPolicy)>,
    memory: Option<Memory>,
    run: Option<Run>,
    clock_ns: u64,
}

static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
    policies: Vec::new(),
    memory: None,
    run: None,
    clock_ns: 0,
});

impl Runtime {
    fn policy(&mut self, net: Net) -> Result<&OnnxPolicy, String> {
        if !self.policies.iter().any(|(n, _)| *n == net) {
            let policy = OnnxPolicy::from_bytes(net.bytes())
                .map_err(|e| format!("could not load the {net:?} policy: {e}"))?;
            if policy.input_len() != OBS_LEN || policy.output_len() != ACTION_LEN {
                return Err(format!(
                    "the {net:?} policy takes {} → {} values, not 61 → 14",
                    policy.input_len(),
                    policy.output_len()
                ));
            }
            self.policies.push((net, policy));
        }
        Ok(&self
            .policies
            .iter()
            .find(|(n, _)| *n == net)
            .expect("just loaded")
            .1)
    }

    /// Advance the clock by `dt_ns` and settle the memory and the run for this
    /// tick of `leaf`: after a pause both start over; a different leaf starts
    /// a fresh run over the same memory; the same leaf continues.
    fn tick(&mut self, leaf: Leaf, net: Net, dt_ns: u64) -> (&mut Memory, &mut Run) {
        self.clock_ns = self.clock_ns.saturating_add(dt_ns);
        let now = self.clock_ns;
        let paused = match &self.memory {
            Some(memory) => now.saturating_sub(memory.last_tick_ns) > RESET_AFTER_PAUSE_NS,
            None => true,
        };
        if paused {
            self.memory = Some(Memory {
                last_action: [0.0; ACTION_LEN],
                previous_targets: None,
                command: Command::default(),
                last_tick_ns: now,
            });
            self.run = None;
        }
        let memory = self.memory.as_mut().expect("just ensured");
        memory.last_tick_ns = now;
        let continues = matches!(&self.run, Some(run) if run.leaf == leaf);
        if !continues {
            self.run = Some(Run {
                leaf,
                net,
                decimator: Decimator::new(CONTROL_PERIOD_NS),
                elapsed_ns: 0,
                targets: memory.previous_targets.unwrap_or(DEFAULT_POSE),
            });
        }
        let run = self.run.as_mut().expect("just ensured");
        // The shipped networks are feed-forward: switching one for another
        // resets nothing. (A recurrent network would clear its state here.)
        run.net = net;
        run.elapsed_ns = run.elapsed_ns.saturating_add(dt_ns);
        (memory, run)
    }
}

/// The 61-value observation the networks take.
fn observation(
    inputs: &Inputs,
    last_action: &[f32; ACTION_LEN],
    command: &Command,
) -> [f32; OBS_LEN] {
    let mut obs = [0.0f32; OBS_LEN];
    obs[0..3].copy_from_slice(inputs.gyro);
    let q = [
        inputs.orientation[0],
        inputs.orientation[1],
        inputs.orientation[2],
        inputs.orientation[3],
    ];
    obs[3..6].copy_from_slice(&projected_gravity(q));
    for i in 0..ACTION_LEN {
        obs[6 + i] = inputs.joint_positions[i] - DEFAULT_POSE[i];
        obs[20 + i] = inputs.joint_velocities[i];
        obs[34 + i] = last_action[i];
    }
    obs[48..51].copy_from_slice(&command.twist);
    obs[51..55].copy_from_slice(&command.head);
    // 55, 56: body x, y — always zero.
    obs[57] = command.body_z;
    obs[58] = command.body_roll;
    obs[59] = command.body_pitch;
    // 60: body yaw — always zero.
    obs
}

/// Targets from an action, the daemon's way: the default pose plus the scaled
/// action, then a low-pass toward the previous targets.
fn targets_from(
    action: &[f32; ACTION_LEN],
    previous: Option<[f32; ACTION_LEN]>,
    tuning: &Tuning,
) -> [f32; ACTION_LEN] {
    let mut targets = [0.0f32; ACTION_LEN];
    for i in 0..ACTION_LEN {
        targets[i] = DEFAULT_POSE[i] + tuning.action_scale * action[i];
    }
    if let Some(previous) = previous {
        for i in 0..ACTION_LEN {
            let alpha = if HEAD.contains(&i) {
                tuning.head_lowpass
            } else {
                tuning.legs_lowpass
            };
            if let Some(alpha) = alpha {
                targets[i] = alpha * targets[i] + (1.0 - alpha) * previous[i];
            }
        }
    }
    targets
}

/// Whether a leaf's command goes through the daemon's smoothing (a client's
/// twist and head do; a skill's fixed or phase-encoded command does not).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Smoothing {
    Client,
    Raw,
}

/// One tick of a policy leaf: infer when the control period is due, hold the
/// last targets otherwise; write the targets out. `command` is built from the
/// time elapsed in the run, so a phase-encoded skill sees its own clock.
/// Returns that elapsed time.
fn step(
    runtime: &mut Runtime,
    leaf: Leaf,
    net: Net,
    tuning: &Tuning,
    inputs: &Inputs,
    command: &dyn Fn(u64) -> Command,
    smoothing: Smoothing,
    targets: &mut Vec<f32>,
) -> Result<u64, String> {
    inputs.check().map_err(String::from)?;
    let (memory, run) = runtime.tick(leaf, net, inputs.dt_ns);
    let due = run.decimator.due(inputs.dt_ns);
    let elapsed = run.elapsed_ns;
    if due {
        let requested = command(elapsed);
        let effective = match smoothing {
            Smoothing::Client => memory.command.smoothed_toward(requested),
            Smoothing::Raw => requested,
        };
        memory.command = effective;
        let obs = observation(inputs, &memory.last_action, &effective);
        let previous = memory.previous_targets;
        let action = runtime
            .policy(net)?
            .infer(&obs)
            .map_err(|e| e.to_string())?;
        let mut action_array = [0.0f32; ACTION_LEN];
        action_array.copy_from_slice(&action);
        let new_targets = targets_from(&action_array, previous, tuning);
        let memory = runtime.memory.as_mut().expect("settled by tick");
        memory.last_action = action_array;
        memory.previous_targets = Some(new_targets);
        runtime.run.as_mut().expect("settled by tick").targets = new_targets;
    }
    let run = runtime.run.as_ref().expect("settled by tick");
    targets.clear();
    targets.extend_from_slice(&run.targets);
    Ok(elapsed)
}

fn head_array(head: &[f32]) -> Result<[f32; 4], String> {
    if head.len() != 4 {
        return Err("head must have 4 values: neck_pitch, head_pitch, head_yaw, head_roll".into());
    }
    Ok([head[0], head[1], head[2], head[3]])
}

fn failing(message: String) -> Status {
    // A leaf cannot return the reason; the tree sees the failure and the
    // reason goes to stderr (the host's log on native, the console in wasm).
    eprintln!("microduck-policies: {message}");
    Status::Failure
}

#[arora_module::module(
    id = "5c0a2e7f-6b3d-4d9a-9a41-2f6c7d1e8b90",
    name = "microduck-policies",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "Microduck's learned locomotion policies as behavior leaves",
    executable_mime = "application/wasm"
)]
pub mod microduck_policies {
    use super::*;

    /// Walk at a commanded twist with the `alpha_walking` network, standing
    /// still with `alpha_stand` when the twist is at most 0.05, the twist and
    /// head smoothed as the daemon smooths a client's. Runs until the tree
    /// stops ticking it. (`velstand`, the robot's default gait since its v5
    /// policies, walks and stands with one network; it drifts sideways in
    /// this simulator, so the pair is used.)
    ///
    /// `vx`, `vy` in m/s (x forward, y left), `vyaw` in rad/s (positive turns
    /// left); the policies do not walk visibly below about 0.25 m/s in this
    /// simulator. `head` is the four head angles the policy holds while it
    /// balances. `dt_ns` is the tick period (`arora/dt`); `gyro` (3, rad/s),
    /// `orientation` (a world-from-trunk quaternion `[w, x, y, z]`),
    /// `joint_positions` and `joint_velocities` (14, in the policy order) are
    /// the robot's sensors; `targets` receives the 14 joint setpoints.
    #[export(id = "a3d1f0c2-9b8e-4f6a-8c5d-1e2f3a4b5c6d")]
    pub fn walk(
        #[param(id = "0f1e2d3c-4b5a-4968-8776-655443322110")] vx: f32,
        #[param(id = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d")] vy: f32,
        #[param(id = "2b3c4d5e-6f7a-4b8c-9d0e-1f2a3b4c5d6e")] vyaw: f32,
        #[param(id = "3c4d5e6f-7a8b-4c9d-8e1f-2a3b4c5d6e7f")] head: Vec<f32>,
        #[param(id = "4d5e6f7a-8b9c-4d0e-9f2a-3b4c5d6e7f80")] dt_ns: u64,
        #[param(id = "5e6f7a8b-9c0d-4e1f-8a3b-4c5d6e7f8091")] gyro: Vec<f32>,
        #[param(id = "6f7a8b9c-0d1e-4f2a-9b4c-5d6e7f8091a2")] orientation: Vec<f32>,
        #[param(id = "7a8b9c0d-1e2f-4a3b-8c5d-6e7f8091a2b3")] joint_positions: Vec<f32>,
        #[param(id = "8b9c0d1e-2f3a-4b4c-9d6e-7f8091a2b3c4")] joint_velocities: Vec<f32>,
        #[param(id = "9c0d1e2f-3a4b-4c5d-8e7f-8091a2b3c4d5")] targets: &mut Vec<f32>,
    ) -> Status {
        let head = match head_array(&head) {
            Ok(head) => head,
            Err(e) => return failing(e),
        };
        let twist = [vx, vy, vyaw];
        let magnitude = (vx * vx + vy * vy + vyaw * vyaw).sqrt();
        let (net, tuning) = if magnitude <= STANDING_THRESHOLD {
            (Net::Standing, &STANDING)
        } else {
            (Net::Walking, &WALKING)
        };
        let command = Command {
            twist,
            head,
            ..Command::default()
        };
        let inputs = Inputs {
            dt_ns,
            gyro: &gyro,
            orientation: &orientation,
            joint_positions: &joint_positions,
            joint_velocities: &joint_velocities,
        };
        let mut runtime = RUNTIME.lock().unwrap();
        match step(
            &mut runtime,
            Leaf::Walk,
            net,
            tuning,
            &inputs,
            &|_| command,
            Smoothing::Client,
            targets,
        ) {
            Ok(_) => Status::Running,
            Err(e) => failing(e),
        }
    }

    /// Stand and balance, holding the given head angles (offsets from the
    /// default head pose). Runs until the tree stops ticking it.
    #[export(id = "b4e2a1d3-0c9f-4a7b-9d6e-2f3a4b5c6d7e")]
    pub fn stand(
        #[param(id = "3c4d5e6f-7a8b-4c9d-8e1f-2a3b4c5d6e7f")] head: Vec<f32>,
        #[param(id = "4d5e6f7a-8b9c-4d0e-9f2a-3b4c5d6e7f80")] dt_ns: u64,
        #[param(id = "5e6f7a8b-9c0d-4e1f-8a3b-4c5d6e7f8091")] gyro: Vec<f32>,
        #[param(id = "6f7a8b9c-0d1e-4f2a-9b4c-5d6e7f8091a2")] orientation: Vec<f32>,
        #[param(id = "7a8b9c0d-1e2f-4a3b-8c5d-6e7f8091a2b3")] joint_positions: Vec<f32>,
        #[param(id = "8b9c0d1e-2f3a-4b4c-9d6e-7f8091a2b3c4")] joint_velocities: Vec<f32>,
        #[param(id = "9c0d1e2f-3a4b-4c5d-8e7f-8091a2b3c4d5")] targets: &mut Vec<f32>,
    ) -> Status {
        let head = match head_array(&head) {
            Ok(head) => head,
            Err(e) => return failing(e),
        };
        let command = Command {
            head,
            ..Command::default()
        };
        let inputs = Inputs {
            dt_ns,
            gyro: &gyro,
            orientation: &orientation,
            joint_positions: &joint_positions,
            joint_velocities: &joint_velocities,
        };
        let mut runtime = RUNTIME.lock().unwrap();
        match step(
            &mut runtime,
            Leaf::Stand,
            Net::Standing,
            &STANDING,
            &inputs,
            &|_| command,
            Smoothing::Client,
            targets,
        ) {
            Ok(_) => Status::Running,
            Err(e) => failing(e),
        }
    }

    /// Sit down with the sit/stand network, head and body neutral, for 3 s,
    /// then succeed; the targets last written hold the seat.
    #[export(id = "c5f3b2e4-1d0a-4b8c-8e7f-3a4b5c6d7e8f")]
    pub fn sit(
        #[param(id = "4d5e6f7a-8b9c-4d0e-9f2a-3b4c5d6e7f80")] dt_ns: u64,
        #[param(id = "5e6f7a8b-9c0d-4e1f-8a3b-4c5d6e7f8091")] gyro: Vec<f32>,
        #[param(id = "6f7a8b9c-0d1e-4f2a-9b4c-5d6e7f8091a2")] orientation: Vec<f32>,
        #[param(id = "7a8b9c0d-1e2f-4a3b-8c5d-6e7f8091a2b3")] joint_positions: Vec<f32>,
        #[param(id = "8b9c0d1e-2f3a-4b4c-9d6e-7f8091a2b3c4")] joint_velocities: Vec<f32>,
        #[param(id = "9c0d1e2f-3a4b-4c5d-8e7f-8091a2b3c4d5")] targets: &mut Vec<f32>,
    ) -> Status {
        posture(
            Leaf::Sit,
            1.0,
            dt_ns,
            &gyro,
            &orientation,
            &joint_positions,
            &joint_velocities,
            targets,
        )
    }

    /// Rise from a seat with the sit/stand network, head and body neutral,
    /// for 3 s, then succeed.
    #[export(id = "d6a4c3f5-2e1b-4c9d-9f80-4b5c6d7e8f90")]
    pub fn rise(
        #[param(id = "4d5e6f7a-8b9c-4d0e-9f2a-3b4c5d6e7f80")] dt_ns: u64,
        #[param(id = "5e6f7a8b-9c0d-4e1f-8a3b-4c5d6e7f8091")] gyro: Vec<f32>,
        #[param(id = "6f7a8b9c-0d1e-4f2a-9b4c-5d6e7f8091a2")] orientation: Vec<f32>,
        #[param(id = "7a8b9c0d-1e2f-4a3b-8c5d-6e7f8091a2b3")] joint_positions: Vec<f32>,
        #[param(id = "8b9c0d1e-2f3a-4b4c-9d6e-7f8091a2b3c4")] joint_velocities: Vec<f32>,
        #[param(id = "9c0d1e2f-3a4b-4c5d-8e7f-8091a2b3c4d5")] targets: &mut Vec<f32>,
    ) -> Status {
        posture(
            Leaf::Rise,
            0.0,
            dt_ns,
            &gyro,
            &orientation,
            &joint_positions,
            &joint_velocities,
            targets,
        )
    }

    /// Perform an episodic skill — `kick_left`, `kick_right`, `roulade`,
    /// `ground_pick` — for its trained duration, then succeed. An unknown
    /// skill fails. (The parameter is `skill`, not `name`: Groot reserves
    /// `name` for the node itself.)
    #[export(id = "e7b5d4a6-3f2c-4d0e-8a91-5c6d7e8f90a1")]
    pub fn skill(
        #[param(id = "f8c6e5b7-4a3d-4e1f-9ba2-6d7e8f90a1b2")] skill: String,
        #[param(id = "4d5e6f7a-8b9c-4d0e-9f2a-3b4c5d6e7f80")] dt_ns: u64,
        #[param(id = "5e6f7a8b-9c0d-4e1f-8a3b-4c5d6e7f8091")] gyro: Vec<f32>,
        #[param(id = "6f7a8b9c-0d1e-4f2a-9b4c-5d6e7f8091a2")] orientation: Vec<f32>,
        #[param(id = "7a8b9c0d-1e2f-4a3b-8c5d-6e7f8091a2b3")] joint_positions: Vec<f32>,
        #[param(id = "8b9c0d1e-2f3a-4b4c-9d6e-7f8091a2b3c4")] joint_velocities: Vec<f32>,
        #[param(id = "9c0d1e2f-3a4b-4c5d-8e7f-8091a2b3c4d5")] targets: &mut Vec<f32>,
    ) -> Status {
        let name = skill;
        let Some(skill) = skill_named(&name) else {
            return failing(format!("unknown skill {name:?}"));
        };
        let inputs = Inputs {
            dt_ns,
            gyro: &gyro,
            orientation: &orientation,
            joint_positions: &joint_positions,
            joint_velocities: &joint_velocities,
        };
        let mut runtime = RUNTIME.lock().unwrap();
        let command = |elapsed_ns: u64| match skill.command {
            SkillCommand::Zero => Command::default(),
            SkillCommand::Phase { period_ns, .. } => {
                let phase = elapsed_ns as f32 / period_ns as f32;
                let angle = std::f32::consts::TAU * phase;
                Command {
                    twist: [angle.cos(), angle.sin(), 0.0],
                    ..Command::default()
                }
            }
        };
        match step(
            &mut runtime,
            Leaf::Skill(name),
            skill.net,
            &STANDING,
            &inputs,
            &command,
            Smoothing::Raw,
            targets,
        ) {
            Ok(elapsed) => {
                let done = match skill.command {
                    SkillCommand::Zero => elapsed >= skill.duration_ns,
                    SkillCommand::Phase {
                        period_ns,
                        end_phase,
                    } => {
                        elapsed >= skill.duration_ns
                            || elapsed as f32 / period_ns as f32 >= end_phase
                    }
                };
                if done {
                    Status::Success
                } else {
                    Status::Running
                }
            }
            Err(e) => failing(e),
        }
    }

    /// Succeeds when the robot is down (projected gravity `z` above −0.5),
    /// fails while it is upright — a condition leaf, instantaneous where the
    /// daemon debounces its verdict for 200 ms.
    #[export(id = "09d7f6c8-5b4e-4f2a-8cb3-7e8f90a1b2c3")]
    pub fn fallen(
        #[param(id = "6f7a8b9c-0d1e-4f2a-9b4c-5d6e7f8091a2")] orientation: Vec<f32>,
    ) -> Status {
        if orientation.len() != 4 {
            return failing("orientation must be a quaternion [w, x, y, z]".into());
        }
        let g = projected_gravity([
            orientation[0],
            orientation[1],
            orientation[2],
            orientation[3],
        ]);
        if g[2] > FALLEN_GRAVITY_Z {
            Status::Success
        } else {
            Status::Failure
        }
    }

    /// Play a head animation into `head` — the four angles the balancing
    /// leaves hold — over `dt_ns` ticks: `nod`, `shake`, `look_around`, or
    /// `rest`. Succeeds when the clip ends (`rest` at once).
    #[export(id = "1ae8a7d9-6c5f-4a3b-9dc4-8f90a1b2c3d4")]
    pub fn head_clip(
        #[param(id = "2bf9b8ea-7d60-4b4c-8ed5-90a1b2c3d4e5")] clip: String,
        #[param(id = "4d5e6f7a-8b9c-4d0e-9f2a-3b4c5d6e7f80")] dt_ns: u64,
        #[param(id = "3c4d5e6f-7a8b-4c9d-8e1f-2a3b4c5d6e7f")] head: &mut Vec<f32>,
    ) -> Status {
        super::play_head_clip(&clip, dt_ns, head)
    }

    /// The joint order the policies expect — what `joint_positions`,
    /// `joint_velocities` and `targets` are laid out in.
    #[export(id = "4cfab9ec-8e71-4c5d-9fe6-a1b2c3d4e5f6")]
    pub fn joint_names() -> Vec<String> {
        JOINT_NAMES.iter().map(|s| s.to_string()).collect()
    }
}

/// The sit/stand network with the posture flag in the twist's `vx` slot
/// (sit = 1, stand = 0), head and body zero. Each leaf runs 3 s then
/// succeeds so a sequence can move on: the daemon rises in 1 s and lets a
/// seat settle for 2 s before handing over, and 3 s covers either with
/// margin; the seat itself is held by the targets last written.
#[allow(clippy::too_many_arguments)]
fn posture(
    leaf: Leaf,
    flag: f32,
    dt_ns: u64,
    gyro: &[f32],
    orientation: &[f32],
    joint_positions: &[f32],
    joint_velocities: &[f32],
    targets: &mut Vec<f32>,
) -> Status {
    const POSTURE_NS: u64 = 3_000_000_000;
    let command = Command {
        twist: [flag, 0.0, 0.0],
        ..Command::default()
    };
    let inputs = Inputs {
        dt_ns,
        gyro,
        orientation,
        joint_positions,
        joint_velocities,
    };
    let mut runtime = RUNTIME.lock().unwrap();
    match step(
        &mut runtime,
        leaf,
        Net::SitStand,
        &STANDING,
        &inputs,
        &|_| command,
        Smoothing::Raw,
        targets,
    ) {
        Ok(elapsed) if elapsed >= POSTURE_NS => Status::Success,
        Ok(_) => Status::Running,
        Err(e) => failing(e),
    }
}

/// The head animations: a few keyframed clips over the neutral head pose.
struct HeadClips {
    /// The clip name and its elapsed time, for the clip currently playing.
    playing: Option<(String, u64)>,
}

static HEAD_CLIPS: Mutex<HeadClips> = Mutex::new(HeadClips { playing: None });

fn play_head_clip(clip: &str, dt_ns: u64, head: &mut Vec<f32>) -> Status {
    // The head command is an offset from the default head pose: zero is the
    // pose the standing network holds by itself.
    let neutral = [0.0f32; 4];
    let seconds = |ns: u64| ns as f32 / 1e9;
    let (duration_s, pose): (f32, fn(f32, [f32; 4]) -> [f32; 4]) = match clip {
        "rest" => {
            head.clear();
            head.extend_from_slice(&neutral);
            HEAD_CLIPS.lock().unwrap().playing = None;
            return Status::Success;
        }
        // Two nods: head pitch swings ±0.3 rad.
        "nod" => (2.0, |t, n| {
            [
                n[0],
                n[1] + 0.3 * (std::f32::consts::TAU * t).sin(),
                n[2],
                n[3],
            ]
        }),
        // Two shakes: head yaw swings ±0.6 rad.
        "shake" => (2.0, |t, n| {
            [
                n[0],
                n[1],
                n[2] + 0.6 * (std::f32::consts::TAU * t).sin(),
                n[3],
            ]
        }),
        // A slow look left, right and back, with a slight roll.
        "look_around" => (4.0, |t, n| {
            let s = (std::f32::consts::TAU * t / 4.0).sin();
            [n[0], n[1], n[2] + 1.0 * s, n[3] + 0.2 * s]
        }),
        other => return failing(format!("unknown head clip {other:?}")),
    };
    let mut clips = HEAD_CLIPS.lock().unwrap();
    let elapsed_ns = match &clips.playing {
        Some((name, elapsed)) if name == clip => elapsed.saturating_add(dt_ns),
        _ => 0,
    };
    let t = seconds(elapsed_ns);
    let angles = pose(t.min(duration_s), neutral);
    head.clear();
    head.extend_from_slice(&angles);
    if t >= duration_s {
        clips.playing = None;
        Status::Success
    } else {
        clips.playing = Some((clip.to_string(), elapsed_ns));
        Status::Running
    }
}

#[cfg(test)]
mod tests {
    use super::microduck_policies::*;
    use super::*;

    fn upright() -> Vec<f32> {
        vec![1.0, 0.0, 0.0, 0.0]
    }

    fn at_rest() -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        (vec![0.0; 3], DEFAULT_POSE.to_vec(), vec![0.0; 14])
    }

    /// The module holds one run at a time, so the tests that drive leaves
    /// run one after the other.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// Standing at the default pose, upright and still, the standing policy
    /// writes fourteen finite targets within the servos' travel and keeps
    /// running. (Whether it balances is the device's simulation test.)
    #[test]
    fn stand_writes_targets_and_keeps_running() {
        let _serial = SERIAL.lock().unwrap();
        let (gyro, positions, velocities) = at_rest();
        let mut targets = Vec::new();
        for _ in 0..10 {
            let status = stand(
                vec![0.0; 4],
                CONTROL_PERIOD_NS,
                gyro.clone(),
                upright(),
                positions.clone(),
                velocities.clone(),
                &mut targets,
            );
            assert_eq!(status, Status::Running);
        }
        assert_eq!(targets.len(), 14);
        for target in &targets {
            assert!(
                target.is_finite() && target.abs() < std::f32::consts::PI,
                "{targets:?}"
            );
        }
    }

    /// The walk leaf under a zero twist runs the standing network, under a
    /// real twist the walking one, and hands over between them.
    #[test]
    fn walk_switches_networks_on_the_twist() {
        let _serial = SERIAL.lock().unwrap();
        let (gyro, positions, velocities) = at_rest();
        let mut targets = Vec::new();
        let tick = |vx: f32, targets: &mut Vec<f32>| {
            walk(
                vx,
                0.0,
                0.0,
                vec![0.0; 4],
                CONTROL_PERIOD_NS,
                gyro.clone(),
                upright(),
                positions.clone(),
                velocities.clone(),
                targets,
            )
        };
        assert_eq!(tick(0.0, &mut targets), Status::Running);
        assert_eq!(
            RUNTIME.lock().unwrap().run.as_ref().unwrap().net,
            Net::Standing
        );
        assert_eq!(tick(0.3, &mut targets), Status::Running);
        assert_eq!(
            RUNTIME.lock().unwrap().run.as_ref().unwrap().net,
            Net::Walking
        );
        assert_eq!(targets.len(), 14);
    }

    /// A skill runs for its duration then succeeds; an unknown one fails.
    #[test]
    fn a_skill_ends_after_its_duration() {
        let _serial = SERIAL.lock().unwrap();
        let (gyro, positions, velocities) = at_rest();
        let mut targets = Vec::new();
        let mut ticks = 0;
        loop {
            let status = skill(
                "kick_left".into(),
                CONTROL_PERIOD_NS,
                gyro.clone(),
                upright(),
                positions.clone(),
                velocities.clone(),
                &mut targets,
            );
            ticks += 1;
            if status == Status::Success {
                break;
            }
            assert_eq!(status, Status::Running);
            assert!(ticks < 100, "the kick never ended");
        }
        assert_eq!(ticks, 25, "0.5 s of 20 ms ticks");
        assert_eq!(
            skill(
                "moonwalk".into(),
                CONTROL_PERIOD_NS,
                gyro,
                upright(),
                positions,
                velocities,
                &mut targets
            ),
            Status::Failure
        );
    }

    /// The observation layout, index by index, as the daemon lays it out:
    /// gyro 0..3, gravity 3..6, Δq 6..20, q̇ 20..34, previous action 34..48,
    /// twist 48..51, head 51..55, body x y 55..57 zero, z roll pitch 57..60,
    /// yaw 60 zero.
    #[test]
    fn observation_is_laid_out_as_the_daemon_lays_it_out() {
        let gyro = [0.1, 0.2, 0.3];
        let positions: Vec<f32> = (0..14).map(|i| DEFAULT_POSE[i] + 0.01 * i as f32).collect();
        let velocities: Vec<f32> = (0..14).map(|i| 1.0 + i as f32).collect();
        let last_action: [f32; 14] = std::array::from_fn(|i| -(i as f32));
        let inputs = Inputs {
            dt_ns: CONTROL_PERIOD_NS,
            gyro: &gyro,
            orientation: &[1.0, 0.0, 0.0, 0.0],
            joint_positions: &positions,
            joint_velocities: &velocities,
        };
        let command = Command {
            twist: [0.3, 0.02, -0.5],
            head: [0.1, 0.2, 0.3, 0.4],
            body_z: -0.01,
            body_roll: 0.05,
            body_pitch: 0.06,
        };
        let obs = observation(&inputs, &last_action, &command);
        assert_eq!(obs.len(), 61);
        assert_eq!(&obs[0..3], &gyro);
        assert!((obs[3]).abs() < 1e-6 && (obs[4]).abs() < 1e-6 && (obs[5] + 1.0).abs() < 1e-6);
        for i in 0..14 {
            assert!(
                (obs[6 + i] - 0.01 * i as f32).abs() < 1e-6,
                "Δq at {}",
                6 + i
            );
            assert_eq!(obs[20 + i], 1.0 + i as f32, "q̇ at {}", 20 + i);
            assert_eq!(obs[34 + i], -(i as f32), "previous action at {}", 34 + i);
        }
        assert_eq!(&obs[48..51], &[0.3, 0.02, -0.5]);
        assert_eq!(&obs[51..55], &[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(&obs[55..57], &[0.0, 0.0]);
        assert_eq!(&obs[57..60], &[-0.01, 0.05, 0.06]);
        assert_eq!(obs[60], 0.0);
    }

    /// The daemon's target tuning: default pose plus the scaled action, then
    /// the low-pass toward the previous targets — 0.7 on the legs, 0.5 on the
    /// four head joints (5..9), scale 0.9 walking and 1.0 standing.
    #[test]
    fn targets_follow_the_daemon_tuning() {
        let action = [1.0f32; 14];
        let fresh = targets_from(&action, None, &WALKING);
        for i in 0..14 {
            assert!((fresh[i] - (DEFAULT_POSE[i] + 0.9)).abs() < 1e-6, "{i}");
        }
        let previous = [0.0f32; 14];
        let filtered = targets_from(&action, Some(previous), &STANDING);
        for i in 0..14 {
            let raw = DEFAULT_POSE[i] + 1.0;
            let expected = if (5..9).contains(&i) {
                0.5 * raw
            } else {
                0.7 * raw
            };
            assert!(
                (filtered[i] - expected).abs() < 1e-6,
                "{i}: {} vs {expected}",
                filtered[i]
            );
        }
    }

    /// A client's command is smoothed a fifth of the way per control tick;
    /// the memory carries across leaves and resets after a 200 ms pause.
    #[test]
    fn memory_carries_across_leaves_and_resets_after_a_pause() {
        let _serial = SERIAL.lock().unwrap();
        let (gyro, positions, velocities) = at_rest();
        let mut targets = Vec::new();
        // Start clean: a pause resets everything.
        {
            let mut runtime = RUNTIME.lock().unwrap();
            runtime.clock_ns = runtime.clock_ns.saturating_add(RESET_AFTER_PAUSE_NS + 1);
        }
        walk(
            0.3,
            0.0,
            0.0,
            vec![0.0; 4],
            CONTROL_PERIOD_NS,
            gyro.clone(),
            upright(),
            positions.clone(),
            velocities.clone(),
            &mut targets,
        );
        {
            let runtime = RUNTIME.lock().unwrap();
            let memory = runtime.memory.as_ref().unwrap();
            assert!(
                (memory.command.twist[0] - 0.06).abs() < 1e-6,
                "one fifth of 0.3: {:?}",
                memory.command
            );
            assert!(memory.previous_targets.is_some());
        }
        // Another leaf keeps the memory (the previous action, the anchor).
        let last_action = RUNTIME.lock().unwrap().memory.as_ref().unwrap().last_action;
        stand(
            vec![0.0; 4],
            CONTROL_PERIOD_NS,
            gyro.clone(),
            upright(),
            positions.clone(),
            velocities.clone(),
            &mut targets,
        );
        {
            let runtime = RUNTIME.lock().unwrap();
            assert_eq!(runtime.run.as_ref().unwrap().leaf, Leaf::Stand);
            assert_ne!(runtime.memory.as_ref().unwrap().last_action, [0.0; 14]);
            assert_ne!(
                runtime.memory.as_ref().unwrap().last_action,
                last_action,
                "stand inferred once more"
            );
        }
        // A pause longer than 200 ms starts over.
        stand(
            vec![0.0; 4],
            RESET_AFTER_PAUSE_NS + 1,
            gyro,
            upright(),
            positions,
            velocities,
            &mut targets,
        );
        {
            let runtime = RUNTIME.lock().unwrap();
            let memory = runtime.memory.as_ref().unwrap();
            assert_eq!(memory.command.twist, [0.0; 3]);
            assert_eq!(
                runtime.run.as_ref().unwrap().elapsed_ns,
                RESET_AFTER_PAUSE_NS + 1
            );
        }
    }

    #[test]
    fn fallen_reads_the_orientation() {
        assert_eq!(fallen(upright()), Status::Failure);
        // Lying on the back: rotated 90° about y.
        let half = std::f32::consts::FRAC_PI_4;
        assert_eq!(
            fallen(vec![half.cos(), 0.0, half.sin(), 0.0]),
            Status::Success
        );
        assert_eq!(fallen(vec![1.0, 0.0]), Status::Failure);
    }

    #[test]
    fn a_head_clip_plays_then_rests() {
        let mut head = Vec::new();
        let mut ticks = 0;
        while head_clip("nod".into(), CONTROL_PERIOD_NS, &mut head) == Status::Running {
            ticks += 1;
            assert_eq!(head.len(), 4);
            assert!(ticks <= 100);
        }
        assert_eq!(ticks, 100, "2 s of 20 ms ticks");
        assert_eq!(
            head_clip("rest".into(), CONTROL_PERIOD_NS, &mut head),
            Status::Success
        );
        assert_eq!(head, vec![0.0; 4]);
    }

    #[test]
    fn joint_names_are_the_policy_order() {
        assert_eq!(joint_names().len(), 14);
        assert_eq!(joint_names()[9], "right_hip_yaw");
    }
}
