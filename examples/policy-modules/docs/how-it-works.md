# How a control policy runs inside an Arora module

A learned control policy is a function from an observation vector to an action
vector, evaluated at a fixed rate. This page explains how such a policy lives
in an Arora module, how a behavior tree drives it, and how a tree switches
between policies as the situation changes. The Microduck module
(`modules/microduck-policies`) is the worked example; every design point below
applies to any robot. The [README](../README.md) shows it running.

## The pieces

```
              ┌───────────────────────── device process ─────────────────────────┐
              │                                                                   │
  sensors ──▶ │ HAL ──▶ store ──▶ behavior tree ──call──▶ policy module (wasm)    │
              │  ▲              (leaves bound to keys)         │                  │
  actuators ◀─┤  └──── store ◀── tree writes the outputs ◀─────┘                  │
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
  `Running` is called again next step — that repetition *is* the control loop.
- **The policy module** exports the leaves. Each is an ordinary function whose
  in parameters are the sensors and the command, whose one out parameter
  (`&mut`) is the joint targets, and whose return is a `Status`. The module
  never reads or writes a key: the tree hands it the values its leaf binds,
  and writes its outputs to the keys the leaf names. It embeds the networks
  (ONNX bytes) and keeps the small state a policy needs between calls. Built
  for `wasm32-wasip2` it is a component and runs on the engine's component
  executor; the same crate linked into the host runs in-process
  (`--executor native`). Updating the robot's control policy is shipping a new
  module.

## A policy leaf, step by step

Take `walk`, the leaf that drives the walking network:

```rust
#[export(id = "…")]
pub fn walk(
    vx: f64, vy: f64, vyaw: f64,        // the command
    head: Vec<f32>,                      // more command: head angles
    dt_ns: u64,                          // the step period (arora/dt)
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

The tag is the function's name in PascalCase: the tree resolves `<Walk>` to
`walk` among the functions the device's modules export.

Each step:

1. The runtime publishes its clock (`arora/dt`, nanoseconds since the
   previous step), applies the HAL's latest sensor readings to the store,
   then what clients wrote over a bridge.
2. The tree resolves each `{key}` to the store's value and calls `walk`.
3. Inside, the leaf decides whether a **control period** has elapsed
   (`Decimator`): the network was trained at 50 Hz, the runtime may step at
   any rate, so the leaf infers only when 20 ms have accumulated and holds its
   last targets otherwise. A stalled runtime catches up by one period at most.
4. When due, it builds the **observation** exactly as training did — here
   gyro, projected gravity from the orientation quaternion, joint positions
   minus the default pose, joint velocities, the previous raw action, and the
   13-value command — and runs the network (`OnnxPolicy::infer`).
5. It turns the action into **targets** the way the robot's own controller
   does (`default pose + scale × action`, then a low-pass toward the previous
   targets) and returns them in `targets`.
6. The tree writes `targets` to `joints.target_position`; the runtime flushes
   the step's changes to the HAL, which applies them as the actuators'
   setpoints.
7. The leaf returns `Running`, so the whole thing happens again next step.

Nothing in the loop blocks: inference on a policy of this size takes tens of
microseconds natively and well under a millisecond in wasm.

### Where the state lives

A policy needs memory between calls. The module keeps two things in a
`static`, on the model of the robot's own controller (its daemon):

- **The controller memory** — the previous raw action (an input to every
  network), the previous targets (the low-pass anchor), and the smoothed
  command. It carries across leaves and networks: when a tree moves from
  `sit` to `rise`, the first `rise` observation carries the last `sit` action,
  as the daemon's does. It resets only after a pause of more than 200 ms
  without a call — the daemon resets its controller after the same pause —
  so a branch the tree abandons and comes back to seconds later starts clean.
- **The run** — which leaf is active, its clock (a skill's elapsed time), its
  50 Hz decimator, and the targets it holds between inferences. Another leaf
  starts a fresh run over the same memory.

The shipped networks are feed-forward, so switching networks resets nothing;
a recurrent network would clear its state at the switch, where the daemon
does.

A module runs one body, so one memory and one run at a time is the honest
model. Two leaves of the same function in one tree would share them: the
module ABI hands a call no run identity, the open question the SDK's
[async-functions design](../../../docs/async-functions.md) records.

The networks themselves are loaded lazily from the embedded bytes on first use
and kept: loading is the expensive part (tract type-checks and optimizes the
graph), inference is cheap.

### What crosses the module boundary

Only primitives and arrays: `f32`, `f64`, `u64`, `String`, `Vec<f32>`,
`Vec<String>`, and the `Status` enumeration. That keeps the leaf callable from
any tree editor, any host, and any executor without a shared type registry.
A parameter's type is checked, not converted: an `F64` in the store does not
bind to an `f32` parameter. So a parameter an operator writes takes the type
editors write — `walk`'s velocities are `f64`, the number Semio Studio sends —
and the leaf narrows it for the network. The joint order is the one
convention both sides must agree on: the module exports it (`joint_names`) and
the device refuses to start when the HAL's order differs.

The device loads the module with `with_declared_module::<Module>`, so every
export — the `Status`-returning leaves included — is indexed with the
declaration's signatures and reachable from a tree by name. (`with_module`,
given a bare header and no registry, indexes only the exports whose
parameters and return are primitives.)

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
  longer called — the last targets hold. (The robot has no get-up network; a
  robot that has one puts it in that branch.)
- **The walking leaf itself** switches networks on the command: at a twist
  magnitude of 0.05 or less it runs the standing network, above it the
  walking one, as the robot's daemon does. So "walk + direction" and "stand
  still" are one leaf and one key set from the tree's point of view; a
  command written to `command.vx` from an editor, a script or another leaf
  changes the gait.
- **An animation over a balancing policy** is the `Parallel`. `head_clip`
  (the `HeadClip` leaf) plays a keyframed head animation: each call returns
  four head angles in its out parameter `head`, which the tree writes to
  `command.head`; `Walk`, ticked after it in the same step, reads them as
  part of its command. The head moves through the policy — the network was
  trained to hold commanded head angles while balancing — rather than around
  it, so balance is never fought. A parallel ticks every child each step, and
  a finished animation starts over, so the head keeps looking around for as
  long as the branch is active.
- **Episodic skills** are leaves that end: `Skill skill="kick_left"` runs its
  network for the trained 0.5 s and returns `Success`; `Sit` and `Rise` run
  the sit/stand network for 3 s each (a choice: the daemon rises in 1 s and
  lets a seat settle for 2 s). A `SequenceStar` chains them and resumes past
  the ones that succeeded (`sit_rise` and `kick` trees). Called again after
  its end, an episodic leaf keeps running its network and keeps returning
  `Success`: a seat is held by the sitting network, not by frozen targets.
- **A request from outside** selects a branch in the `interactive` tree. A
  tree's control nodes route on their children's status only, so the
  branch is chosen by Arora's native string nodes: `RegexMatch` turns
  "`command.behavior` is `kick_left`" into a status, and `SetString` writes a
  value back. Each requestable behavior is a branch of the fallback headed by
  a `RegexMatch`:

  ```xml
  <Sequence>
    <RegexMatch value="{command.behavior}" matcher="^kick_left$" first_match="{matched_request}" />
    <Skill skill="kick_left" … />
    <SetString variable="{command.behavior}" value="walk" />
  </Sequence>
  ```

  `RegexMatch` returns what it matched in `first_match`; `{matched_request}`
  names no store key, so that lands in a tree-local variable.

  While the request stands, the sequence re-enters the same skill leaf every
  step, so its run continues; when the skill succeeds, `SetString` hands the
  request back to `walk`, and the next step falls through to the walking
  branch. A branch without a `SetString` (`sit`) holds until something else is
  requested. Whoever writes `command.behavior` — an editor's live data, a
  script on the local bridge, another leaf — switches the policy, and the
  situation checks placed before it (`Fallen`) still take precedence. (The
  shipped `interactive` tree also runs a `Say` beside each policy leaf; see
  [speech](speech.md).)

The same trees run on any Arora that loads this module: the runtime, the
tree interpreter and the module are the same code on a laptop with MuJoCo, a
robot with a hardware HAL, or a browser page — only the HAL differs.

## The reusable pieces

`crates/arora-policy` holds what every policy module needs and no host
provides: `OnnxPolicy` (load once, infer), `History` (the last N vectors, for
policies that take them), `Decimator` (the fixed-rate clock), and
`projected_gravity`. It has no host dependency and builds for wasm unchanged.

`crates/arora-hal-mujoco` holds the simulator side: any MJCF robot under the
standard keys, a real-time or lockstep clock, CSV recording, and the keys an
editor drives its model by (joints under ids of its choosing, the base pose
as six scalars).

What sets this HAL apart from a robot's is that it runs MuJoCo itself, in the
device's process. `MujocoHal::new` loads the MJCF, creates the simulation's
data, puts the robot in its keyframe and resolves every joint, actuator,
sensor and body it was configured with; from then on the HAL owns the model
and the data. Each control period (20 ms here) it applies the latest
setpoints written to it to the position actuators, steps the physics
(four 5 ms steps), and publishes the sensors. Who calls for the next period
is its clock: a thread of its own pacing the periods against the wall clock,
started when the runtime subscribes to the HAL's feed, or, in lockstep, the
embedder calling `advance` between runtime steps, which is what makes a run
deterministic and faster than real time. A `sim/reset` returns the
simulation to its keyframe.

A fork can draw that line elsewhere: a HAL that connects to a MuJoCo
simulation running in another process or on another machine, rather than
creating one, would publish the same keys from what that simulation reports
and send it the setpoints, and nothing above the HAL would change. Its clock
would then be the remote simulation's, so a lockstep run would need that
simulation to step on request. This example does not provide such a HAL.

The device (`crates/arora-microduck`) is built on four SDK seams:
`AroraBuilder::with_declared_module` and Groot tags resolved by function name
let a declared module's leaves be reached from an authored tree;
`AroraBuilder::with_groot` and `with_step_period` let a device carrying such a
tree run on the standard `AroraBuilder::run` (the operator front end, the
Studio or local bridge). The same assembly, built with `build` instead of
`run` and stepped by hand, is the lockstep simulation `--duration` and the
tests use.

## What it costs

Measured on an Apple M1 laptop:

| | |
|---|---|
| showcase tree, 0.3 m/s, 12 simulated s (release, wasm guest) | 1.34 m travelled, upright, 5.2 s of wall time including the wasm compile |
| walk tree, 0.3 m/s, 10 simulated s | 1.16 m travelled, upright |
| served, commanded 0.3 m/s over the local bridge (dev build) | 0.89 m in 8 s; the bridge answers after about 12 s of wasm compile |
| wasm policy guest | 22 MB in the release build (thin LTO, stripped), the ONNX runtime and seven networks inside |
| lockstep speed with the policy in wasm | 6 simulated s in 0.2 s (about 30× real time) |
| `cargo test`, warm | under a minute, twelve simulated runs included |

## Limits worth knowing

- The module keeps one memory and one run; concurrent leaves of one function
  share them.
- Deliberately not reproduced from the daemon: its servo gain schedule (200
  walking, 160 standing; the MJCF fixes the gain), the 200 ms debounce on its
  fall verdict, and head or body commands while seated (`sit` and `rise` hold
  them at zero).
- Groot passes an inline literal to a module leaf as a string; bind numbers
  to keys (`{command.vx}`) rather than writing them inline.
- The Microduck walking networks do not walk visibly below 0.25 m/s in this
  simulator (see the [study](study.md#verified-on-this-machine)); command
  0.25–0.3 m/s.
- The wasm module carries the ONNX runtime and the networks: 22 MB, compiled
  by the engine in seconds at device start (see above).
