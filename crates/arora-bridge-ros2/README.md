# arora-bridge-ros2

A bridge ([`arora_bridge::Bridge`](https://docs.rs/arora-bridge)) whose remote
is a ROS 2 graph: a running Arora device appears to ROS as topics, services and
actions, so any ROS 2 node reads its state, writes its inputs and calls its
functions with standard ROS tools.

Here ROS is the device's remote. When ROS is the device's own hardware, the
HAL [`arora-hal-ros2`](../arora-hal-ros2/readme.md) is the crate instead.

## Three planes

The bridge mirrors what the device has, asked of the device — nothing is
declared to it:

| Device | ROS 2 | Name |
|---|---|---|
| a key | a topic | `/{namespace}/keys/{path}` |
| a function | a service | `/{namespace}/methods/{name}` |
| a task run (a function returning the behavior `Status`) | an action | `/{namespace}/actions/{name}` |

The bridge asks the device for its inputs and functions when it starts, and
again every two seconds. What the device gains in between — an input it
opens, a key its HAL describes, a module it loads — gets its topic, service or
action when the answer arrives; what it loses — an input it closes, a module
it unloads — loses its endpoint then. The asks go through the device's own
command channel, in process, and an answer equal to the last one changes
nothing.

On top of these, an **exposure profile** serves a standard surface — typed
messages on well-known names, such as ROS4HRI's — over the device's keys and
functions (see [Exposure profiles](#exposure-profiles)).

## Smallest use

```rust,no_run
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig};

# async fn example() {
let bridge = Ros2Bridge::new(Ros2BridgeConfig::new("robot", 0)).await;
// arora::Arora::builder().with_bridge(Box::new(bridge))
# }
```

`Ros2BridgeConfig::new(namespace, domain_id)` names the topics' namespace and
the ROS domain. `Ros2Bridge::new` starts a ROS 2 node on a background task, so
call it inside a Tokio runtime; dropping the bridge stops the node. A node
that cannot start is logged, and leaves the bridge inert.

A device can hold this bridge beside others, such as the open local bridge;
each serves the same store.

## Keys as topics

Every key the device sets is published on its topic, as the `std_msgs` type
of its first value: `f64` as `std_msgs/Float64`, likewise `f32`, `i64`, `i32`,
`u64`, `u32`, `bool` and `string`. Any other value — an array, a structure, a
`u8` — is published as JSON in a `std_msgs/String`. A key that a profile's
typed output or include claims publishes there instead.

The device's **inputs** are the keys its store opens to remote writers
(`KeyMeta::editable`): the bridge subscribes each on its topic, typed from the
key's meta, and turns each message into a write of that key. The device checks
the write as it checks every remote write, and the bridge logs a refusal (at
most once per topic and set of keys every five seconds). An input whose meta
states no type gets no topic. The bridge publishes nothing on a topic it
subscribes to: a key it publishes stops when its topic becomes an input's.

A ROS name allows only `[A-Za-z0-9_]` in each `/`-separated token, so the
bridge escapes a key path token by token: any other character becomes `_`, a
run of underscores collapses to one, a token starting with a digit gains a
leading `_`, and an empty token becomes `_`. `rig/poses/pose_b.weight` is the
topic `/{namespace}/keys/rig/poses/pose_b_weight`. A path that is already a
valid name is its topic as is. Two keys can escape to one topic (`a.b` and
`a-b`): the key that sorts first keeps it, and the other gets no topic and is
reported once. The bridge keeps the topic of each key, and never reverses the
escape.

### Delivery: the newest value

The bridge publishes state, not events. Each step's changes go into a buffer
holding the newest value of each key, which the node drains: a key written
again before it was published replaces its pending value, so a device that
changes faster than ROS drains publishes fewer, fresher samples, and the
buffer is bounded by the device's key count.

Each endpoint runs under a `Qos`, by default after its direction:

| Direction | Default | Policy |
|---|---|---|
| out — state the device publishes | `Qos::SensorData` | best-effort, volatile, keep-last-1 |
| in — commands the device receives | `Qos::Reliable` | reliable, volatile, keep-last-1 |

A best-effort writer matches no reliable reader, so a subscriber on the
rclcpp/rclpy default sees nothing on an output topic until it asks for best
effort (`ros2 topic echo --qos-reliability best_effort …`). Typed endpoints,
includes and topic skills take a `qos` of their own.

A value whose JSON passes 64 KB is warned about once per key: an image belongs
on a typed output (`sensor_msgs/CompressedImage`), not on the JSON fallback.

## Functions as services

For each of the device's functions whose parameters and return type ROS 2 can
represent, the bridge serves a service. Its request has one field per
parameter, its response the return value, both built at run time from the
function's signature and carried as real ROS 2 messages through the CDR codec
of [`arora-msgs-ros2`](../arora-msgs-ros2/README.md). An optional parameter or
return travels as the bounded sequence `T[<=1]`: empty is absent. A function
ROS 2 cannot represent is skipped, and logged.

## Task runs as actions

A function that returns the behavior `Status` reports `Running`, `Success` or
`Failure` across ticks: the device hosts a call to it as a task run, and the
bridge serves it as an action instead of a service. A goal spawns the run;
the feedback and result are typed from what the run writes; cancelling a goal
halts the run.

## Exposure profiles

A profile (`ExposureProfile`, added with `Ros2BridgeConfig::with_profile`)
binds a standard surface to the device:

- **Typed endpoints** (`Endpoint`): a topic of a registered message type on an
  absolute name. Inbound, a message's fields land on device keys, all in one
  change; outbound, the message is composed from device keys and published
  again whenever one of them changes. Each `FieldRoute` pairs a dotted field
  path with a key.
- **Includes** (`Include`): keys matching a glob (`*` one segment, `**` the
  rest) published under a rewritten absolute name, on the scalar plane.
- **Action bindings** (`ActionBinding`): a standard `.action` on an absolute
  name, whose goal spawns a device task run, the goal's fields routed onto the
  function's parameters. A bound action serves one goal at a time:
  `std_skills/Meta.priority` arbitrates, an equal or higher priority preempting
  the active run and a lower one being rejected.
- **Topic skills** (`TopicSkillBinding`): a topic whose every message spawns a
  device task run, for a skill that sets a state held until changed. The latest
  message wins: the run the previous message spawned is halted first.

The bridge checks each binding against the device's functions when it starts
and whenever they change — the function exists, is a task run, and every
routed field lands on a parameter of a compatible type — and refuses it,
logged, while it does not hold: a binding whose function a module loaded
later provides is served from then on. A parameter no field
names is left out of the call. `ExposureProfile::coverage` lists what of a
profile a device does not serve.

Single endpoints also have builder methods: `with_typed_input`,
`with_typed_output`, and their `_on` forms for an absolute topic name.

A route's dotted path reads a message: a name descends into a field
(`expression.valence`), a number takes one element of a sequence
(`visemes.0.value`), `*` takes every element and collects what the rest
reaches into one array (`timeline.visemes.*.viseme.value`), a geometry point
or vector lands as an `[f32; 3]`, and a `builtin_interfaces/Duration` as its
seconds.

### ROS4HRI

`ExposureProfile::ros4hri()` serves the [ROS4HRI](https://ros.org/reps/rep-0155.html)
face surface under PAL's `/robot_face/*` names:

```rust,no_run
use arora_bridge_ros2::{ExposureProfile, Ros2BridgeConfig};

let config = Ros2BridgeConfig::new("robot", 0).with_profile(ExposureProfile::ros4hri());
```

| Name | Served as | ROS type | Device |
|---|---|---|---|
| `/skill/set_expression` | topic skill | `interaction_skills/SetExpression` | `set_expression(expression, valence, arousal)` |
| `/skill/set_facial_action_units` | topic skill | `vizij_face_msgs/SetFacialActionUnits` | `set_facial_action_units(codes, intensities)` |
| `/skill/set_viseme` | topic skill | `vizij_face_msgs/SetViseme` | `set_viseme(viseme)` |
| `/skill/look_at` | action | `interaction_skills/LookAt` | `look_at(policy, target, frame)` |
| `/skill/say` | action | `communication_skills/Say` | `say(text)` |
| `/skill/play_visemes` | action | `vizij_face_msgs/PlayVisemes` | `play_visemes(visemes, starts, durations)` |
| `/robot_face/look_at` | in | `geometry_msgs/PointStamped` | `standard/ros4hri/gaze/target`, `…/gaze/frame` |
| `/tts/viseme`, `/tts/visemes` | in | `hri_msgs/Viseme`, `hri_msgs/Visemes` | `standard/ros4hri/viseme` (and its `/time`, `/duration`) |
| `/closed_captions` | in | `hri_actions_msgs/ClosedCaption` | `standard/ros4hri/caption/text`, `…/caption/speaker` |
| `/robot_face/speech` | out | `std_msgs/String` | `standard/ros4hri/speech/text` |
| `/robot_face/viseme` | out | `std_msgs/String` | `standard/ros4hri/speech/viseme` |
| `/robot_face/image_raw` | out | `sensor_msgs/Image` | `display/face` |
| `/robot_face/image_raw/compressed` | out | `sensor_msgs/CompressedImage` | `display/face/compressed` |
| `/diagnostics` | out, reliable | `diagnostic_msgs/DiagnosticArray` | `diagnostics` |

A face that reads its expression from the `standard/ros4hri/expression/*` keys
rather than implementing `set_expression` takes
`ExposureProfile::ros4hri().with_set_expression_on_keys()`. The
`communication_skills/Say` this profile serves is the vendored one, whose
feedback differs from upstream's (see
[arora-msgs-ros2](../arora-msgs-ros2/README.md#departures-from-upstream)).

## Reference

| Feature | Effect |
|---|---|
| `dds` (default) | The DDS middleware backend. |
| `zenoh` | The `rmw_zenoh`-compatible backend: build with `--no-default-features --features zenoh`. |

On the Zenoh backend, the session reads its configuration from
`ZENOH_CONFIG_OVERRIDE` or `ZENOH_SESSION_CONFIG_URI`, for instance
`ZENOH_CONFIG_OVERRIDE='mode="client";connect/endpoints=["tcp/localhost:7447"]'`
to join a `zenohd` router. `rmw_zenoh` matches a publisher to a subscriber by
its message's REP-2016 type hash: the bridge keys a typed output on the hash
of its registered message, so native nodes receive it; a message the hasher
cannot describe still publishes, reaching only other `ros2-client` peers, and
is warned about once. `examples/zenoh_probe.rs` exercises a device's keys
against a real ROS 2 stack over Zenoh.

Limits:

- A ROS client cannot list the device's keys or call a function by name
  through this bridge: the topics, services and actions are its surface.
- The bridge has no device registration: `get_device_info` answers none.

`cdr` re-exports the CDR codec of
[`arora-msgs-ros2`](../arora-msgs-ros2/README.md), and `Type` and `Value` the
Arora value vocabulary.
