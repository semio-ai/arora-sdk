# 1. The static core: HAL, data layer, communication bridge

> Sub-study of [README.md](README.md). Question: *the HAL controller, the data
> layer and the communication bridge feel like they should be static — would I
> gain something using Copper there instead of linking these components
> directly?*

## TL;DR

**No meaningful gain, and one real conflict.** Copper does not provide a HAL, a
data store, or a Studio bridge. What it actually replaces is the *driver loop*
(`arora-sdk::Instance`, the old `studio-bridge/engine`) — and it replaces it with
a model (typed per-edge messages, no shared state) that **fights** the
shared-`DataStore` blackboard the split proposals are built on.

## What "static" already means in the plan

The split already makes these components static in the only sense that matters:
they are ordinary Rust crates you link directly. From
[`../proposal-bring-studio-bridge-in.md`](../proposal-bring-studio-bridge-in.md):

- the HAL is `arora-sdk::Hal`, an `#[async_trait]` with
  `read`/`write`/`updates` keyed by `Key`;
- the data layer is `arora-types::DataStore` (impl `arora-ecbs`), a shared
  in-process blackboard that *HAL, bridge, BT, and the engine all read and write*;
- the bridge is a `studio-bridge::BridgeConnector` that mirrors the `DataStore`
  to Semio Studio.

So the baseline is already "link statically with these components, directly."
The question is whether *Copper* improves on that. To answer it you have to be
precise about what Copper is.

## What Copper actually is (and isn't)

A Copper app is a compile-time DAG. The `#[copper_runtime(config="…ron")]` macro
reads a RON graph **at build time** and generates the loop. Verified — see
[`sandbox/src/minimal_pipeline.rs`](sandbox/src/minimal_pipeline.rs) and
[`sandbox/copperconfig.ron`](sandbox/copperconfig.ron); the macro even refuses to
compile without a `build.rs` that exports `LOG_INDEX_DIR`
([`sandbox/build.rs`](sandbox/build.rs)).

Its building blocks are three task traits (`cu29-runtime/src/cutask.rs`):

```rust
pub trait CuSrcTask: Freezable + Reflect { type Output<'m>: CuMsgPayload; … }
pub trait CuTask:    Freezable + Reflect { type Input<'m>: CuMsgPack; type Output<'m>: CuMsgPayload; … }
pub trait CuSinkTask:Freezable + Reflect { type Input<'m>: CuMsgPack; … }
```

Two facts follow that decide this whole section:

1. **Data flows along typed edges, not through a shared store.** Each connection
   in the RON carries one concrete message type; each task owns its own state.
   There is no global key-value snapshot. In the sandbox, an `i32` travels
   `src → dbl → sink`; nothing can ask "what is the current value of joint1?"
   the way `DataStore::read(&[Key])` does. Copper ships **no** blackboard type.
2. **`process()` is synchronous and must be short.** It takes `&CuContext`, runs
   in topological order on the time-critical path. Async/blocking I/O is pushed
   out to bridges or `cuasynctask`. The proposed `Hal` is `async` with a
   `Stream` of updates — the opposite shape.

## Component by component

### HAL controller

In Copper, hardware is modelled as `CuSrcTask` (sensors) and `CuSinkTask`
(actuators). That is a reasonable shape — but it is *exactly as much code as
writing `Hal` impls*, with no abstraction handed to you. You would still write
one source/sink per robot (today's `ros2-robots`, `restful-api-robots`, `nao`).

Worse, the mapping is lossy:

- The `Hal` trait is `async` and exposes a single `read`/`write`/`updates`
  surface over arbitrary `Key`s. Copper wants **statically typed, per-signal
  edges** decided at compile time. A robot whose joint set is discovered at
  runtime (Arora explicitly supports dynamic keys) cannot be a fixed set of
  Copper edges.
- ROS 2 today is the `ros2_client` crate (direct DDS). Copper's ROS 2 path is
  `cu-ros2-bridge` over **Zenoh** (doc [04](04-communication-plugins.md)) — a
  different transport you would adopt wholesale.

**Gain from Copper here: none.** You trade an async, key-flexible trait for a
sync, statically-wired one and still write the same per-robot code.

### Data layer

This is the sharp conflict. The entire point of `arora-ecbs` /
`arora-types::DataStore` is a **shared blackboard** that decouples producers and
consumers: the HAL writes sensors, the BT reads `sensors/lidar/min_dist` and
writes intent, the bridge mirrors everything to Studio
([`../proposal-bring-studio-bridge-in.md`](../proposal-bring-studio-bridge-in.md) §4).

Copper has no such thing and is philosophically against it: shared mutable state
defeats its deterministic, zero-copy edge model. To get a blackboard you would
implement one yourself (as a Copper *resource* or a hub task) — i.e. you keep
`arora-ecbs` and Copper contributes nothing to the data layer, while adding the
friction of routing every read/write either through edges or around the runtime.

**Gain from Copper here: negative.** It does not provide the store and resists
the pattern.

### Communication bridge

`studio-bridge` is an async connector: it mirrors a local store to Studio over
Firestore + Zenoh, with snapshot-on-connect and liveliness tokens (the
`sem-136`/`sem-137` work on the bridge today). Copper's bridges move *typed
edge payloads*, not "mirror a key-value store with snapshots." Re-expressing the
bridge as Copper channels would be a rewrite with no functional gain; details in
[04](04-communication-plugins.md).

## But doesn't Copper bring replay and sub-µs latency?

Yes — and neither helps *this* layer:

- **Deterministic replay** (freeze/thaw + unified log) is most valuable for
  self-contained compute pipelines. The static core is dominated by **external
  I/O** (Firestore, Zenoh, WebSocket, hardware) which is inherently
  non-deterministic and lives *outside* any Copper boundary. You would replay
  the part that least needs it.
- **Sub-µs scheduling.** Measured: a full 3-task Copper iteration is ~410 ns
  ([`sandbox/src/throughput.rs`](sandbox/src/throughput.rs),
  `copper_iter_ns=409.8`). But the HAL/bridge operate at hardware and network
  rates — **milliseconds**. Shaving nanoseconds off a loop that waits on a
  Firestore round-trip is pointless.

## What Copper would actually replace

Just the **driver loop**. `arora-sdk::Instance` (and the old
`studio-bridge/engine`'s `tokio::select!`) is the thing whose job Copper's
generated runtime does. So the honest framing is not "Copper improves the HAL /
store / bridge" — it is "Copper is an alternative *Instance loop* that happens to
require restructuring the HAL, store and bridge to fit its model." That is a lot
of churn to replace a loop that is ~500 lines and already works.

## Verdict for the static core

Keep linking `arora-sdk` + `arora-ecbs` + `studio-bridge` directly. Copper
neither supplies these components nor accelerates the layer they live in, and its
no-shared-state model is at odds with the `DataStore` design that ties the core
together. The one transferable idea — *describe the wiring as data* — Arora
already has in `module.yaml` and behavior-tree records.
