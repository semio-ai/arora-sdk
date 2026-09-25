# How a control policy runs inside an Arora module

A learned control policy is a function from an observation vector to an action
vector, evaluated at a fixed rate. This page explains how such a policy lives
in an Arora module, how a behavior tree drives it, and how a tree switches
between policies as the situation changes. The Microduck module
(`modules/microduck-policies`) is the worked example; every design point below
applies to any robot.

## The pieces

```
              ┌───────────────────────── device process ─────────────────────────┐
              │                                                                   │
  sensors ──▶ │ HAL ──▶ store ──▶ behavior tree ──tick──▶ policy module (wasm)    │
              │  ▲              (leaves bound to keys)         │                  │
  actuators ◀─┤  └──── store ◀──── targets written back ◀──────┘                  │
              │                                                                   │
              └───────────────────────────────────────────────────────────────────┘
```

- **The HAL** presents the robot as keys on the shared store: sensors in
  (`joints.position`, `joints.velocity`, `imu.gyro`, `imu.orientation`),
  actuation out (`joints.target_position`). A real robot's HAL and a
  simulator's HAL publish the same keys, so nothing above them knows which is
  running. In this example the HAL is MuJoCo (`crates/arora-hal-mujoco`).
- **The behavior tree** is authored data (a Groot XML file). A leaf names a
  module function and binds each parameter to a store key (`{joints.position}`)
  or a literal. The runtime ticks the tree once per step; a leaf that returns
  `Running` is ticked again next step — that re-ticking *is* the control loop.
- **The policy module** exports the leaves. Each is an ordinary function whose
  inputs are the sensors and the command, whose one `&mut` output is the joint
  targets, and whose return is a `Status`. The module embeds the network
  (ONNX bytes) and keeps the small state a policy needs between ticks. Built
  for `wasm32-wasip1` it runs on the engine's WebAssembly executor; the same
  crate linked into the host runs in-process. Updating the robot's control
  policy is shipping a new module.

## A policy leaf, step by step

Take `walk`, the leaf that drives the walking network:

```rust
#[export(id = "…")]
pub fn walk(
    vx: f32, vy: f32, vyaw: f32,        // the command
    head: Vec<f32>,                      // more command: head angles
    dt_ns: u64,                          // the tick period (arora/dt)
    gyro: Vec<f32>, orientation: Vec<f32>,
    joint_positions: Vec<f32>, joint_velocities: Vec<f32>,
    targets: &mut Vec<f32>,              // the output
) -> Status
```

and its node in a tree:

```xml
<Walk vx="{command.vx}" vy="{command.vy}" vyaw="{command.vyaw}" head="{command.head}"
      dt_ns="{arora/dt}" gyro="{imu.gyro}" orientation="{imu.orientation}"
      joint_positions="{joints.position}" joint_velocities="{joints.velocity}"
      targets="{joints.target_position}" />
```

Each tick:

1. The runtime has already applied the HAL's latest sensor readings to the
   store and published the clock (`arora/dt`, nanoseconds since the previous
   step).
2. The tree resolves each `{key}` to the store's value and calls `walk`.
3. Inside, the leaf decides whether a **control period** has elapsed
   (`Decimator`): the network was trained at 50 Hz, the runtime may tick at
   any rate, so the leaf infers only when 20 ms have accumulated and holds its
   last targets otherwise. A stalled runtime catches up by one period at most.
4. When due, it builds the **observation** exactly as training did — here
   gyro, projected gravity from the orientation quaternion, joint positions
   minus the default pose, joint velocities, the previous raw action, and the
   13-value command — and runs the network (`OnnxPolicy::infer`).
5. It turns the action into **targets** the way the robot's own controller
   does (`default pose + scale × action`, then a low-pass toward the previous
   targets) and writes them into `targets`.
6. The tree writes `targets` back to `joints.target_position`; the runtime
   flushes it to the HAL; the HAL applies it as the actuators' setpoints.
7. The leaf returns `Running`, so the whole thing happens again next step.

Nothing in the loop blocks: inference on a policy of this size takes tens of
microseconds natively and well under a millisecond in wasm.

### Where the state lives

