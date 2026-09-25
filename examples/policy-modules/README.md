# Control policies as Arora modules

A learned locomotion policy running inside a WebAssembly Arora module, driven
by a behavior tree, against a robot simulated in MuJoCo — on a laptop, headless
if wanted. The robot is Pollen Robotics' **Microduck** and its published walking,
standing, sit/stand and skill networks; the pattern is the point, and it is
built to be forked for another robot.

Updating the robot's control policy is shipping a new module: the same `.wasm`
loads on any Arora host — this device, a robot's runtime, a browser page.

```
examples/policy-modules/
├── crates/arora-policy          ONNX inference (tract), history, decimator, gravity — what a policy module needs, wasm-safe
├── crates/arora-hal-mujoco      any MJCF robot as an Arora HAL, under the keys a real robot HAL uses
├── modules/microduck-policies   the module: walk, stand, sit, rise, skill, fallen, head_clip, joint_names
├── crates/device                the device: simulator + module (wasm or in-process) + tree + local bridge; lockstep tests
├── trees/                       Groot trees: stand, walk, showcase, sit_rise, kick
├── tools/                       setup-mujoco.sh, fetch-assets.sh, replay.py (recording → video)
├── assets/                      fetched, not committed: the Microduck MJCF, meshes and policies (Apache-2.0)
└── docs/                        study.md (the survey), how-it-works.md, reproducing.md
```

## Run it

Prerequisites: Rust (the pinned nightly installs itself through
`rust-toolchain.toml`), `git`, `curl`; `uv` for the Python replay tool. Then,
from this directory:

```sh
tools/fetch-assets.sh                 # the Microduck model and policies, pinned revisions
eval "$(tools/setup-mujoco.sh)"       # MuJoCo 3.12.0; exports MUJOCO_DYNAMIC_LINK_DIR and the loader path
cargo test                            # every crate, including the simulated runs (a few minutes the first time)
```

A batch run: ten simulated seconds of walking at 0.3 m/s, as fast as the
machine allows, with the policy in the wasm guest, recorded:

```sh
mkdir -p recordings
cargo run --release -p policy-device -- --tree trees/walk.groot.xml --command 0.3 0 0 \
    --duration 10 --record recordings/walk.csv
uv run --python 3.12 --with mujoco --with numpy --with imageio --with imageio-ffmpeg \
    python tools/replay.py recordings/walk.csv --video recordings/walk.mp4
```

A live device at real time, serving the open local bridge on
`ws://127.0.0.1:9000`, running the showcase tree (look around while walking,
hold still when fallen); write `command.vx`, `command.vy`, `command.vyaw`
through the bridge to drive it:

```sh
cargo run --release -p policy-device -- --command 0.3 0 0
```

`--executor native` runs the module in-process instead of the wasm guest;
`--speed 0` runs the live device as fast as the machine allows; `--tree`
picks another tree.

## The trees

| Tree | What it does |
|---|---|
| `stand` | balance in place, holding the commanded head angles |
| `walk` | walk at the commanded twist; the leaf hands over to the standing network at zero command |
| `showcase` | a fallback: hold still when fallen, else look around (a head clip) in parallel with walking |
| `sit_rise` | sit (3 s), rise (3 s), then stand — a `SequenceStar` of episodic leaves |
| `kick` | a learned skill (left kick, 0.5 s) then stand |

A tree binds each leaf parameter to a store key (`{joints.position}`,
`{imu.gyro}`, `{command.vx}`, `{arora/dt}`) and the targets output to
`{joints.target_position}`; the same trees run on any Arora that loads the
module and a HAL publishing those keys.

## What to expect

Measured on an Apple M1 laptop:

| | |
|---|---|
| showcase tree, 0.3 m/s, 12 simulated s (release, wasm guest) | 1.38 m travelled, upright, 7.2 s of wall time including the wasm compile |
| wasm guest | about 16 MB stripped, the ONNX runtime and seven networks inside; 13.6 s to compile and load in a dev build |
| lockstep speed with the policy in wasm | 6 simulated s in 0.2 s (about 30× real time) |
| `cargo test`, warm | about 20 s, seven simulated runs included |

The walking networks stand still below about 0.25 m/s in plain MuJoCo (the
robot's own daemon and reference script behave the same); command 0.25–0.3.

## Read next

- [`docs/how-it-works.md`](docs/how-it-works.md) — how a policy leaf runs, where
  its state lives, how a tree switches policies.
- [`docs/reproducing.md`](docs/reproducing.md) — what to keep and what to adapt
  for another robot, and how to verify a port.
- [`docs/study.md`](docs/study.md) — the survey: every robot, simulator and
  policy considered, what was verified on this machine, and the routes not taken.

## Moving it to its own repository

The workspace is self-contained: its own `Cargo.lock`, toolchain file and
`.cargo/config.toml`. The `arora-*` dependencies are declared with both a
`path` (into the SDK checkout) and a published `version`; delete the `path`
keys and they resolve from crates.io. The SDK additions it relies on
(`AroraBuilder::with_declared_module`, `Arora::load_groot`, Groot tags resolved
by function name) ship with the `arora` release that carries them.

## Not in this version

- A HAL for the real Microduck: its daemon takes intents (a twist, head
  angles, a skill), not joint targets, so that HAL speaks the daemon's
  JSON-RPC API and leaves the legs to the robot's own networks — the API is
  recorded in the study.
- A Semio Studio connection (the `arora` crate's `studio-bridge` feature) and
  a rendering of the duck in Studio; the local bridge is what is served.
- Per-run identity for the module's state (one run at a time; the SDK's
  async-functions design records the open question).
