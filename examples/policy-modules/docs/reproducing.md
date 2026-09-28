# Reproducing this on another robot

What to keep, what to adapt, and what to expect, for someone bringing a
learned policy for a different robot into an Arora module. It assumes the
[how-it-works](how-it-works.md) page.

## What you need from the robot's side

1. **A simulator model** (MJCF for `arora-hal-mujoco`): the robot with
   position actuators on every controlled joint, an IMU site with `gyro` and
   `accelerometer` sensors (and a `framequat` if the IMU is not rigid on the
   base), a keyframe for the start pose. Fidelity matters only where the
   policy is sensitive: actuator gains, joint damping/friction/armature, and
   foot contact parameters should be the ones the policy was trained against.
2. **A policy** in ONNX with a batch of one: one `f32` input `[1, N]`, one
   output `[1, M]`. A PyTorch actor exports with `torch.onnx.export`; a JAX
   one through `jax2tf` + `tf2onnx`. Normalization baked into the graph is
   easiest; otherwise apply it in the leaf. Recurrent policies (LSTM state
   in and out) are not covered by `OnnxPolicy`; tract has the operators, the
   leaf would carry the state between ticks.
3. **The contract**: the exact observation layout (order, units, scaling,
   history length), the action semantics (offset from which default pose,
   scale, clipping or slew limits, filters), the control rate, the joint
   order, and any command encoding. Read it from the deployment code of the
   robot, not from the training config alone — deployment tunings differ
   (Microduck runs scale 0.9 with low-pass filters; training used 1.0 and
   none).
4. **Licenses** that allow you to embed the network and redistribute the
   model.

## What to keep as is

- `crates/arora-policy`: `OnnxPolicy`, `Decimator`, `History`,
  `projected_gravity`. Add to it only what is robot-agnostic.
- `crates/arora-hal-mujoco`: configure it, do not fork it. Its
  `MujocoHalConfig` names the joints (and their actuators), the IMU sensors,
  the touch sensors, the base body, the keyframe, the control rate and the
  clock. If your policy needs a sensor the HAL does not publish (a
  velocimeter, a magnetometer), add a `<sensor>` kind to the HAL as a
  configurable spec rather than a robot-specific key.
- The key conventions: `<joint>.position`, `<joint>.velocity`,
  `<joint>.target_position` per joint (the joint's key entity is its name, or
  the id an editor drives it by), and the `joints.*` aggregates in the HAL's
  declared order; `imu.gyro`, `imu.accelerometer`, `imu.orientation`
  (`[w, x, y, z]`, world from IMU); `sim/*` for what only a simulator knows.
  Leaves read the aggregates, so renaming the per-joint keys changes nothing
  above the HAL.
- The device shape (`crates/device`): HAL + declared modules (wasm or
  in-process) + Groot tree (`with_groot`), built directly for a lockstep run
  and handed to the standard `AroraBuilder::run` for serving — the operator
  front end, and Semio Studio or the local bridge by build feature — with
  the joint-order check at build. Only the HAL is simulation-specific: the
  device for the real robot swaps it and keeps the rest.
- `modules/blackboard` and the `interactive` tree's shape: a fallback of
  situation checks first, then one branch per requestable behavior headed by
  `Equals` on `command.behavior`, `Assign` handing an episodic one back.
- Command parameters an operator writes typed as the editors write them:
  `f64` for numbers (Semio Studio sends every number as one; a type is
  checked, not converted, at the leaf boundary).
- The module shape: one run in a `static`, leaf identity + staleness to
  reset it, the decimator, the observation builder, the targets builder, the
  `Status` semantics (`Running` for gaits, `Success` when an episodic skill
  ends, `Failure` on a bad input).

## What to adapt

Everything robot-specific lives in the module crate, in this order of
likelihood:

1. **`JOINT_NAMES`, `DEFAULT_POSE`, `CONTROL_PERIOD_NS`** — the joint order
   must be the MJCF actuator order your HAL config lists and the order the
   policy's action indexes.
