# Control policies as Arora modules

A learned locomotion policy running inside a WebAssembly Arora module, driven
by a behavior tree, against a robot simulated in MuJoCo — on a laptop, and
live in Semio Studio. The robot is Pollen Robotics' **Microduck** and its
published walking, standing, sit/stand and skill networks; the pattern is the
point, and it is built to be forked for another robot.

Updating the robot's control policy is shipping a new module: the same `.wasm`
loads on any Arora host — this device, a robot's runtime, a browser page.

```
examples/policy-modules/
├── crates/arora-policy          ONNX inference (tract), history, decimator, gravity — what a policy module needs, wasm-safe
├── crates/arora-hal-mujoco      any MJCF robot as an Arora HAL, under the keys a real robot HAL uses
├── modules/microduck-policies   the policy module: walk, stand, sit, rise, skill, fallen, head_clip, joint_names
├── modules/blackboard           equals / assign: the leaves a tree switches on a request with
├── modules/say-piper, say-silent  the say leaf: Piper on board (GPLv3, opt-in) and a silent one
├── crates/device                the device: simulator + modules (wasm or in-process) + tree, on the standard Arora run
├── trees/                       Groot trees: interactive, stand, walk, showcase, sit_rise, kick
├── tools/                       setup-mujoco.sh, fetch-assets.sh, export-urdf.py (for Studio), replay.py (recording → video)
├── assets/                      fetched, not committed: the Microduck MJCF, meshes and policies (Apache-2.0)
└── docs/                        study.md (the survey), how-it-works.md, reproducing.md
```

## Set up

Prerequisites: Rust (the pinned nightly installs itself through
`rust-toolchain.toml`), `git`, `curl`, and `uv` for the Python tools. From
this directory:

```sh
tools/fetch-assets.sh                 # the Microduck model and policies, pinned revisions
eval "$(tools/setup-mujoco.sh)"       # MuJoCo 3.12.0; exports MUJOCO_DYNAMIC_LINK_DIR and the loader path (per shell)
cargo test                            # every crate, ten simulated runs included (several minutes the first time)
```

## Watch and drive it in Semio Studio

Studio draws the duck from a URDF and moves it from the device's live keys;
its Live Data page writes the commands. Once:

1. Export the URDF and its meshes:

   ```sh
   uv run --python 3.12 --with mujoco python tools/export-urdf.py
   ```

   It writes `assets/microduck/urdf/` (`microduck.urdf` and 38 STL meshes)
   and checks the result against the MJCF: every mesh vertex where MuJoCo
   has it, to the micrometre.
2. In Studio, **Upload URDF** and select `microduck.urdf` with every `.stl`
   of that directory. Studio saves a model named `microduck`, of model family
   `Microduck`.
3. On that model, **Download Asset**, and save the GLB, say as
   `assets/microduck/studio-model.glb`. It carries the ids Studio gave the
   joints and the robot's placement; the device publishes under them.

Then, for each demo:

4. Run the device with the Studio bridge:

   ```sh
   cargo run --release -p policy-device --features studio,piper -- \
       --studio-model assets/microduck/studio-model.glb
   ```

   The terminal asks for the owner (your Studio user id, shown in Studio's
   **Register Device** dialog), a device name, and the model family: answer
   `Microduck`; an empty owner serves the local bridge instead of Studio.
   `DEVICE_OWNERS`, `DEVICE_NAME` and `MODEL_FAMILY` answer in advance.
   Without a terminal nothing is asked: the device connects to Studio and
   registers what those variables say. The device's Studio identity is kept
   next to the binary (`target/release/.semio/`), so every run of that build
   is the same device.
5. In Studio, open a project with the Microduck model and pair the device
   from the **Connections** sidebar: the duck in the scene moves with the
   simulated one, walking across the floor.
6. On the device's **Live Data** page (claim the device first), add the keys
   to drive:

   | Key | Write | Effect |
   |---|---|---|
   | `command.vx`, `command.vy`, `command.vyaw` | a number (slider) | walk: m/s forward, m/s left, rad/s turning left; walks from about 0.25 m/s |
   | `command.behavior` | `walk`, `stand`, `sit`, `rise`, `kick_left`, `kick_right` | switch behavior; a kick or a rise hands back to `walk` when done, a sit holds until `rise` |
   | `sim/reset` | `true` | put the duck back on its feet at the start |

The device runs the `interactive` tree by default: on the ground, it holds
still until reset; otherwise it does what `command.behavior` asks, looking
around while it walks, and announces each phase out loud on this machine's
speakers ("Ready to go.", "Watch my kick!", …), synthesized on board by
Piper — the `piper` feature, whose first build compiles libpiper (cmake)
and downloads a voice; libpiper is GPLv3, hence opt-in. Without it, or with
`--speech silent`, the sentences are only logged.

