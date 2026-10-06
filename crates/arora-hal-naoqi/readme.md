# arora-hal-naoqi

NAO and Pepper robots as Arora devices, over their NAOqi middleware.

The HAL talks to the robot with [`libqi-vibe`](https://crates.io/crates/libqi-vibe), a Rust
implementation of the `qi` framework: no C++ SDK. Its `arora-naoqi` runner works two ways:

- **From your computer**, the quickest way to try it: it builds like any Rust program and
  drives the robot over the network, through NAOqi's port 9559.
  [Try it on a NAO](#try-it-on-a-nao).
- **On the robot itself**, where the device runs on its own, with no computer attached.
  [Deploy it on the robot](#deploy-it-on-the-robot).

The second way is the remarkable one. Arora is written in Rust, so the whole device (the
runtime, its behavior trees, its WebAssembly modules, its Semio Studio connection)
cross-compiles to a single static binary for the NAO's own computer, a 32-bit Intel Atom,
with nothing to install on the robot. That is rare in the robotics ecosystem: today's
stacks, ROS 2 and the Python AI tooling among them, seldom run on the NAO's on-board
computer, and stay on a separate machine that drives the robot over the network.

- [Try it on a NAO](#try-it-on-a-nao)
- [Deploy it on the robot](#deploy-it-on-the-robot)
- [The default behavior](#the-default-behavior)
- [Semio Studio](#semio-studio)
- [Keys](#keys)
- [Behavior leaves](#behavior-leaves)
- [NAOqi 2.1 and older robots](#naoqi-21-and-older-robots)

## Try it on a NAO

What you get: the robot says "Arora is ready" while its wrists rotate slightly and its hands
open and close; the terminal UI plays it again when you press `r`; the device appears in
your Semio Studio account. `arora-naoqi` runs on your computer and drives the robot over
the network: nothing to cross-compile, nothing to install on the robot.

### 1. Prepare the robot

1. Connect the NAO to your network and note its IP address: press its chest button once and
   it says it. From your computer, `nc -z <nao-ip> 9559` checks that its NAOqi answers.
2. Sit the robot down, or let it crouch in its rest posture: the default behavior stiffens
   only the wrists and hands, nothing that holds the robot up.

There is no need to turn Autonomous Life off: the HAL does it when it connects, so that the
robot's own life does not move the joints Arora drives. Turning it off can send the robot
to its rest posture, one more reason to sit it down first. To keep Autonomous Life on, set
`"disable_autonomous_life": false` in the configuration (see [Use it](#3-use-it)).

### 2. Run it from your computer

You need Rust, installed with [rustup](https://rustup.rs): the repository pins its nightly
toolchain (`rust-toolchain.toml`), which rustup fetches on the first build. Then, from the
repository root, on macOS or Linux:

```sh
cargo run --release -p arora-hal-naoqi --features runner --bin arora-naoqi -- tcp://<nao-ip>:9559
```

The first build takes a while (about ten minutes); the next runs start at once. For Semio
Studio, your computer needs Internet access.

A robot that requires NAOqi authentication takes its credentials from a JSON file given
after the address: `{"user": "nao", "password": "<password>"}`.

### 3. Use it

The terminal UI asks three questions first (see [Semio Studio](#semio-studio)): your Studio
owner UID, a device name, a model family (`nao`). Leave the owner empty to run without
Studio. Then the device starts, the robot says "Arora is ready" with its hands, and:

| Key | Does |
|---|---|
| `r` | plays the default behavior again |
| `PgUp` / `PgDn` / `End`, mouse wheel | scroll the logs, follow them again |
| `q`, `Ctrl-C` | stop the device |

More logs: set `RUST_LOG=debug` before the command. Without a terminal (`nohup`, a
service), the runner is headless: it takes the Studio answers from `DEVICE_OWNERS`,
`DEVICE_NAME` and `MODEL_FAMILY`, and connects to Studio only when `DEVICE_OWNERS` is set.

Options: `--tree <file.groot.xml>` runs another behavior tree, `--no-tree` none; a JSON
configuration ([`configs/nao.json`](configs/nao.json): joint ids, sampling period, joint
speed, sensor families, Autonomous Life) can replace the address, and a second JSON file
overrides it.

## Deploy it on the robot

Once it works from your computer, deploy it: the same device, cross-compiled, runs on the
NAO itself, without your computer. The robot needs SSH access (`ssh nao@<nao-ip>`, the
password is the robot's, `nao` unless it was changed) and, for Semio Studio, Internet
access and the right date (TLS checks certificates against it: `date` on the robot should
print the current time).

### 1. Cross-compile `arora-naoqi` for the NAO

The NAO's computer is a 32-bit Atom: the target is `i686-unknown-linux-musl`, a static
binary that needs nothing from the robot's system. On a Mac, from the repository root:

```sh
# Once: the cross C toolchain (for the crates with C code: ring, aws-lc-sys, zstd-sys),
# CMake (aws-lc-sys builds with it), and the Rust target for the pinned toolchain.
brew install messense/macos-cross-toolchains/i686-unknown-linux-musl cmake
rustup target add i686-unknown-linux-musl

# The build. The linker comes from .cargo/config.toml; the C compiler from these variables.
CC_i686_unknown_linux_musl=i686-unknown-linux-musl-gcc \
CXX_i686_unknown_linux_musl=i686-unknown-linux-musl-g++ \
AR_i686_unknown_linux_musl=i686-unknown-linux-musl-ar \
cargo build --release -p arora-hal-naoqi --features runner --bin arora-naoqi \
  --target i686-unknown-linux-musl
```

The binary is `target/i686-unknown-linux-musl/release/arora-naoqi`: a statically linked
32-bit ELF. The first build takes a while (half an hour on an M1, mostly `aws-lc-sys` and
`wasmtime`). The release profile keeps debug information (over 300 MB); strip it before
copying it to the robot (about 45 MB):

```sh
i686-unknown-linux-musl-strip target/i686-unknown-linux-musl/release/arora-naoqi
```

On an Intel Mac, point
`CARGO_TARGET_I686_UNKNOWN_LINUX_MUSL_LINKER` at `/usr/local/bin/i686-unknown-linux-musl-gcc`
(`.cargo/config.toml` names the Apple Silicon path). On Linux, any
`i686-linux-musl` cross toolchain does, with the same variables pointing at it.

### 2. Copy it to the robot

```sh
scp target/i686-unknown-linux-musl/release/arora-naoqi nao@<nao-ip>:~/
```

### 3. Run it on the robot

```sh
ssh -t nao@<nao-ip>   # -t: the terminal UI needs a terminal
./arora-naoqi tcp://127.0.0.1:9559
```

The device then behaves as [from your computer](#3-use-it). To leave it running after you
log out, run it headless with `nohup` and the Studio variables.

## The default behavior

[`trees/ready.groot.xml`](trees/ready.groot.xml), built into the runner:

```text
Fallback
├── Sequence
│   ├── Equals command.behavior == "ready"
│   ├── Parallel
│   │   ├── HandsReady      wrists ±20° twice, hands open and close three times, over 4 s
│   │   └── Fallback
│   │       ├── Say "Arora is ready"
│   │       └── Succeed     (a robot that cannot speak still moves)
│   └── Assign command.behavior = "idle"
└── Succeed                 (rest)
```

The runner starts with `command.behavior` at `ready`. The `r` key sets it to `ready` again;
so does any bridge client, Studio included: the runner opens `command.behavior` to remote
writers. The gesture stiffens the four joints it moves (0.6) and relaxes them once over.

## Semio Studio

The runner is built with the Studio bridge (`arora`'s `studio-bridge` feature): when the
terminal UI asks for an owner and you give one, the device registers with Semio Studio and
connects to it. There is no pairing code yet: a device declares its owners itself, by their
Studio user ids (UIDs).

1. **Find your Studio UID.** Studio does not display it yet. Sign in to Studio in a
   browser, open the developer tools, then Application (Storage in Firefox) → IndexedDB →
   `firebaseLocalStorageDb` → `firebaseLocalStorage`: the entry's `value.uid` is your UID.
2. **Start the device** and answer the prompts: the owner UID (several, comma-separated, for
   a shared robot), a device name (`arora-device-<id>` when left empty), the model family
   `nao`. Or set `DEVICE_OWNERS`, `DEVICE_NAME`, `MODEL_FAMILY`: a variable that is set
   skips its question.
3. The logs say `Registered device <id> with Studio`, then that the Zenoh session is open.
   The robot is now in your device list in Studio, where you bind it to a robot model and
   drive it.

The device's identity (its Studio credentials) is kept on the machine that runs
`arora-naoqi`, under `semio/arora/devices/default/studio/` in its user data directory:
`~/.local/share` on Linux and on the robot, `~/Library/Application Support` on macOS. The
next runs reuse it without asking again; a device run from your computer, then deployed on
the robot, registers twice. To register the robot afresh, delete that directory, or give the run another
identity with `DEVICE_LOCAL_ID=<name>` (or a whole other directory with `DEVICE_DIR`).

## Keys

The keys are those of an Arora robot standard ([`standard/arora-robot.json`](standard/arora-robot.json)):
an interface, as data, of the keys a robot exposes to Arora with their kind, type, unit and
range, independent of NAOqi. The standard is local to this HAL until Arora has one; the HAL
is its mapping onto NAOqi, and a test checks the two agree.

Joints are named: the NAOqi joint names in snake case (`HeadYaw` → `head_yaw`,
`LShoulderPitch` → `l_shoulder_pitch`), unless the configuration overrides them. A robot
model whose joints carry generated ids (a GLB from Studio) maps those ids to these names on
its side.

| Key | Direction | NAOqi source or target |
|---|---|---|
| `<joint>.position` (rad), `.velocity` (rad/s, derived), `.stiffness`, `.temperature` (°C), `.current` (A) | sensed | `ALMemory` `Device/SubDeviceList/<Joint>/…` |
| `battery.charge` (0..1), `battery.current` (A), `battery.charging` | sensed | `ALMemory` battery keys |
| `imu.angle.{x,y}`, `imu.gyroscope.{x,y,z}`, `imu.accelerometer.{x,y,z}` | sensed | `ALMemory` inertial sensor keys |
| `sonar.{left,right}.distance` (m) | sensed | `ALMemory` ultrasound keys |
| `touch.head.{front,middle,rear}`, `touch.hand.{left,right}.{back,left,right}`, `bumper.{left,right}`, `chest_button` | sensed (events) | `ALMemory` tactile events |
| `<joint>.target_position` (rad) | commanded | `ALMotion.setAngles` |
| `<joint>.target_stiffness` (0..1) | commanded | `ALMotion.setStiffnesses` |
| `led.<group>.color` (`0xRRGGBB`), `led.<group>.intensity` | commanded | `ALLeds.fadeRGB`, `ALLeds.setIntensity` |
| `velocity.{x,y}`, `rotation.z` (normalized -1..1) | commanded | `ALMotion.moveToward` |

Numeric sensor keys are sampled from `ALMemory` at `sensor_period_ms` (50 ms by default)
in one `getListData` call, and only changed values are reported. Touch keys are driven
by `ALMemory` events.

The HAL receives and publishes keys; it sets no key meta. Which keys remote writers may
change is the device's to say, as with the other HALs: the runner opens
`command.behavior` only.

## Behavior leaves

What a robot does over time is a behavior tree's; the HAL gives trees two host modules:

- **`Say`** ([`say`](src/say.rs)): the speech contract of Arora's trees
  (`say(text, voice, viseme)`, the ids `say-silent` and `say-piper` share in
  `examples/policy-modules`) in the robot's voice, `ALTextToSpeech`. `Running` while the
  robot speaks, then `Success` while ticked; a new text, or the leaf halted for 250 ms,
  stops the sentence. The runner installs the robot's voice with
  `say::install(hal.voice())`.
- **`HandsReady`** ([`gestures`](src/gestures.rs)): the default behavior's gesture, timed
  joint targets and stiffness on the wrists and hands, paced by the tree's `arora/dt`.

The default tree also uses the policy-modules example's `blackboard` leaves (`Equals`,
`Assign`), until they and the say contract move to a crate of their own.

## NAOqi 2.1 and older robots

`libqi-vibe` is validated against libqi 4 (NAOqi 2.5 and later) and against `naoqi-sim`, its
simulated NAOqi. NAOqi 2.1 is older, and this HAL has not run on it yet:

- **The connection.** NAOqi 2.1's libqi may predate the capability exchange and the
  authentication that `libqi-vibe` opens a session with. If the runner cannot connect,
  the logs at `RUST_LOG=debug` show where the handshake stops: please report them.
- **The kernel.** NAO's NAOqi 2.1 system runs Linux 2.6.33; Rust's standard library
  targets 3.2 and later. The static musl binary is expected to run, untested. If it does
  not start on the robot, [run it from a computer](#2-run-it-from-your-computer)
  instead.
- **Joints and services.** The HAL discovers the joints with `ALMotion.getBodyNames` and
  uses `ALMotion.setAngles`, `setStiffnesses`, `ALTextToSpeech.say` and `stopAll`, all
  present in NAOqi 2.1.
