# arora-bridge-ros2

ROS 2 as an Arora bridge: exposes a device's keys over ROS 2 topics and accepts
commands from the graph, implementing [`arora_bridge::Bridge`].

An Arora device is one blackboard with four seams around it (store, HAL, bridge,
behavior). This crate is a **bridge** whose remote is a ROS 2 graph — ROS is the
external control/data plane into an Arora runtime. It is a sibling to
`arora-bridge-ws` (whose remote is a local WebSocket app) and is **distinct from
`arora-hal-ros2`**: there ROS is the device's own hardware (a HAL); here ROS is
the remote (a bridge).

## Topic convention

Keys map to topics under a namespace: the key `face/mouth` is the topic
`/{namespace}/keys/face/mouth`. Values speak the Arora data vocabulary
(`arora_types::Value`).

| Direction | Bridge method | ROS 2 |
| --- | --- | --- |
| Runtime → ROS | `send_data(StateChange)` | Publishes each changed key to its topic. The `std_msgs` type is chosen from the value's type (`Value::F64` → `std_msgs/Float64`, …); non-scalar values fall back to a JSON-encoded `std_msgs/String`. |
| ROS → Runtime | `commands()` | Each message received on a declared input topic becomes a `BridgeOp::Update` the runtime applies to its store. |

Input keys must be declared in the config (path + value type): a ROS 2 topic is
typed, and the subscription is created before any message arrives. Output keys
need no declaration — a publisher is created from each changed value's type on
first use.

## Delivery: only the latest value, twice over

The bridge publishes **state, not events**, and it says so at both layers.

*Above ROS.* The runtime hands the bridge one coalesced `StateChange` per step;
those go into a **latest-value-per-key** buffer that the node task drains. A key
written again before the drain replaces its pending value instead of queuing
behind it, so the memory the bridge holds is bounded by the device's key count
however far behind the ROS graph falls — and what it publishes is always the
freshest value, never a backlog of stale ones. A device that renders or steps
faster than ROS drains therefore publishes *fewer* samples, not later ones. The
count of values replaced before they were published is logged at `debug`.

*In ROS.* Each endpoint runs under a `Qos` profile, and the default follows the
flow:

| Flow | Default | Policy |
| --- | --- | --- |
| out (state: rig values, the face image) | `Qos::SensorData` | best-effort, volatile, keep-last-1 |
| in (commands: speech text, an expression) | `Qos::Reliable` | reliable, volatile, keep-last-1 |

State is only interesting at its newest value and a slow reader must not stall
the writer; a command is an instruction and dropping one loses it. Override per
endpoint with the `qos` field on `Endpoint`, `Include`, `InputKey`,
`TypedInput` and `TypedOutput`.

**Large values.** The scalar plane's JSON fallback will carry anything, but a
ROS 2 topic should not: RustDDS fragments samples over 1 KB, and an image-sized
one costs memory per sample inside the writer (see the 6.0.0 changelog). A value
serializing past 64 KB of JSON is warned about once per key. Declare such a key
as a typed output — an image belongs on `sensor_msgs/CompressedImage`.

**The trade on the outbound plane:** a best-effort writer does not match a
reliable reader. A subscriber left on the rclcpp/rclpy default (reliable) sees
nothing on a `Qos::SensorData` topic until it asks for best-effort — the same
bargain `image_transport` strikes for camera streams. `ros2 topic echo` needs
`--qos-reliability best_effort` on those topics.

**Type hashes on the Zenoh backend.** `rmw_zenoh` keys every publisher on its
message's REP-2016 type hash, and a native subscriber listens on that exact key,
so a publisher announcing the wrong hash is listed by `ros2 topic list`,
described by `ros2 topic info -v`, and never heard. A typed output is keyed on
the hash the bridge computes from the message's registry description — the same
value ROS 2 ships in the type's `.json` — so `ros2 topic echo`, `rqt_image_view`
and any rclcpp/rclpy node receive it. A message the hasher cannot describe still
publishes, but reaches only other `ros2-client` peers; that is warned about once
per key. Subscriptions match on a wildcard, so the inbound plane never depended
on the hash; DDS discovery carries none and is unaffected either way.

## Configuration

```rust,no_run
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type};

# async fn example() {
let config = Ros2BridgeConfig::new("robot", 0)
    .with_input("face/mouth/open", Type::F64)
    .with_input("enabled", Type::Boolean);
let bridge = Ros2Bridge::new(config).await;
// Hand `bridge` to the Arora runtime as its `Bridge`.
# let _ = bridge;
# }
```

`Ros2Bridge::new` spins a DDS node in a background task; call it from within a
Tokio runtime. Dropping the bridge stops the task.

## Bridge-method mapping

