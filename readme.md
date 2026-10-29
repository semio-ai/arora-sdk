# Arora

<p align="center">
  <a href="examples/policy-modules/README.md"><img src="examples/policy-modules/docs/images/microduck-walk.webp" alt="The Microduck walking in MuJoCo, driven by a learned policy inside a WebAssembly Arora module" width="480"></a>
  <br><sub>Robot model and policies: the <a href="https://github.com/pollen-robotics/microduck_rl">Microduck</a> by Pollen Robotics, Apache-2.0.</sub>
</p>

**Arora runs robots.** It is a runtime for robotic devices: it reads the
hardware, runs the device's behavior, and lets remote clients watch and drive
it live, all on one deterministic loop. What the device computes comes as
**modules** — typed functions written once, in Rust or C++, that run natively,
in WebAssembly, or in a browser.

Above, Pollen Robotics' **Microduck** walks in the MuJoCo simulator. Its
gait is a learned policy compiled into a WebAssembly module; a behavior tree
calls it every 20 ms and hands the joint targets it returns to the simulator.
The simulator is only the device's hardware layer: a HAL for the robot
would run the same module and the same tree against real motors. The
[Microduck example](examples/policy-modules/README.md) runs it on a laptop in
three commands.

## How the duck walks: Arora in five ideas

**1. The device's state is values under keys.** Everything the duck knows is
in one **store**: `imu.gyro`, `joints.position`, `command.vx`, `arora/time`.
A key is a path; a value is a typed Arora `Value`.

**2. The HAL connects the store to the hardware.** The MuJoCo **HAL**
(hardware abstraction layer) writes what the simulated robot senses —
orientation, joint positions and velocities — into the store each step, and
drives the motors from `joints.target_position`. A HAL for the real robot
would do the same over the robot's motor bus.

**3. A module is a set of typed functions.** The policy module exports
`walk`, `stand`, `sit`, `rise` and more. A call takes in parameters and gives
back out parameters; it touches no key:

```rust
pub fn walk(vx: f64, vy: f64, vyaw: f64, head: Vec<f32>, dt_ns: u64,
            gyro: Vec<f32>, orientation: Vec<f32>,
            joint_positions: Vec<f32>, joint_velocities: Vec<f32>,
            targets: &mut Vec<f32>) -> Status
```

**4. A behavior decides what to call, and where the results go.** Each step
the device ticks its **behavior tree**. The walking tree is one leaf: it reads
keys into `walk`'s arguments and writes the `targets` it gets back to
`joints.target_position`, which the HAL then sends to the motors:

```xml
<Walk vx="{command.vx}" vy="{command.vy}" vyaw="{command.vyaw}" head="{command.head}"
      dt_ns="{arora/dt}" gyro="{imu.gyro}" orientation="{imu.orientation}"
      joint_positions="{joints.position}" joint_velocities="{joints.velocity}"
      targets="{joints.target_position}" />
```

**5. Bridges let remote clients in.** A remote client may write only the keys
the device opens: the duck opens its `command.*` keys, its HAL the simulator's
inputs, and every other key stays read-only to remotes. A client of the
**open local bridge** — JSON messages over a local WebSocket — sets
`command.vx` to `0.3`, and the duck walks forward. The same device can also
face a ROS 2 graph, or, in a build that opts in, Semio Studio.

All five meet in the **step**, which the device runs on a fixed period: publish the
clock, apply the HAL's readings, apply the remote writes, tick the behavior,
send what changed to the HAL and to the clients. One owner, one order, no
locks — the same loop natively and in a browser page.

[Concepts](docs/overview.md) takes these ideas one at a time, in full.

## Try it

```sh
cargo build
./target/debug/arora crates/arora/examples/hello_tree.groot.xml
```