2. **`observation()`** — rebuild it from the robot's deployment code. Watch
   for: scaling factors on velocities (Open Duck Mini scales joint velocities
   by 0.05; Microduck does not), gravity from an accelerometer versus from a
   quaternion, history blocks (use `History`), phase or clock features (a
   `Decimator`-driven counter), command blocks with reserved zeros, and
   biases only present in training (Open Duck Mini adds 1.3 m/s² to the
   accelerometer's x).
3. **`targets_from()`** and the memory rules — the action scale, the default
   pose, slew limits, low-pass filters, gain schedules, command smoothing, and
   what resets when (a network switch, a pause, a fall). A robot whose actuators are torque motors
   in the MJCF needs a PD loop somewhere: put it in the HAL config as a
   position actuator in the MJCF (preferred, it is what the policy saw) or in
   the leaf.
4. **The leaves** — which networks exist and how they are selected: a
   walk/stand pair keyed on the command, episodic skills with durations,
   posture transitions. Keep each leaf's parameters primitive.
5. **The trees** — the situations your robot must tell apart (fallen, low
   battery, an obstacle), and the animation channels the policy accepts as
   commands (head angles here; arm targets on a humanoid whose policy takes
   them).
6. **The assets** — `tools/fetch-assets.sh` pins the sources and revisions;
   the module embeds the policies by path from `assets/`.

## The Microduck specifics, for comparison

| Aspect | Microduck (this example) |
|---|---|
| Joints | 14 (5 per leg, neck pitch, head pitch/yaw/roll); the daemon's wire order adds a mouth at index 9, the policies never see it |
| Observation | 61: gyro 3, projected gravity 3, Δq 14, q̇ 14, previous action 14, command 13 (twist 3, head 4, body x y 0, z, roll, pitch, yaw 0) |
| Action | 14 offsets; targets = default pose + 0.9 (walk) / 1.0 (stand, skills) × action; low-pass 0.7 legs, 0.5 head |
| Rate | 50 Hz over a 5 ms physics step (decimation 4); the MJCF sets no timestep, so the HAL config sets it (MuJoCo's default is 2 ms) |
| Networks | one architecture for every skill (MLP 61→512→256→128→14, ELU); the leaf picks the file |
| Standing switch | twist magnitude ≤ 0.05 → standing network |
| Command | twist and head smoothed a fifth of the way per tick (the daemon's `cmd_alpha`); skills and postures take theirs raw |
| Memory | previous action, filter anchor and smoothed command carry across leaves; reset after a 200 ms pause without a tick |
| Tradeoffs | walks only from 0.25 m/s in this simulator; no get-up network; the daemon's gain schedule (200 walking / 160 standing) is not modelled since the MJCF fixes `kp`; the fall verdict is instantaneous where the daemon debounces 200 ms; the mouth is not simulated; `alpha_walking` + `alpha_stand` rather than the single `velstand` gait the robot defaults to |

Open Duck Mini v2 differs on every line but the rate (see the study): 101
observation values with three past actions and the last targets, foot
contacts, an imitation phase; `0.25 × action` with a slew limit; Feetech
servos at `kp 13.37`.

## Showing it in Semio Studio

Studio needs the robot's model and the device's keys under Studio's ids.

1. **The model.** `tools/export-urdf.py` turns any MJCF with a floating base
   and hinge joints into a URDF with STL visuals, and checks every mesh
   vertex against MuJoCo's. It needs the hinges at their bodies' origins (as
   onshape-to-robot places them) and STL meshes (Studio reads STL and DAE
   only, matched by file name — two meshes with one file name in different
   directories collide). Studio files the model under the URDF's robot name,
   capitalised, as its model family: the family the device registers with
   must match it for Studio to offer pairing.
2. **The ids.** Studio gives each joint, and the robot's root translation and
   rotation, an id at upload, and drives them from `<id>.position` (radians,
   metres). The device reads them from the GLB Studio saves
   (`--studio-model`, `crates/device/src/studio.rs`), gives the joint ids to
   the HAL as the joints' key entities, and the root ids as its
   `base_pose_keys`: six scalars, the translation and the fixed-axes
   roll, pitch, yaw — what Studio's root takes, in its Z-up world.
3. **The commands.** Studio's Live Data page writes a number as `F64`, a
   string as `String`, a boolean as `Boolean`, and a key the device has not
   published yet as a string: seed every command key in the store with its
   type at build (`command.behavior`, `command.vx`) and publish the ones a
   HAL consumes (`sim/reset`).

A real robot's device does the same with its own HAL: joint ids as key
entities, and a base pose only if the robot estimates one (without it,
Studio's duck walks in place).

## Verifying a port

Do these in order; each catches a different class of mistake.

1. **Ground truth in Python.** Before writing Rust, run the robot's own
   sim2sim script (or a 60-line headless replica of it) with plain `mujoco`
   and `onnxruntime` and record what "working" looks like: distance per
   command, height, whether it falls, at which speeds. Without it, a port that
   stands still at 0.1 m/s looks broken when the policy simply has a dead
   band (Microduck's lies below 0.25 m/s).
2. **The observation, offline.** Feed the Rust observation builder the same
   sensor values as the Python one and compare the vectors index by index;
   then compare one inference. tract and onnxruntime agree to float
   precision.
3. **The module's unit tests** — a golden test pins the observation layout
   index by index and the targets tuning numerically (a shifted block or a
   wrong scale still walks in simulation, so nothing else catches it); leaves
   return `Running`/`Success` as specified; the memory carries and resets as
   specified.
4. **The device in lockstep** — the simulation tests: stands for 10 s,
   walks a distance at a commanded speed, stops when the command drops, the
   showcase tree runs, from the wasm guest as well as in-process.
5. **Served, at real time**, with the recording on, and replay the CSV in
   the robot's own viewer to look at the gait; then drive it over the local
   bridge from a script (write `command.vx` as `{"f64": 0.3}`, read the
   `values_changed` feed) before showing it in Studio.

## What this does not cover

- The real robot. A hardware HAL that publishes the same keys is the missing
  piece; for Microduck specifically, the daemon accepts intents rather than
  joint targets, so a HAL for the real robot speaks that API and the robot's
  own networks stay in charge of the legs (see the study).
- Recurrent policies, and policies whose observation needs the previous
  *targets* rather than actions (Open Duck Mini): both are a few lines in the
  leaf, not a change to the shape.
- Training. Nothing here trains; the module is where a trained network goes.
