# arora-hal

The hardware abstraction layer of [Arora](https://github.com/semio-ai/arora-sdk):
the boundary between a device's runtime and its hardware. A HAL presents the
hardware's sensors and actuators as values at keys, so the device's behaviors
and its remotes never talk to the hardware themselves.

## The HAL

A **key** is a `/`-separated path (`arm/elbow.position`) and a **value** an
Arora `Value` (`arora_types::value::Value`). A `Hal` is the device's one source
of hardware state and its one sink for actuation:

- what the hardware senses, it reports as a stream of state changes
  (`updates`);
- what the device wants actuated, it receives as a state change, without
  blocking (`try_send`);
- what its keys are, it describes (`describe_keys`), so the device knows which
  ones a remote may write.

The runtime ([`arora`](https://docs.rs/arora)) owns the HAL and drives it once
per step: it applies the HAL's reported changes to the device's store, ticks the
behavior, then hands the HAL everything that changed in that step, minus the
keys the HAL itself just reported. A HAL acts on the keys it knows and ignores
the rest, as hardware ignores what it has no actuator for.

## Smallest use

`FakeHal` is an in-memory HAL for tests and hardware-less runs. A joint's
setpoint written to it is sensed back as the joint's position:

```rust
use arora_hal::{FakeHal, Hal};
use arora_types::data::{Key, StateChange};
use arora_types::value::Value;

# async fn demo() {
let hal = FakeHal::new();
hal.add_joints(["head_yaw"]);
hal.write(StateChange::set("head_yaw/target.position", Value::from(0.5)))
    .await
    .unwrap();
assert_eq!(
    hal.read(&[Key::from("head_yaw.position")]).await.unwrap(),
    vec![Some(Value::from(0.5))]
);
# }
```

A device runs on it with `arora::Arora::builder().with_hal(Box::new(hal))`.
`FakeHal` clones share their state, so a test keeps a clone to watch what the
device wrote.

## Writing a HAL

| Method | What it does |
|---|---|
| `describe` | The device's `HalDescription`: model family, hardware and software versions. |
| `read` / `read_all` | The current values of some keys, or of every key the HAL exposes. |
| `write` | Apply a change, awaiting the hardware. |
| `try_send` | Push a change toward the hardware **without blocking**: the runtime calls it from its synchronous step. A HAL whose I/O is real (HTTP, DDS) enqueues onto a task of its own and returns. |
| `updates` | A stream of the changes the hardware reports. Each call is an independent subscription; the stream ending means the hardware feed is gone. |
| `describe_keys` | A stream of key descriptions (see below). Default: none. |
| `assets` | The device's 3D models, when the HAL has them (see below). Default: none. |

The trait takes `&self` everywhere and depends on no async runtime: a HAL owns
whatever task its I/O needs. `HalError` (`Broken`, `NoSuchKey`, `Other`) is
its error.

## Inputs and outputs

A key is closed to remote writers unless the device opens it. A HAL opens its
inputs by describing them: each item of `describe_keys` maps keys to their
`KeyMeta` (`arora_types::data::KeyMeta`) — the type, range, unit, rest value
and description of each, and whether it is `editable`. A HAL's inputs (the
setpoints it reads from a write) are editable; its outputs (what it senses) are
not.

The runtime states each item to the device's store (`DataStore::set_meta`):
the items ready when the device is built, then each later one at the next step.
Every bridge relays that meta to its clients as `__meta`, and accepts a remote
write only to an editable key, of the type the meta states. A HAL that learns
its keys as it runs (a discovered topic, a fetched configuration) yields one
more item when it learns them. A key the device's host stated meta for keeps
the host's; a key the HAL does not describe stays as the store says, closed
unless the host opens it (`arora --open`, or a subtree with
`DataStore::set_prefix_meta`).

## Joint targets

The HALs in this workspace share one naming contract for a joint. For the
component `c` of the entity at path `k`, the measured value is `k.c`
(`head_yaw.position`) and its target — the setpoint written to the hardware —
takes either of two keys:

- `k/target.c` (`head_yaw/target.position`), the target as an entity of its own;
- `k.target_c` (`head_yaw.target_position`), the component prefixed with
  `target_`.

`target_keys` names both keys, `set_targets` reads the targets a change sets in
either form (where one change sets both, `k/target.c` wins), and
`fold_newest_wins` merges a backlog of changes so the later target wins in
either form. This is a convention among HALs, nothing more: to the store and
the bridges a key is a path, and a `.` one more character of its name.

## Device models

A HAL that knows the 3D model of its hardware implements `HalAssets` and
returns it from `Hal::assets`. A HAL is made of named components, fixed for its
life; one that is not composed is the single component `DEVICE`. Each
component with a model states it as a `ComponentModel`: the published release
it uses (`ModelReference`), the `content_hash` of the GLB the device holds,
whether the device serves those bytes (`servable`), and where it is drawn on
another component's model (`Mount`). A model held under a private grant is
never servable, because anything that can call the device receives what it
serves.

The runtime serves the models as functions of a host module, under the ids in
`hal_module`: `MODELS` (every component's model), `MODEL_GLB` (one servable
component's bytes, by name) and `MODEL_GLBS` (every servable model, as
`ComponentGlb`s). A remote finds them in the device's method list and calls
them like any module function.

## Implementations

- `FakeHal`, here.
- [`arora-hal-ros2`](https://github.com/semio-ai/arora-sdk/tree/main/crates/arora-hal-ros2):
  a ROS 2 robot, its topics as keys.
- [`arora-hal-restful`](https://github.com/semio-ai/arora-sdk/tree/main/crates/arora-hal-restful):
  a robot driven over an HTTP API.
- [arora-aldebaran](https://github.com/semio-ai/arora-aldebaran): NAO and
  Pepper over NAOqi.

Part of the device runtime interfaces, with
[`arora-bridge`](https://docs.rs/arora-bridge) and
[`arora-behavior`](https://docs.rs/arora-behavior).