That is a device with a stand-in HAL, running a small behavior tree and
serving the open local bridge on `ws://127.0.0.1:9000`. The
[`arora` runner](crates/arora/readme.md#run-the-device-runner) loads modules
from module directories (`--module`), and `--open` lets a client write any
key but the runtime's own `arora/` keys, for a sandbox. For the duck, follow
the [Microduck example](examples/policy-modules/README.md#run-it).

[Building](docs/building.md) lists the prerequisites (a nightly Rust that
`rust-toolchain.toml` installs, CMake for the C++ modules).

## Bring your own robot

A robot comes to Arora as a **HAL**: an implementation of `arora_hal::Hal`
that reports what the robot senses as keys and drives its actuators from
keys. Hand it to a device, and the rest — the step, the behaviors, the
modules, the bridges — is Arora's:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    arora::Arora::builder()
        .with_hal(Box::new(MyRobotHal::connect().await?))
        .run()
        .await
}
```

**Support a new robot in a repository of its own**, depending on the
published crates (`arora`, `arora-hal`, …) from crates.io: the robot's HAL
and its runner change with the robot, and its toolchain stays out of Arora's.
[arora-aldebaran](https://github.com/semio-ai/arora-aldebaran) carries NAO
and Pepper this way. [`arora-hal`](crates/arora-hal/readme.md) explains the
trait, and the [`arora` crate](crates/arora/readme.md#use-it-as-a-library)
the builder and the command-line options a runner takes.

## Write a module

A Rust module declares its interface on the code itself, and the crate builds
into a WebAssembly component or a native library:

```rust
#[arora_module::module(id = "a1a6bb9a-…", name = "polly", version = "0.1.0")]
pub mod polly {
  #[export(id = "e1b4bda7-…")]
  pub fn say(#[param(id = "fb3787f2-…")] text: String) -> Status { … }
}
```

Every module, function and parameter is pinned by a UUID, so names can change
without breaking a caller. A device carries a module as a **module
directory** — its header, its artifact and optionally its type records —
and loads it at start. See [`arora-module`](crates/arora-module/readme.md); a C++ module
starts from [`examples/modules/cpp`](examples/modules/cpp/readme.md).

## The crates

One Cargo workspace; [architecture](docs/architecture.md) shows how the
layers depend on each other. Crates marked † are not published to crates.io.

| | Crates |
|---|---|
| **Vocabulary** | [`arora-types`](crates/arora-types/readme.md) values, types, calls, records · [`arora-buffers`](crates/arora-buffers/readme.md) their binary form |
| **The device** | [`arora`](crates/arora/readme.md) the runtime and its runner · [`arora-hal`](crates/arora-hal/readme.md) · [`arora-bridge`](crates/arora-bridge/readme.md) · [`arora-behavior`](crates/arora-behavior/readme.md) · [`arora-simple-data-store`](crates/arora-simple-data-store/readme.md) |
| **Modules** | [`arora-engine`](crates/arora-engine/readme.md) loads and calls them · [`arora-module`](crates/arora-module/readme.md) declares one in Rust · [`arora-module-authoring`](crates/arora-module-authoring/cli/readme.md) generates one from a `module.yaml` · [`arora-registry`](crates/arora-registry/readme.md) the type registry |
| **Behaviors** | [`arora-behavior-tree`](crates/arora-behavior-tree/readme.md) behavior trees, as text, Groot XML or YAML |
| **Bridges** | [`arora-bridge-ws`](crates/arora-bridge-ws/README.md) the open local bridge · [`arora-bridge-ros2`](crates/arora-bridge-ros2/README.md) ROS 2 · [`arora-msgs-ros2`](crates/arora-msgs-ros2/README.md) ROS 2 messages |
| **Robots** | [`arora-hal-ros2`](crates/arora-hal-ros2/readme.md)† · [`arora-hal-restful`](crates/arora-hal-restful/readme.md)† |
| **Front ends** | [`arora-web`](crates/arora-web/readme.md) the device in a browser · [`arora-cli`](crates/arora-cli/readme.md)† call a module from the command line |
| **Libraries** | [`arora-animation`](crates/arora-animation/readme.md) and the module [`@arora/animation`](modules/animation/readme.md) · [`arora-geometry`](crates/arora-geometry/README.md) · [`arora-graphics-types`](crates/arora-graphics-types/README.md) · [`arora-dataflow-nodes`](crates/arora-dataflow-nodes/README.md) |

### ROS 2 and ROS4HRI

ROS 2 meets a device on either side. As a bridge,
[`arora-bridge-ros2`](crates/arora-bridge-ros2/README.md) exposes the
device's keys as topics, its functions as services and its task runs as
actions, with exposure profiles that publish typed standard messages — among
them [ROS4HRI](https://ros.org/reps/rep-0155.html), the human-robot
interaction standard, whose `hri_msgs` types
[`arora-msgs-ros2`](crates/arora-msgs-ros2/README.md) carries. As a HAL,
[`arora-hal-ros2`](crates/arora-hal-ros2/readme.md) drives a robot that
already speaks ROS 2.

## Documentation

Read top to bottom:

1. [Concepts](docs/overview.md) — the ideas, one at a time.
2. [Architecture](docs/architecture.md) — the crates, the device, the engine,
   the module pipelines.
3. Topics: [building](docs/building.md), [testing](docs/testing.md),
   [records](docs/records.md), [dispatch](docs/dispatch.md),
   [long-running functions](docs/async-functions.md), the
   [runtime's data flow](crates/arora/docs/runtime-and-data-flow.md).
4. [Design decisions](docs/design_decisions.md) — why it is built this way.
5. Each crate's readme, for its API.

The other pages under `docs/` — proposals, plans, studies — are designs
under discussion or research, not descriptions of the code.

## License

Arora is released under the [Apache License, Version 2.0](LICENSE), copyright
Semio Community ([NOTICE](NOTICE)), except the example module
[`say-piper`](examples/policy-modules/modules/say-piper), which links Piper
(GPLv3) and is licensed GPL-3.0-or-later. Each published crate ships the
license and notice files.
