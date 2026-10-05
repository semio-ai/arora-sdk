# arora-hal-naoqi

NAO and Pepper robots as Arora devices, over their NAOqi middleware.

The HAL talks to the robot with [`libqi-vibe`](https://crates.io/crates/libqi-vibe), a Rust
implementation of the `qi` framework: no C++ SDK. The `arora-naoqi` runner can run on the
robot itself (cross-compiled, below) or on any computer that reaches the robot's NAOqi over
TCP (port 9559).

- [Try it on a NAO](#try-it-on-a-nao)
- [The default behavior](#the-default-behavior)
- [Semio Studio](#semio-studio)
- [Keys](#keys)
- [Behavior leaves](#behavior-leaves)
- [NAOqi 2.1 and older robots](#naoqi-21-and-older-robots)

## Try it on a NAO

What you get: the robot says "Arora is ready" while its wrists rotate slightly and its hands
open and close; the terminal UI plays it again when you press `r`; the device appears in
your Semio Studio account.

### 1. Prepare the robot

1. Connect the NAO to your network and note its IP address: press its chest button once and
   it says it.
2. Check that you can log in: `ssh nao@<nao-ip>` (the password is the robot's, `nao` unless
   it was changed).
3. Sit the robot down, or let it crouch in its rest posture: the default behavior stiffens
   only the wrists and hands, nothing that holds the robot up.
4. Turn Autonomous Life off, so that it does not move the arms while Arora does. On the
   robot:

   ```sh
   qicli call ALAutonomousLife.setState disabled
   ```

5. For Semio Studio, the robot needs Internet access and the right date (TLS checks
   certificates against it): `date` on the robot should print the current time.

### 2. Cross-compile `arora-naoqi` for the NAO

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

### 3. Copy it to the robot

```sh
scp target/i686-unknown-linux-musl/release/arora-naoqi nao@<nao-ip>:~/
```

### 4. Run it

```sh
ssh -t nao@<nao-ip>   # -t: the terminal UI needs a terminal
./arora-naoqi tcp://127.0.0.1:9559
```

The terminal UI asks three questions first (see [Semio Studio](#semio-studio)): your Studio
owner UID, a device name, a model family (`nao`). Leave the owner empty to run without
Studio. Then the device starts, the robot says "Arora is ready" with its hands, and:

| Key | Does |
|---|---|
| `r` | plays the default behavior again |
| `PgUp` / `PgDn` / `End`, mouse wheel | scroll the logs, follow them again |
| `q`, `Ctrl-C` | stop the device |

More logs: `RUST_LOG=debug ./arora-naoqi tcp://127.0.0.1:9559`. Without a terminal
(`nohup`, a service), the runner is headless: it takes the Studio answers from
`DEVICE_OWNERS`, `DEVICE_NAME` and `MODEL_FAMILY`, and connects to Studio only when
`DEVICE_OWNERS` is set.

To try it before cross-compiling, run the same device on your computer, against the robot
over the network:

```sh
cargo run --release -p arora-hal-naoqi --features runner --bin arora-naoqi -- tcp://<nao-ip>:9559
```

Options: `--tree <file.groot.xml>` runs another behavior tree, `--no-tree` none; a JSON
configuration (`configs/nao.json`: joint ids, sampling period, joint speed, sensor
families) can replace the address, and a second JSON file overrides it.

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

The device's identity (its Studio credentials) is kept on the robot under
`~/.local/share/semio/arora/devices/default/studio/`; the next runs reuse it without
asking again. To register the robot afresh, delete that directory, or give the run another
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
  not start on the robot, run it on a computer instead (above).
- **Joints and services.** The HAL discovers the joints with `ALMotion.getBodyNames` and
  uses `ALMotion.setAngles`, `setStiffnesses`, `ALTextToSpeech.say` and `stopAll`, all
  present in NAOqi 2.1.