## Without Studio

Built without `--features studio`, the device serves the open local bridge
on `ws://127.0.0.1:9000`, the standard Arora bridge any local client talks
to: JSON messages, a `values_changed` push per step with every key that
changed, and writes as

```json
{"type": "write_values", "values": {"command.vx": {"f64": 0.3}, "command.behavior": {"str": "kick_left"}}}
```

```sh
cargo run --release -p policy-device                       # serves; the bridge answers once the modules are compiled
cargo run --release -p policy-device -- --command 0.3 0 0  # starts walking
cargo run --release -p policy-device --features piper      # and speaks on board
```

Without `--studio-model`, joints are published under their names
(`left_knee.position`); `--executor native` runs the modules in-process
instead of as wasm guests; `--tree` picks another tree.

A batch run: simulated time in lockstep, as fast as the machine allows,
recorded and rendered to a video (no display needed):

```sh
mkdir -p recordings
cargo run --release -p policy-device -- --tree trees/walk.groot.xml --command 0.3 0 0 \
    --duration 10 --record recordings/walk.csv
uv run --python 3.12 --with mujoco --with numpy --with imageio --with imageio-ffmpeg \
    python tools/replay.py recordings/walk.csv --video recordings/walk.mp4
```

## The trees

| Tree | What it does |
|---|---|
| `interactive` | the device's default: `command.behavior` selects walking, standing, sitting, rising or a kick; holds still when fallen; announces each phase out loud |
| `stand` | balance in place, holding the commanded head angles |
| `walk` | walk at the commanded twist; the leaf hands over to the standing network at zero command |
| `showcase` | a fallback: hold still when fallen, else look around (a head clip) in parallel with walking |
| `sit_rise` | sit (3 s), rise (3 s), then stand — a `SequenceStar` of episodic leaves |
| `kick` | a learned skill (left kick, 0.5 s) then stand |

A tree binds each leaf parameter to a store key (`{joints.position}`,
`{imu.gyro}`, `{command.vx}`, `{arora/dt}`) and the targets output to
`{joints.target_position}`; the same trees run on any Arora that loads the
modules and a HAL publishing those keys.

## What to expect

Measured on an Apple M1 laptop:

| | |
|---|---|
| showcase tree, 0.3 m/s, 12 simulated s (release, wasm guest) | 1.37 m travelled, upright, 5.2 s of wall time including the wasm compile |
| walk tree, 0.3 m/s, 10 simulated s | 1.17 m travelled, upright |
| served, commanded 0.3 m/s over the local bridge (dev build) | 0.89 m in 8 s; the bridge answers after about 12 s of wasm compile |
| wasm policy guest | 22 MB in the release build (thin LTO, stripped), the ONNX runtime and seven networks inside |
| lockstep speed with the policy in wasm | 6 simulated s in 0.2 s (about 30× real time) |
| `cargo test`, warm | under a minute, ten simulated runs included |

The walking networks stand still below about 0.25 m/s in plain MuJoCo (the
robot's own daemon and reference script behave the same); command 0.25–0.3.

## Read next

- [`docs/how-it-works.md`](docs/how-it-works.md) — how a policy leaf runs, where
  its state lives, how a tree switches policies, on the situation or on a
  request.
- [`docs/reproducing.md`](docs/reproducing.md) — what to keep and what to adapt
  for another robot, how to show it in Studio, and how to verify a port.
- [`docs/speech.md`](docs/speech.md) — how the tree speaks while it balances,
  and where the say contract should live.
- [`docs/study.md`](docs/study.md) — the survey: every robot, simulator and
  policy considered, what was verified on this machine, and the routes not taken.

## Moving it to its own repository

The workspace is self-contained: its own `Cargo.lock`, toolchain file and
`.cargo/config.toml`. The `arora-*` dependencies are declared with both a
`path` (into the SDK checkout) and the `version` the code calls into; once
`arora` 10.1 and `arora-behavior-tree` 8.1 (the releases carrying
`with_declared_module`, `load_groot`, `with_groot`, `with_step_period`, the
local bridge's write fix, and Groot tags resolved by function name) are on
crates.io, deleting the `path` keys makes them resolve from there.

## Not in this version

- A HAL for the real Microduck: its daemon takes intents (a twist, head
  angles, a skill), not joint targets, so that HAL speaks the daemon's
  JSON-RPC API and leaves the legs to the robot's own networks — the API is
  recorded in the study. The device is otherwise the one that would run on
  the robot: only the HAL is simulation-specific.
- Per-run identity for the module's state (one run at a time; the SDK's
  async-functions design records the open question).
