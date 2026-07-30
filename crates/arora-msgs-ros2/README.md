# arora-msgs-ros2

The ROS 2 message types for Arora. It lets an Arora device speak ROS 2: it
carries the standard message packages as arora types, a registry that maps them
by name and id, and the CDR codec that moves a value on and off the ROS 2 wire.
It is shared by [`arora-bridge-ros2`](../arora-bridge-ros2) (ROS as a device's
remote data plane) and [`arora-hal-ros2`](../arora-hal-ros2) (ROS as a device's
own hardware), so both agree on one set of message types and one codec.

## One type, four faces

Every ROS message reduces to a single thing — an arora `ty::low::Type` carrying
its ROS-qualified name (`geometry_msgs/msg/Point`) — from which everything else
follows:

| face | how |
|---|---|
| **on the wire** | `cdr::encode` / `cdr::decode` turn a `Value` into ROS 2 CDR bytes and back, type-directed by the `low::Type` — no per-message Rust needed |
| **its identity** | `rihs01` (the `interop` feature) computes the message's REP-2016 type hash, so an arora publisher reaches native C++ ROS nodes |
| **in Rust** | for the bundled messages, a generated struct (`geometry_msgs::Point`) you hold, mutate, and (de)serialize with `serde` — what ros2-client's typed pub/sub uses |
| **by name** | `Ros2Registry` resolves `"geometry_msgs/Point"` to the type — what a node editor needs to type a key |

The type id is `gen_uuid_from_str("geometry_msgs/msg/Point")` — a pure function
of the ROS name, so a message has the **same** id whether it is a bundled type
or [defined at runtime](#types-defined-at-runtime), and across builds.

## The bundled messages

Generated at build time from the vendored `.msg` files under `msgs/` (ROS 2
Jazzy), one Rust module per ROS package:

```
arora_msgs_ros2::builtin_interfaces::Time
arora_msgs_ros2::std_msgs::Header
arora_msgs_ros2::geometry_msgs::Point
arora_msgs_ros2::sensor_msgs::Imu
arora_msgs_ros2::hri_msgs::LiveSpeech
```

`.msg` constants become associated `const`s (`sensor_msgs::BatteryState::POWER_SUPPLY_STATUS_FULL`).
`arora_msgs_ros2::registry()` returns a `Ros2Registry` with every bundled type.

The `hri_msgs` package is [ROS4HRI](https://ros.org/reps/rep-0155.html)'s human-robot-interaction
vocabulary — `Expression`, `FacialActionUnits`, `Gaze`, `LiveSpeech`, and the
rest — the message layer Vizij's face standard is built on (see
[vizij-rs](https://github.com/vizij-ai/vizij-rs/blob/main/docs/ros4hri.md)).

**To add a message**, drop its `.msg` into `msgs/<package>/` and rebuild — the
`build.rs` parser and code generator do the rest. Its dependency packages must
be present too (the closure here is builtin_interfaces + std/geometry/sensor/hri).

## Types defined at runtime

The registry is not limited to the bundled messages:

- a ROS developer adds a type that was never a `.msg` here —
  `registry.define_from_msg("my_msgs", "Reading", "std_msgs/Header header\nfloat64 value")`;
- a behavior loads a type it authored on the fly — `registry.define_from_json(blob)`,
  where the blob is a serialized `low::Type`.

Either joins the registry with exactly the id it would have as a bundled message,
so a topic typed with it is immediately encodable, hashable, and — with a ROS
bridge enabled — speakable to real ROS clients.

## Features

The core — the codec, the registry, the generated types, `ros2_representable` —
depends on **no** ROS middleware, so a modeling consumer (a node editor, a wasm
build) stays lean. The `dds` / `zenoh` features add `interop`: the REP-2016 hash
and the live-ROS surface, pulling `ros2-client` with the matching backend.

```toml
arora-msgs-ros2 = { path = "../arora-msgs-ros2" }                        # core only
arora-msgs-ros2 = { path = "../arora-msgs-ros2", features = ["zenoh"] }  # + hash
```

## Layout

- `msgs/` — the vendored `.msg` sources (the source of truth).
- `build.rs` + `schema.rs` — the `.msg` parser (shared with runtime `define_from_msg`) and the code generator.
- `cdr.rs` — the ROS 2 CDR codec, a backend of the `arora_types::value_serde` walk.
- `registry.rs` — `Ros2Registry` and the runtime type-definition paths.
- `representable.rs` — `ros2_representable`, the "can this ride ROS 2" predicate.
- `hash.rs` (`interop`) — the REP-2016 `RIHS01_…` type hash.
- `tests/use_cases.rs` — the five worked scenarios above, as runnable tests.
