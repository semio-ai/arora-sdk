# arora-hal-ros2

A HAL ([`arora-hal`](../arora-hal/readme.md)) that drives a ROS 2 robot: the
robot's topics become the device's keys, so an Arora device runs on any robot
that speaks ROS 2. Here ROS is the device's own hardware; to expose a device to
a ROS 2 graph instead, use the bridge
[`arora-bridge-ros2`](../arora-bridge-ros2/README.md).

The crate is not published on crates.io: build it from this workspace.

## Run a robot

The `arora-ros2` binary runs a robot as an Arora device:

```sh
cargo run -p arora-hal-ros2 --features runner --bin arora-ros2 -- nao
```

The robot is a built-in name — `nao`, `pepper`, `quori`, `ur3`, `ur5`, `g1` —
or the path of a robot configuration (a JSON `ROS2RobotConfig`, below). A
built-in robot maps its joints from its 3D model: put the robot's GLB at
`crates/arora-hal-ros2/models/<robot>.glb`, or name another file with
`model_glb_path` in an overrides file (`arora-ros2 nao overrides.json`).
Nothing fills `models/` at build time.

The device serves the open local bridge on `ws://127.0.0.1:9000`
([`arora-bridge-ws`](../arora-bridge-ws/README.md)), where any local client
lists its keys and writes its inputs.

## How topics become keys

`Ros2Hal::new(config)` builds the HAL from a `ROS2RobotConfig`: a list of
`TopicConfig`s, each a topic `name`, its `message_type`, a `direction`
(`Publish`, `Subscribe` or `Both`) and a `mapping` that says how the message
maps to keys. A joint's key entity is its Arora id, which `joint_ids` maps
from its ROS name.

| Mapping | Received | Published from |
|---|---|---|
| `JointState` | from a `sensor_msgs/JointState`: `<joint>.position`, `<joint>.velocity`, `<joint>.effort` | each joint's position target, as a `sensor_msgs/JointState` or a `std_msgs/Float64MultiArray` |
| `JointAngles` (`naoqi_bridge_msgs/JointAnglesWithSpeed`) | — | each joint's position target, at a fixed speed of 0.3 |
| `Float64MultiArray` | `<joint>.position` | each joint's position target |
| `JointTrajectory` (`trajectory_msgs/JointTrajectory`) | — | `common.target_trajectory`, a key-value with `jointNames` and `points` |
| `StandardMessage` (`std_msgs/String`) | the key `data` maps to | the key `data` maps to |

A joint's position target is `<joint>/target.position` or
`<joint>.target_position`, the HALs' target keys (see
[arora-hal](../arora-hal/readme.md#joint-targets)); a command names joints at
the root of the key tree only.

The HAL describes these keys (`Hal::describe_keys`) from its configuration:
the joints' position targets, the trajectory key and a published text as
inputs, open to remote writers; the sensed joint values and a received text as
outputs. A device over it opens its inputs with no configuration of its own.

## A robot of your own

A JSON file deserializes to `ROS2RobotConfig`:

```json
{
  "model_family": "my_arm",
  "domain_id": 0,
  "topics": [
    {
      "name": "/joint_states",
      "message_type": "sensor_msgs/JointState",
      "direction": "Subscribe",
      "mapping": { "JointState": { "conversion": "Standard" } }
    },
    {
      "name": "/forward_position_controller/commands",
      "message_type": "std_msgs/Float64MultiArray",
      "direction": "Publish",
      "mapping": { "JointState": { "conversion": "Standard" } }
    }
  ],
  "joint_ids": { "Override": { "shoulder_joint": "shoulder", "elbow_joint": "elbow" } }
}
```

```sh
cargo run -p arora-hal-ros2 --features runner --bin arora-ros2 -- my_arm.json
```

`joint_ids` is `"FromGLB"` (the default: the joint names of the model at
`model_glb_path`), `{"Override": {…}}` (this map alone) or `{"Extend": {…}}`
(the model's names, then this map), each map from a ROS joint name to its
Arora id. The robot's model is served to a remote only when
`model_glb_servable` is `true`; leave it `false` for a model held under a
private grant.

As a library, `Ros2Hal::new(config).await` must run inside a Tokio runtime
that outlives the HAL, and the HAL goes to the device with
`arora::Arora::builder().with_hal(Box::new(hal))`.

## Reference

The runner's command line:

```text
arora-ros2 [--module <DIR>]... [--device <NAME>] [--device-dir <PATH>]
           [--bridge-port <PORT>] [--no-studio] <ROBOT> [OVERRIDES]
```

| Argument | Environment | Meaning |
|---|---|---|
| `<ROBOT>` | `ROBOT` | A built-in robot name, or a `ROS2RobotConfig` JSON file. |
| `[OVERRIDES]` | — | A JSON file whose set fields override the robot's configuration. |
| `--module <DIR>` | — | A module directory to load, besides the device directory's `modules/`. Repeatable. |
| `--device <NAME>` | `ARORA_DEVICE` | The device's name on this machine (`default` when unset). |
| `--device-dir <PATH>` | `ARORA_DEVICE_DIR` | The device directory, instead of the per-user directory of its name. |
| `--bridge-port <PORT>` | `ARORA_BRIDGE_PORT` | The open local bridge's port (9000 when unset). |
| `--no-studio` | — | Only in a build with the `studio` feature: serve the open local bridge instead of the device's Semio Studio bridge. |
| — | `ROS_DOMAIN_ID` | Overrides the configuration's ROS domain id. |

| Feature | Effect |
|---|---|
| `dds` (default) | The DDS middleware backend. |
| `zenoh` | The `rmw_zenoh`-compatible backend: build with `--no-default-features --features runner,zenoh`. |
| `runner` | The `arora-ros2` binary. The library needs none of it. |
| `studio` | Opt-in Semio Studio connection for the runner: a run serves the device's Studio bridge unless it passes `--no-studio`. The device runs at once; one that is not enrolled with Studio enrolls while it runs, by the token in `STUDIO_ENROLLMENT_TOKEN` or a code: the terminal UI asks whether to enroll, the device's name and model family, shows the code in its prompt line, then the device's Studio record and id in its header; headless, the code is logged. A refused enrollment, or a halted Studio bridge (such as a credential of another Firebase project, naming the credentials directory to delete), fails the run. |

Message types come from [`arora-msgs-ros2`](../arora-msgs-ros2/README.md),
shared with the ROS 2 bridge.
