# Robots, simulators and policies: the survey behind this example

What was learned choosing a robot whose control policy could run inside a
WebAssembly Arora module against a simulator on a developer's machine. Read
this before re-researching a candidate: every claim below was checked against
the primary source it names (repository trees, raw files, crate registries) or
verified on this machine (macOS 26, Apple M1) on 2026-09-26, and everything
that could not be verified says so.

## What the example needed

1. **A simulator that runs headless on macOS arm64**, driven by a program, so
   an autonomous run (a test, an agent) can exercise the robot without a
   display.
2. **A pre-trained locomotion policy** with a documented observation/action
   contract, in a format a pure-Rust inference engine loads (ONNX), because
   training a walking policy takes a GPU and weeks of reward iteration.
3. **The robot's own stack, runnable locally**, so supporting the real robot
   from Arora can be exercised against the same daemon the robot runs.
4. A model and policies under a **license** that allows redistribution.

MuJoCo is the only simulator that satisfies the first point outright: its
arm64 builds need no display for physics, its Python and Rust bindings both
work here, and every candidate below ships a MuJoCo model (MJCF).

## The choice

**Microduck** (Pollen Robotics / Hugging Face, released 2026-08-27, Apache-2.0)
is the target, with **Open Duck Mini v2** (its open-hardware predecessor) as the
verified fallback. The [Microduck section](#microduck) holds the contract the
module implements; [`reproducing.md`](reproducing.md) says what to keep and
what to adapt on another robot.

The rest of this page is the survey: every candidate, what it offers, and why
it was or was not taken.

## Candidates

| Robot | Simulator model | Pre-trained policy | Contract documented | License | Stack runs locally | Verdict |
|---|---|---|---|---|---|---|
| **Microduck** (Pollen) | MJCF in `microduck_rl` (plain PD actuator variant) | 10 ONNX on Hugging Face + manifest (walk, stand, sit/stand, ground pick, kicks, roll, roller) | yes, in the Rust `duck-control` crate | Apache-2.0 (repos, models) | yes: `robotd --sim` against a MuJoCo body server, both run here | **chosen** |
| **Open Duck Mini v2** (A. Pirrone) | MJCF in `Open_Duck_Playground` | 2 ONNX in the hardware repo | yes, `mujoco_infer.py` | Apache-2.0 hardware repo; playground has Apache headers, no LICENSE file; runtime has none | Python runtime for a Pi, no middleware | verified fallback (walked 20 s headless here) |
| NAO (Aldebaran) | community MJCF (HULKs, V6, GPL-3.0); Webots PROTO (V5, Apache-2.0) | none released and validated; B-Human's `nao.onnx` is unproven on NAO | classical engines only | mixed; meshes CC BY-NC-ND | no: no NAOqi with physics on macOS | not this week: a classical walk port is 4–6 days |
| HighTorque Pi (12 DoF) | MJCF in the RL baseline (one attribute to patch for MuJoCo 3) | ONNX (705→12, 15 stacked frames) | yes | BSD-3 headers, no LICENSE file; weights unstated | ROS 1 + RK3588 RKNN inference | possible; heavier history contract, unclear weight license |
| Berkeley Humanoid Lite | MJCF (assets repo) | ONNX (45→12, 50 Hz) | yes | MIT code, CC-BY-SA-4.0 assets | ROS-free low-level Python | cleanest generic contract; not a social robot |
| MuJoCo Playground humanoids (Unitree G1, Booster T1, Berkeley Humanoid, Apollo) | MJCF in the repo, meshes from Menagerie | ONNX (G1 103→29, T1 85→23, BH 52→12) | yes | Apache-2.0; Menagerie per-robot licenses | no | good second target for a big humanoid |
| Unitree G1 / H1 (`unitree_rl_gym`) | MJCF | TorchScript only | yes | BSD-3 | `unitree_mujoco` + DDS SDK: Linux-only binaries | needs a torch export step; stack not on macOS |
| Booster T1 (`booster_gym`) | MJCF | TorchScript only | yes | Apache-2.0 | Booster SDK | needs a torch export step |
| K-Scale K-Bot / Z-Bot | MJCF (kscale-assets) | `.kinfer` (ONNX pair + metadata, Git LFS, recurrent) | yes | MIT | `kinfer` is Rust over native onnxruntime | recurrent graph, LFS pulls, host runtime |
| Toddlerbot (Stanford) | MJCF | ONNX on Google Drive / W&B, not in the repo | partly | MIT | Python | policies not in tree |
| Fourier N1 | MJCF | TorchScript | yes | LGPL-3.0 | — | export step, LGPL |
| Robotis OP3 | Menagerie MJCF | community NumPy policy, sim-only | partly | Apache-2.0 | — | unvalidated |
| Duke Humanoid v2 | MJCF | TorchScript, whole-body (obs 120, act 31) | heavy | Apache-2.0 | — | interface too heavy |
| Menlo Asimov-1 | MJCF without actuators | none yet | — | GPL-2.0 / CERN-OHL-S | — | no policy |

"Asimov" in this context most plausibly means Menlo Research's open humanoid
Asimov-1; the other things by that name (a Rust neurosymbolic platform, a YC
data company, a DeepMind safety benchmark) are not robots.

## Microduck

Sources, at the revisions examined: [`pollen-robotics/microduck`](https://github.com/pollen-robotics/microduck)
`a9ec4b2` (workspace 0.15.0, 2026-09-23),
[`pollen-robotics/microduck_rl`](https://github.com/pollen-robotics/microduck_rl)
branch `develop` `cb70b79` (2026-09-14),
[`pollen-robotics/microduck-policies`](https://huggingface.co/pollen-robotics/microduck-policies)
`1b56c39` (tag v5), all Apache-2.0. The browser simulator Space carries no
license field.

### The robot and its stack

25 cm, 0.74 kg, fifteen Dynamixel XL330 servos on one 1 Mbaud bus (five per
leg, neck pitch, head pitch/yaw/roll, a mouth), an LSM6DSV16X IMU read as a
Dynamixel node in the same bus transaction, a Rockchip RK3566. The software is
a Rust workspace of daemons talking JSON-RPC 2.0 over Unix sockets, one object
per line: `robotd` (the robot: the 50 Hz control loop, the policies, motor
safety), `configd`, `updaterd`, `btd` (Bluetooth), `padd` (gamepad),
`mediad`, `tofd`. `duck-control` is the library with the observation, the
policy runner (the `ort` crate over a native onnxruntime) and the motor
safety.

**Clients send intents, never joint commands.** The `robot.*` API has
`robot.move` (a twist, sent as a notification at 20–50 Hz, expiring after
500 ms), `robot.head` (four head angles), `robot.look` (gaze IK), `robot.pose`
(body z, roll, pitch), `robot.mouth`, `robot.do` (a skill: `sit_toggle`,
`ground_pick`, `roulade`, `kick_left`, `kick_right`), `robot.enable`,
`robot.init`, `robot.relax`, `robot.stop`, `robot.setMode` (`walk` or
`roller`), policy slots (`robot.policies`, `robot.loadPolicy`), and
`robot.subscribe`, after which `robot.state` notifications stream joints,
targets, velocities, currents, IMU (gyro and a world-from-trunk quaternion),
projected gravity, odometry and the active policy name. There is no method
that takes joint targets and no gain or torque passthrough; the 50 Hz policy
loop is the daemon's own thread. Consequences for Arora:

- Supporting the **real** Microduck from Arora means a HAL that speaks this
  intent API: the robot's own policies stay in charge of the legs, and Arora
  chooses intents. That HAL is a follow-up; the API is recorded here so it
  needs no rediscovery.
- Running **Arora's** policy against the robot means owning the servo bus
  yourself (the daemon says two writers on one UART corrupt each other's
  replies) — a real-robot integration outside this example's scope.
- In simulation, Arora's policy module drives the physics directly through
  the MuJoCo HAL; the daemon is not in the loop.

**Sim mode.** `robotd --sim host:port` makes the daemon a TCP client of a
*body server*: newline-delimited JSON, protocol 1, ops `hello`, `read`,
`write` (15 targets in wire order), `gain`, `torque`, `slow` (voltage and
temperatures). The body owns the clock: `duck-body`
(`microduck_rl/src/mjlab_microduck/sim/body_server.py`) steps 4 × 5 ms per
real-time 20 ms pass and answers `read` with positions, velocities, a stand-in
current, and the IMU already resolved into the trunk frame (gyro, projected
gravity, quaternion). Both halves run on this Mac: `cargo build -p robotd`
succeeds (rustc 1.96, no Linux-only dependency; the `ort` crate loads
`libonnxruntime` at run time through `ORT_DYLIB_PATH`, the dylib from the pip
wheel works), and `duck-body --headless` needs only `mujoco` and `numpy`. The
full stack was driven end to end here: enabled from a seated start, the duck
stood up (trunk 0.070 → 0.116 m), the control loop reported healthy at 45 Hz,
and `robot.state` streamed. The docs say the body server runs the BAM
actuator model the policies were trained with; the code runs the MJCF
position actuators (`kp 0.55`) scaled by the daemon's gain over 200.

### The policies

All ten ONNX files on the Hugging Face repository are the same feed-forward
network: input `obs [1, 61]`, output `actions [1, 14]`, a baked normalizer
(Sub, Div) then Gemm 61→512→256→128→14 with ELU, opset 18, 197,896
parameters, 794 KB each. None is recurrent (the daemon supports LSTM exports;
none ships). The manifest (`manifest.json`, schema 2) gives each file a kind:
`perpetual` gaits and stands (`alpha_walking`, `velstand`, `alpha_stand`,
`roller`), `scripted` (`alpha_sitstand`, posture flag in the command's `vx`
slot, sit = 1, stand = 0, 2 s ramp), `episodic` skills with a duration
(`alpha_ground_pick` 2.8 s with a phase-encoded command, `roulade` 1.0 s,
`ball_kick_left` / `ball_kick_right` 0.5 s, `roller_crouch`). ONNX metadata on
every file names the joints, the default pose, the observation blocks and an
`action_scale` of 1.0.

**Observation (61)**, from `duck-control/src/obs.rs`, no clipping or scaling in
the daemon:

| index | width | contents |
|---|---|---|
| 0..3 | 3 | gyro, trunk frame, rad/s |
| 3..6 | 3 | projected gravity, trunk frame, unit vector; upright is `[0, 0, -1]` |
| 6..20 | 14 | joint position minus the default pose, mouth excluded |
| 20..34 | 14 | joint velocity, mouth excluded |
| 34..48 | 14 | previous raw action |
| 48..51 | 3 | command twist `vx, vy, vyaw` (m/s, m/s, rad/s; x forward, y left, +yaw left) |
| 51..55 | 4 | head command `neck_pitch, head_pitch, head_yaw, head_roll` (rad) |
| 55..57 | 2 | body x, y — always zero |
| 57..60 | 3 | body `z, roll, pitch` (m, rad, rad; trained ranges z −0.025..+0.010, angles ±0.26) |
| 60 | 1 | body yaw — always zero |

Head targets ride in the command and are not added to the output. Projected
gravity is `q⁻¹ · (0, 0, −1) · q` for the world-from-trunk quaternion
`[w, x, y, z]`; the daemon adds a three-sample median per component.

**Action**: `targets = default_pose + action_scale × action`, mouth left at 0.
The daemon's deployment tuning (`robotd/src/control.rs`) is `action_scale 0.9`
walking, `1.0` standing, servo gain 200 walking, 160 standing, plus first-order
low-pass filters (0.7 on the legs, 0.5 on the head); the training and the
reference sim2sim script use scale 1.0 and no filter. The network switches to
the stand policy when the twist magnitude is at most 0.05; switching resets
the network's memory, the previous action is shared across networks.

**Joint order (policy = actuator order)**, default pose in radians:
`left_hip_yaw 0, left_hip_roll −0.0873, left_hip_pitch −0.4579, left_knee
−0.0049, left_ankle 0.4530, neck_pitch 0.3491, head_pitch 0.3491, head_yaw 0,
head_roll 0, right_hip_yaw 0, right_hip_roll 0.0873, right_hip_pitch 0.4579,
right_knee 0.0049, right_ankle −0.4530`. The daemon's wire order inserts the
mouth at index 9 (fifteen joints); the MJCF and the policies have fourteen.

### The model

`microduck_rl/src/mjlab_microduck/robot/microduck/`: `scene.xml` (a floor and
the keyframes `INIT`, `STAND`, `SIT`, `FOLD`) including
`robot_groundcontact.xml` — the file the body server and the reference
sim2sim script load; `robot_walk.xml` (walk training, fewer collision geoms);
`robot_allcollisions.xml` (fall training, the browser simulator). Sixteen
bodies, a free joint and fourteen hinges, 38 meshes (55 STL files, 23.6 MB),
total mass 0.737 kg. Every servo joint and actuator carries the
`chosen_actuator` class: `joint damping 0.053 frictionloss 0.0048 armature
0.0018`, `position kp 0.55 kv 0 forcerange ±0.96 ctrlrange ±10` — the BAM
fit of an XL330 at gain 200 and 7.4 V. Sensors on the `imu` site of
`trunk_base`: `framequat orientation`, `gyro angular-velocity` and
`imu_ang_vel`, `velocimeter imu_lin_vel`, `accelerometer imu_accel`. The
`STAND` keyframe is the default pose at trunk height 0.12 m; `SIT` is trunk
0.07 m with knees at 60°. The scene file says `timestep 0.002`; both reference
scripts override it to 0.005 and run the policy every 4 steps.

### Verified on this machine

A headless replica of `infer_policy.py --no-bam --new-cmd-obs` (plain
`mujoco` 3.14 + `onnxruntime` 1.30, no mjlab, no BAM), scene `scene.xml`,
timestep 0.005, decimation 4, scale 1.0, ran every policy about 60× real
time. Neither `alpha_walking` nor `alpha_stand` falls; the stand holds
0.116 m. **The walking policies have a low-speed dead band in plain CPU
MuJoCo**: commanded at 0.05–0.20 m/s they stand still (a few millimetres in
10 s); at 0.25 m/s `alpha_walking` covers 1.0 m in 10 s, at 0.30 m/s 1.25 m
(about 0.10–0.12 m/s achieved). The same threshold holds with the BAM
actuator model and through the real daemon stack in sim mode
(`robot.move vx 0.15` for 8 s: 6 mm), so it is the policies' behaviour, not
the replica's; whether the hardware walks slower than 0.25 m/s is unverified.
The training command ranges are vx ±0.4, vy ±0.3, yaw ±1.0 with a standing
curriculum. The browser simulator commands 0.25 m/s forward.

### Consequences for the module

- The module reproduces `duck-control`'s observation and `robotd`'s
  scale/filter tuning, and embeds the policies as ONNX bytes.
- Walking commands below 0.25 m/s do nothing visible in this simulator.
- Sim time and physics live in `arora-hal-mujoco` with `scene.xml`, the
  `STAND` keyframe, control at 50 Hz over a 5 ms timestep, the `imu_ang_vel`
  gyro and the base body's orientation for gravity.

## Open Duck Mini v2 (the fallback)

Two different robots share the duck lineage: Open Duck Mini v2 (2025, 42 cm,
Feetech STS3215 servos, Raspberry Pi Zero 2W, Python runtime, MuJoCo
Playground/JAX training, 101-value observation) and Microduck (2026, 25 cm,
Dynamixel XL330, Rockchip RK3566, Rust runtime, mjlab training, 61-value
observation). Their policies, models and observation contracts are not
interchangeable.

Repositories: [`apirrone/Open_Duck_Mini`](https://github.com/apirrone/Open_Duck_Mini)
(branch `v2`; hardware, and the policies `BEST_WALK_ONNX.onnx` /
`BEST_WALK_ONNX_2.onnx` at its root),
[`apirrone/Open_Duck_Mini_Runtime`](https://github.com/apirrone/Open_Duck_Mini_Runtime)
(the Pi runtime), [`apirrone/Open_Duck_Playground`](https://github.com/apirrone/Open_Duck_Playground)
(training and the MJCF under `playground/open_duck_mini_v2/xmls/`),
[`apirrone/Open_Duck_reference_motion_generator`](https://github.com/apirrone/Open_Duck_reference_motion_generator)
(placo gait generator). There is no separate description repository.

The policy is a plain MLP: `obs[1,101] → continuous_actions[1,14]`, observation
normalization baked in (Sub, Mul), four Gemm layers 512→256→128→28 with swish
activations, a Split and a Tanh on the mean; 220,262 parameters, 884 KB, opset
11. Observation, in order: gyro (3), accelerometer (3, with a +1.3 m/s² bias on
x applied in training and sim2sim but not on the robot), command (7: vx, vy,
wz, neck pitch, head pitch, head yaw, head roll), joint angles minus the home
pose (14), joint velocities × 0.05 (14), the last three actions (3 × 14), the
motor targets sent last step (14, absolute), foot contacts (2), imitation phase
(2: cos and sin of 2πi/27). Action: `targets = home + 0.25 × action`, slewed to
±5.24 rad/s per 20 ms step; 50 Hz control over 2 ms physics (decimation 10).
Joint order: left hip yaw, roll, pitch, knee, ankle; neck pitch; head pitch,
yaw, roll; right hip yaw, roll, pitch, knee, ankle. Actuators: MuJoCo position
servos, `kp 13.37` (flat) or `17.11` (backlash variant), `forcerange ±3.23`,
joint `damping 0.56 frictionloss 0.068 armature 0.027`.

The reference-motion file (`polynomial_coefficients.pkl`, 3 MB) is a training
artifact; inference needs only the period length (27 steps of 20 ms).

Verified on this machine with a 60-line headless replica of `mujoco_infer.py`
(`uv run --python 3.12 --with mujoco --with onnxruntime --with numpy`; MuJoCo
3.14.0, onnxruntime 1.30.0): the second policy walks 20 s without falling on
both flat scenes, 20–50× real time, one inference in 40–60 µs.

| scene | policy | vx command | distance over 18 s | fell |
|---|---|---|---|---|
| flat (kp 13.37) | ONNX_2 | 0.10 | 0.58 m | no |
| flat | ONNX_2 | 0.15 | 1.70 m | no |
| flat with backlash joints (kp 17.11) | ONNX_2 | 0.10 | 1.39 m | no |
| flat | ONNX (first) | 0.10 | 1.40 m | no |

The upstream script needs the MuJoCo viewer (`mjpython` on macOS) and imports
JAX through the playground's base class; the replica avoids both. The
playground's `pyproject` pins `jax[cuda12]` unconditionally (fails on macOS;
an open pull request gates it on Linux) and `playground>=0.0.3` resolves to a
version missing a module (pin `0.0.5`).

The placo gait generator produces open-loop reference walks (16 joints, 50
fps, JSON frames) usable as animations; its pinned placo release has only
Linux x86_64 wheels, and the newer arm64-capable release's API compatibility
is unverified.

## NAO

No maintained project pairs a NAO simulation with a released, validated
walking policy, and no NAOqi with physics runs on this Mac: Aldebaran's
virtual robot has no gravity, the Webots-based `naoqisim` is deprecated, and
the simulator SDK was dropped for macOS long ago. qiBullet is unmaintained
(last release 2022) on a pybullet whose wheels are Linux-only. Gazebo paths
are ROS 1.

What does exist:

- **MuJoCo NAO V6** from HULKs (`tools/machine-learning/mujoco/model/nao.xml`,
  GPL-3.0): 26 joints, position actuators (`kp 21.1`, ±5 N·m), IMU and eight
  FSR touch sensors, plus a Python port of their walking engine that drives it.
  MuJoCo Menagerie has no NAO.
- **Webots R2025a** is native arm64 and ships the NAO V5 PROTO (Apache-2.0);
  Bembelbots add a V6 PROTO and a LoLA-protocol emulating controller. Webots
  needs a logged-in window session even with `--no-rendering`, so it is not
  headless on macOS.
- **Classical walking engines** small enough to port: HTWK's `HTWKMotion`
  (about 1,000 lines of C++ with Eigen), rUNSWift's `Walk2014Generator` (one
  1,700-line class), and HULKs' walking engine already in Rust (tag
  `robocup2025`, GPL-3.0). All are Hengst-style walks at the 12 ms LoLA cycle
  with FSR and gyro feedback.
- **B-Human 2025** ships `Config/NeuralNets/BoosterWalk/nao.onnx` (254 KB, 47
  inputs → 12 leg targets at 50 Hz), but their own NAO configuration does not
  use it and its quality on NAO is undocumented.

Verdict: a NAO walking in MuJoCo is 4–6 engineer-days of porting and in-sim
tuning of a classical engine, without a learned policy. The `arora-hal-mujoco`
crate and the module pattern here apply unchanged; only the policy body
differs. The SDK's own `modules/nao` targets the real robot's NAOqi through
libqi and is unrelated to simulation.

## Routes considered and not taken

- **A home-made simulator on Bevy + Rapier.** No policy exists for a
  home-made model, and a policy trained in MuJoCo depends on MuJoCo's actuator
  and contact models; reproducing them in another engine is its own research
  project. `rapier3d-mjcf` (dimforge, 0.36) loads MJCF into Rapier and is the
  crate to start from if a pure-Rust, browser-runnable physics ever matters
  more than fidelity.
- **copper-rs as the simulation host.** Copper (1.3.0-dev, Apache-2.0) has a
  deterministic task graph and log/replay, and a `sim_mode` callback API; its
  example worlds are Bevy + avian3d game physics (a balance bot, a
  quadcopter). It has no MuJoCo integration and nothing legged, so it would
  not shorten the path; MuJoCo would still have to be brought in.
- **Training a policy.** MuJoCo Playground and mjlab train on GPUs; CPU JAX on
  this machine is orders of magnitude too slow for a week.
- **A NAO route** (above).
- **`candle-onnx` for inference**: no `Elu` and no `Softplus` in its operator
  table, which rules out the rsl-rl exports (Microduck, HighTorque, Berkeley
  Lite all use ELU). `ort` and K-Scale's `kinfer` wrap native onnxruntime, so
  they cannot live in a wasm module. `wonnx` needs WebGPU from the host.
- **A hand-rolled MLP evaluator** instead of tract: every policy inspected is
  four Gemm layers with one activation, so 150 lines of Rust would do and the
  module would shrink from megabytes to kilobytes. Kept as an option; tract
  was chosen because it loads any ONNX a forker drops in without a converter.

## Tooling verified on this machine

- **`mujoco-rs` 6.1.0** (MuJoCo 3.12.0, MIT/Apache-2.0) builds and runs on
  Apple Silicon. Its automatic MuJoCo download is Linux/Windows only; on
  macOS the framework from the official DMG is extracted by hand
  (`tools/setup-mujoco.sh`). The build needs `MUJOCO_DYNAMIC_LINK_DIR`
  pointing at a directory holding `libmujoco.dylib`; the resulting binary
  looks for `libmujoco.3.12.0.dylib` through `DYLD_LIBRARY_PATH` (a symlink
  under that name is what makes the loader find the framework's library).
  The bindings mirror exactly one MuJoCo release: the `libmujoco.3.14.0.dylib`
  a pip wheel ships does not match. The viewer and offscreen renderer need a
  patched `glutin` on macOS; physics needs nothing.
- **`tract-onnx` 0.23.8** compiles to `wasm32-wasip1` on the SDK's pinned
  nightly with default features off; a probe module with the loader alone is
  14 MB of wasm before size optimization. Its operator table covers the
  policies above (Gemm, Elu, Tanh, Sigmoid, Split, Sub, Div, Mul, Concat,
  Clip); recurrent operators (LSTM, GRU) exist but were not exercised.
- **Python**: `uv` creates a 3.12 environment with `mujoco` 3.14.0 and
  `onnxruntime` 1.30.0 arm64 wheels in seconds; that is enough for every
  ground-truth run and for rendering videos offscreen.
- **Microduck's daemon** builds on macOS with stable Rust and runs in sim
  mode against its Python body server (details above).

## Sources

- Microduck: <https://github.com/pollen-robotics/microduck>,
  <https://github.com/pollen-robotics/microduck_rl> (branch `develop`),
  <https://huggingface.co/pollen-robotics/microduck-policies>,
  <https://huggingface.co/spaces/pollen-robotics/microduck-simulator>
- Open Duck Mini: the four `apirrone` repositories above.
- NAO: HULKs (<https://github.com/HULKs/hulk>, tag `robocup2025`),
  B-Human 2025 code release, rUNSWift 2025 release, NaoHTWK/HTWKMotion,
  Bembelbots/WebotsLoLaController, cyberbotics/webots R2025a.
- Survey: HighTorque-Robotics (`livelybot_pi_rl_baseline`, `Mini-Pi-Plus_AMP`,
  `HT_Robot_URDF`), HybridRobotics/Berkeley-Humanoid-Lite,
  google-deepmind/mujoco_playground, unitreerobotics/unitree_rl_gym and
  unitree_mujoco, BoosterRobotics/booster_gym, kscalelabs (kinfer,
  kinfer-sim, kbot-models), hshi74/toddlerbot, FFTAI/Wiki-GRx-Mujoco,
  generalroboticslab (Duke), menloresearch/asimov-1, copper-project/copper-rs,
  sonos/tract, davidhozic/mujoco-rs.
