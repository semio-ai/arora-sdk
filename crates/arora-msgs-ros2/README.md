# arora-msgs-ros2

The ROS 2 messages as Arora types. An Arora value travels on a ROS 2 wire only
as a real ROS message, and this crate gives the two sides one vocabulary: the
standard message packages as Arora types, a registry that finds them by name,
and the CDR codec that moves a value on and off the wire. The ROS 2 bridge
([`arora-bridge-ros2`](../arora-bridge-ros2/README.md)) and the ROS 2 HAL
([`arora-hal-ros2`](../arora-hal-ros2/readme.md)) both speak through it.

## One type per message

Every ROS message is one Arora type description, a `low::Type`
(`arora_types::ty::low::Type`) carrying the message's ROS name
(`geometry_msgs/msg/Point`). Everything else follows from it:

| To | Use |
|---|---|
| put a value on the wire | `cdr::encode` / `cdr::decode` turn a `Value` into ROS 2 CDR bytes and back, directed by the type: no Rust per message. |
| find a type by name | `Ros2Registry` resolves `"geometry_msgs/Point"` (or `geometry_msgs/msg/Point`) to its type and id. |
| hold a message in Rust | each bundled message is a generated struct (`geometry_msgs::Point`) with `serde`, implementing `RosMessage`. |
| reach native ROS nodes over Zenoh | `rihs01` (under the `dds` or `zenoh` feature) computes the message's REP-2016 type hash. |

A message's type id is the UUID of its ROS name
(`gen_uuid_from_str("geometry_msgs/msg/Point")`), so it is the same for a
bundled message and for one defined at run time, and across builds.

## Smallest use

A `geometry_msgs/Point`, found by name, to CDR bytes and back:

```rust
use arora_msgs_ros2::{cdr, geometry_msgs::Point};
use arora_types::value_serde::bridge::to_value_seeded;
use arora_types::AroraType;

let registry = arora_msgs_ros2::registry();
let ty = registry.get_by_name("geometry_msgs/Point").unwrap();

let (point_ty, point_types) = Point::arora_type_with_registry();
let value = to_value_seeded(&Point { x: 1.0, y: 2.0, z: 3.0 }, &point_ty, &point_types).unwrap();

let bytes = cdr::encode(ty, registry.types(), &value).unwrap();
assert_eq!(cdr::decode(ty, registry.types(), &bytes).unwrap(), value);
```

`tests/use_cases.rs` walks through the five uses: a bridge skipping a value ROS
cannot carry, a type defined at run time, a message held in Rust, a type picked
by name, and a type defined on the fly and saved.

## The bundled messages

`build.rs` generates one Rust module per ROS package from the vendored `.msg`
and `.action` files under `msgs/` (ROS 2 Jazzy): `builtin_interfaces`,
`std_msgs`, `geometry_msgs`, `sensor_msgs`, `trajectory_msgs`,
`diagnostic_msgs`, `naoqi_bridge_msgs`, `hri_msgs`, `hri_actions_msgs`,
`std_skills`, `interaction_skills`, `communication_skills` and
`vizij_face_msgs`. A `.msg` constant becomes an associated `const`
(`sensor_msgs::BatteryState::POWER_SUPPLY_STATUS_FULL`), and `registry()`
returns a `Ros2Registry` holding every bundled type.

