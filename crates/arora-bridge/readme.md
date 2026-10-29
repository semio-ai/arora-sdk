# arora-bridge

The bridge interface of [Arora](https://github.com/semio-ai/arora-sdk): how a
running device is reached from outside it. A bridge carries the Arora API — read
and write keys, call functions, list what the device has — between the device
and a remote, over whatever transport the bridge speaks.

## The bridge

A `Bridge` is one **endpoint**: a device's connection to one remote, owned by
that device's runtime. Its data plane is two methods:

- `take_inbound` hands over, once, the stream of what the remote sends —
  `Inbound` events: a `BridgeCommand`, a device-info update, or
  `DataRequested`, whether anyone over the endpoint wants the device's data;
- `try_send` pushes the device's changed state toward the remote, without
  blocking.

The runtime ([`arora`](https://docs.rs/arora)) polls the inbound stream from
its own loop, applies the commands at its next step, and sends each step's
changes to every endpoint whose remote asked for data. The stream ending means
the endpoint disconnected. A bridge owns whatever task its transport needs;
the trait depends on no async runtime.

A bridge relays the device and keeps nothing of its own: the keys and what
they are come from the device's store, the functions from its modules, both
asked when a client asks. Which keys a client receives is the client's choice,
by subscribing where the transport offers it.

## Commands

A `BridgeCommand` is a `BridgeOp` and a one-shot reply, answered with a
`CallResult` (`arora_types::call::CallResult`) or an error message:

| `BridgeOp` | What the device does | Its answer |
|---|---|---|
| `Get(keys)` | Reads the keys. | An array of optional values, in request order. |
| `Update(change)` | Applies a state change, all or nothing. | Unit, or an error naming the refused keys. |
| `Call(call)` | Calls a module function. | The function's return value. |
| `ListKeys { prefix }` | Lists its keys, each with its `KeyMeta`. | `Vec<(String, KeyMeta)>` on the value plane. |
| `DescribeMethods { prefix }` | Describes its functions. | `Vec<MethodSignature>` on the value plane. |
| `ListMethods { prefix }` | Lists its function names only. Deprecated for `DescribeMethods`. | An array of strings. |

Decode a value-plane answer with `arora_types::value_serde::from_value`.

`Update` is where every remote write is checked, once, for every bridge: a
key is closed to remote writers unless the store's meta for it is `editable`,
and a key whose meta states a type takes only values of that type. A change
holding any refused key writes nothing. A device opens its inputs per key or
per subtree in its store; a HAL opens the ones it describes.

A `MethodSignature` is one function's module id, function id, name and frozen
signature (each parameter's name, type and order, and the return type), enough
for a remote to build a typed call, or a typed ROS 2 service as
[`arora-bridge-ros2`](https://docs.rs/arora-bridge-ros2) does. A type is a
primitive or a versioned reference that the remote resolves in its own type
registry.

## Smallest use

`FakeBridge` is the bridge of a device with no remote: its inbound stream never
yields and it drops what it is sent.

```rust
use arora_bridge::{Bridge, FakeBridge};

let bridge: Box<dyn Bridge> = Box::new(FakeBridge::new());
// arora::Arora::builder().with_bridge(bridge)
```

A device takes any number of bridges, one `with_bridge` each, and serves them
all from the same store.

## The client side

`Caller` carries a `Call` to a device and resolves on its reply, whether the
device is in the same process (`arora::LocalCaller`) or across a transport.
The `client` module holds the conventions every client speaks over these ops,
so a remote over a bridge and an in-process caller behave alike:

- **Calling by name.** `find_method` picks the signature a method name
  designates — failing, naming the modules, when several modules export that
  name and none is named — and `call_of` binds arguments given by parameter
  name onto it, failing on a name the signature does not declare.
- **Runs.** A function that returns the behavior `Status` is `task_shaped`: the
  device hosts a call to it as a long-running, cancellable task run. `spawn`
  wraps the call so it starts a run, `run_of` reads the `Run` it answers with —
  the run's id, the key reporting its status, and the keys carrying its
  feedback, result and updates — and `halt` stops it by its id.
- **What a client reads.** `KeyInfo` is a key's path and its `KeyMeta`,
  serialized under the name `__meta`; `MethodInfo` (built by `method_info`) is a
  method as a by-name client sees it: its parameters, return type, and whether
  it starts a run.

## Control plane

Beside the data plane, a bridge answers `get_device_info` and
`update_device_info` (`DeviceInfo`: name, description, model family and versions, owners) and may
offer `device_id`, the device's identity on its remote. A bridge whose remote
asks permission (a client joining a session) yields `AccessRequest`s from
`access_requests`, each answered with an `AccessDecision`; by default a bridge
yields none.

## Implementations

- [`arora-bridge-ws`](https://docs.rs/arora-bridge-ws): the open local bridge,
  a WebSocket server for apps on the same machine — the device's default.
- [`arora-bridge-ros2`](https://docs.rs/arora-bridge-ros2): a ROS 2 graph as
  the remote.
- The Semio Studio connector, which `arora` includes only in a build with its
  opt-in `studio-bridge` feature.
- `FakeBridge`, here.

Part of the device runtime interfaces, with
[`arora-hal`](https://docs.rs/arora-hal) and
[`arora-behavior`](https://docs.rs/arora-behavior).