- `send_data(StateChange)` → publish each changed key to its topic.
- `commands()` → input-topic messages become `BridgeOp::Update`.
- `data_requested()` → yields `true` once (a ROS 2 graph is a data consumer; DDS
  exposes no clean per-subscriber claim/release toggle).
- `get_device_info` / `device_info_updated` / `update_device_info` → stubs; ROS 2
  has no device-registration concept.

## Methods as services

Where the topic plane mirrors a device's *keys*, the method plane mirrors its
*methods*. The bridge asks the runtime for every method's signature
(`BridgeOp::DescribeMethods`) and, for each one whose parameter and return types
are representable in ROS 2, stands up a service at `/{namespace}/methods/{name}`.
Discovery is automatic — a device's methods are its own surface, so nothing is
declared.

A method maps onto a `.srv` the way ROS already models one: the parameter list is
the **request** (one field per parameter), the return value is the **response**.
Both messages are synthesised as runtime types and driven through the shared CDR
codec (`arora_msgs_ros2::cdr`) against the registry — real ROS 2 message types on
the wire, no ad-hoc encoding. Signatures ROS 2 cannot represent are skipped and
reported, not silently dropped.

## Task runs as actions

Task-run methods — those whose return type is the behavior-tree `Status`
enumeration, i.e. tickable, long-running, cancellable work — are mirrored as ROS
2 **actions** instead. That enum is also the one return type the service plane
cannot carry, so actions claim exactly the methods services skip; the two planes
never overlap. A goal spawns the run (through `BridgeOp::Call` to the
interpreter's `SPAWN`), feedback and result are typed from what the run writes,
and cancel/status ride ros2-client's `RawActionServer`. The goal lifecycle lives
in a `GoalBook`.

Introspection over the bridge (`ListKeys` / `ListMethods`) and `BridgeOp::Call`
over topics remain unwired — services and actions are the method surface.

## ROS4HRI

This bridge is where a device meets a ROS 2 graph, so it is the natural home for
[ROS4HRI](https://ros.org/reps/rep-0155.html) interop. The ROS4HRI message vocabulary
(`hri_msgs/Expression`, `FacialActionUnits`, `Gaze`, …) is available as typed ROS
2 messages in [`arora-msgs-ros2`](../arora-msgs-ros2/README.md), and the Vizij
face standard consumes it (see [vizij-rs's ROS4HRI
docs](https://github.com/vizij-ai/vizij-rs/blob/main/docs/ros4hri.md)).

Typed topics bind per endpoint: `with_typed_input`/`with_typed_output` (and
their `_on` variants for absolute topic names) subscribe or publish a device
key as a registered ROS message, decoded and encoded against its runtime type.

**Exposure profiles** (`profile` module) bundle a whole surface: an
[`ExposureProfile`] holds typed endpoints on absolute topics with per-field
fan-out over device keys, glob includes (`*` one segment, `**` the rest)
whose prefix rewrites expose bulk keys on the scalar plane under absolute
names, and **action bindings** that serve a device task-run method as a
standard ROS 2 action — the skill plane. `ExposureProfile::ros4hri()` ships
the ROS4HRI face surface for both incumbent name sets — PAL (`/robot_face/*`)
and IIIA (`/expressive_face/*`): expression commands fan out to
`standard/ros4hri/expression/*`, `look_at` points land as the gaze target
(vec3) and frame, speech text feeds the lipsync key, and the two standard
skills spawn the device's task runs — `interaction_skills/LookAt` on
`/skill/look_at`, and `communication_skills/Say` on `/skill/say`, whose goal
`input` is the utterance and whose feedback carries what the run reports (for
a face, the viseme at the audio playhead). The rendered face publishes on the
`image_transport` pair PAL OS documents — `display/face` as a
`sensor_msgs/Image` on `/robot_face/image_raw`, `display/face/compressed` as a
`sensor_msgs/CompressedImage` on `/robot_face/image_raw/compressed`; a face
writes the key of the transport it encodes. Enabling it is one call:

```rust
let config = Ros2BridgeConfig::new("robot", 0)
    .with_profile(ExposureProfile::ros4hri());
```

An action binding is the exterior contract of a skill: at startup the bridge
checks it against the device's described methods (the function exists, is a
task run, and every goal field routes onto a parameter of a compatible type)
and refuses it loudly otherwise. A parameter the goal does not name is left to
the method's own default and logged — a standard contract carries what the
standard says, not every parameter an implementation happens to take. A bound action serves one goal at a time —
`std_skills/Meta.priority` arbitrates, an equal-or-higher replacement
preempting the active run (its result reports `ROS_EINTR`) and a lower one
being rejected — and answers with the standard Result message carrying the
`std_skills` errno of the goal's lifecycle, unless the run wrote the Result
(or an errno) itself.

`ExposureProfile::coverage` reports which of the profile's keys and skill
functions a device does not serve, so a deployment checks a face against its
profile up front instead of discovering holes topic by topic.