A policy needs memory between ticks: the previous action (an input to the
network), the previous targets (for the filter), the decimator's phase, and a
run's elapsed time. The module keeps one **run** in a `static`. A run belongs
to a leaf: the same leaf ticked again continues it; another leaf, or the same
leaf after a gap of more than three control periods, starts a fresh one. That
gap rule is what makes switching safe: when a tree stops ticking `walk` and
starts ticking `sit`, `sit` begins with a clean history, and if the tree comes
back to `walk` later it begins clean too — exactly what the robot's daemon does
when it switches networks.

A module runs one body, so one run at a time is the honest model. (Two leaves
of the same function in one tree would share it; the module ABI hands a leaf
no run identity, which the SDK's async-functions design notes as the open
question.)

The networks themselves are loaded lazily from the embedded bytes on first use
and kept: loading is the expensive part (tract type-checks and optimizes the
graph), inference is cheap.

### What crosses the module boundary

Only primitives and arrays: `f32`, `u64`, `String`, `Vec<f32>`, `Vec<String>`,
and the `Status` enumeration. That keeps the leaf callable from any tree
editor, any host, and any executor without a shared type registry. The joint
order is the one convention both sides must agree on: the module exports it
(`joint_names`) and the device refuses to start when the HAL's order differs.

The device loads the module with `with_declared_module::<Module>`, so every
export — the `Status`-returning leaves included — is indexed with the
declaration's signatures and reachable from a tree by name. (`with_module`,
which takes a bare header, can index only primitive-typed exports.)

## Switching policies with the tree

The tree is where situations are told apart. The `showcase` tree:

```xml
<Fallback>
  <Sequence>
    <Fallen orientation="{imu.orientation}" />
    <Succeed />
  </Sequence>
  <Parallel>
    <HeadClip clip="look_around" dt_ns="{arora/dt}" head="{command.head}" />
    <Walk … head="{command.head}" … />
  </Parallel>
</Fallback>
```

- **A condition leaf** (`Fallen`) reads a sensor and returns `Success` or
  `Failure`; it drives no actuator. The `Fallback` tries the fallen branch
  first: while the robot is up, `Fallen` fails and the fallback moves on to
  walking; once it is down, the sequence succeeds and the walking leaf is no
  longer ticked — its run goes stale, the last targets hold. (The robot has
  no get-up network; a robot that has one puts it in that branch.)
- **The walking leaf itself** switches networks on the command: at a twist
  magnitude of 0.05 or less it runs the standing network, above it the
  walking one, as the robot's daemon does. So "walk + direction" and "stand
  still" are one leaf and one key set from the tree's point of view; a
  command written to `command.vx` from an editor, a script or another leaf
  changes the gait.
- **An animation over a balancing policy** is the `Parallel`: `HeadClip`
  writes four head angles into `command.head` every tick, and `Walk`, ticked
  after it in the same step, reads them as part of its command. The head
  moves through the policy — the network was trained to hold commanded head
  angles while balancing — rather than around it, so balance is never fought.
  A parallel ticks every child each step, and a finished clip restarts, so
  the head keeps looking around for as long as the branch is active.
- **Episodic skills** are leaves that end: `Skill skill="kick_left"` runs its
  network for the trained 0.5 s and returns `Success`; `Sit` and `Rise` run
  the sit/stand network for 3 s. A `SequenceStar` chains them and resumes
  past the ones that succeeded (`sit_rise` and `kick` trees).

The same trees run on any Arora that loads this module: the runtime, the
tree interpreter and the module are the same code on a laptop with MuJoCo, a
robot with a hardware HAL, or a browser page — only the HAL differs.

## The reusable pieces

`crates/arora-policy` holds what every policy module needs and no host
provides: `OnnxPolicy` (load once, infer), `History` (the last N vectors, for
policies that take them), `Decimator` (the fixed-rate clock), and
`projected_gravity`. It has no host dependency and builds for wasm unchanged.

`crates/arora-hal-mujoco` holds the simulator side: any MJCF robot under the
standard keys, a real-time or lockstep clock, CSV recording.

The two SDK seams this example added — `AroraBuilder::with_declared_module`
and Groot tags resolved by function name — are what let a declared module's
leaves be reached from an authored tree.

## Limits worth knowing

- The module keeps one run; concurrent leaves of one function share it.
- Groot numeric literals are passed as strings; bind numbers to keys
  (`{command.vx}`) rather than writing them inline.
- The Microduck walking networks do not walk visibly below 0.25 m/s in this
  simulator (see the study); command 0.25–0.3 m/s.
- The wasm module carries the ONNX runtime and the networks; see the README
  for its size and load time.