- `hri_msgs` is [ROS4HRI](https://ros.org/reps/rep-0155.html)'s
  human-robot-interaction vocabulary (`Expression`, `FacialActionUnits`,
  `Gaze`, `LiveSpeech`, …), the message layer Vizij's face standard is built
  on (see [vizij-rs](https://github.com/vizij-ai/vizij-rs/blob/main/docs/ros4hri.md)).
  ROS4HRI's skill packages (`std_skills`, `interaction_skills`,
  `communication_skills`) and `hri_actions_msgs/ClosedCaption` sit beside it.
- `vizij_face_msgs` is Vizij's own package: a face's content (action units,
  expression, viseme) as types shared by perception, commands and state, with
  observation and timing as separate wrappers, and the
  `SetFacialActionUnits`, `SetViseme` and `PlayVisemes` skills on them.

To add a message, put its `.msg` under `msgs/<package>/` and rebuild; the
packages it depends on must be there too.

A `.msg` `byte`, `char` and `uint8` are all an Arora `u8`, with the same CDR
bytes, but REP-2016 hashes `byte` as a type of its own. The build records
which bundled fields are `byte`, so they hash as ROS 2 does; a type defined at
run time has no such record, and its `u8` fields hash as `uint8`.

## Departures from upstream

The vendored files are the standards' own, byte for byte, except these. A
changed message has another REP-2016 hash than upstream's, so on the Zenoh
backend, which matches endpoints by hash, a node built from the upstream
package does not match an endpoint of the changed message.

| File | Upstream | Departure | Upstream status |
|---|---|---|---|
| `communication_skills/Say.action` | [ros4hri/communication_skills](https://github.com/ros4hri/communication_skills/blob/main/action/Say.action) | The feedback carries `string viseme` and `float32 intensity` after the standard's `std_skills/Feedback feedback`: the mouth shape the utterance makes as it plays. A goal from an upstream client still parses; its feedback endpoint's hash differs from upstream's. | Not proposed. |
| `hri_msgs/Viseme.msg`, `hri_msgs/Visemes.msg` | None: [ros4hri/hri_msgs](https://github.com/ros4hri/hri_msgs/tree/master/msg) has no viseme message. | New files: `Viseme` carries the 15-shape constants, a `value`, and the `time` and `duration` that place it in the utterance; `Visemes` is a sequence of them. They are the fields and constants that ROS4HRI TTS nodes publish on `/tts/viseme` and `/tts/visemes`. | Not proposed. |
| `vizij_face_msgs/*` | None: Vizij's own package. | A new package, so no standard type changes. | Written up as a counter-proposal for the standard (vizij-docs, `ros4hri_face/face-interfaces-proposal.md`). |

A device serves the departed `Say` only under the ROS4HRI exposure profile of
the ROS 2 bridge.

## Types defined at run time

The registry also takes types that were never bundled:

- from the text of a `.msg`:
  `registry.define_from_msg("my_msgs", "Reading", "std_msgs/Header header\nfloat64 value")`;
- from a JSON array of `low::Type`s, the type and every type it nests, as a
  behavior saves a type it authored: `registry.define_from_json(json)`;
- from a type and its dependencies: `registry.define(ty, deps)`, or any
  `#[derive(AroraType)]` type with `registry.register_arora::<T>()`.

Each joins with the id it would have as a bundled message, so a topic of that
type is at once encodable, hashable and, through the ROS 2 bridge, reachable by
ROS clients. `ros2_representable` says whether a type can ride ROS 2 at all
(and `NotRepresentable` why not).

## Features

The core — codec, registry, generated types, `ros2_representable` — depends on
no ROS middleware, so a modelling consumer (an editor, a wasm build) stays
lean.

| Feature | Effect |
|---|---|
| `dds` | The REP-2016 hash (`rihs01`, `service_rihs01`, `action_hashes`, `type_hash::<T>()`), with `ros2-client` on its DDS backend. |
| `zenoh` | The same, with `ros2-client` on its Zenoh backend. |
| `interop` | What `dds` and `zenoh` turn on; not usable alone, as `ros2-client` needs exactly one backend. |

## Layout

- `msgs/`: the vendored `.msg` and `.action` sources, the source of truth.
- `build.rs` and `src/schema.rs`: the `.msg` parser, shared with
  `define_from_msg`, and the code generator.
- `src/cdr.rs`: the ROS 2 CDR codec, a backend of the
  `arora_types::value_serde` walk.
- `src/registry.rs`: `Ros2Registry` and the run-time definitions.
- `src/representable.rs`: `ros2_representable`.
- `src/hash.rs` (`interop`): the REP-2016 `RIHS01_…` type hash.
