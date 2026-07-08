# arora-behavior

The behavior abstraction of [Arora](https://github.com/semio-ai/arora-sdk):
anything the runtime can tick.

A `Behavior` advances one step at a time — `tick(&mut BehaviorContext)` — and
reports whether it is `Running` or `Done`. The context hands it the shared
data store (read inputs, write intent) and the module-call bridge, so a
behavior can be a behavior tree
([`arora-behavior-tree`](https://docs.rs/arora-behavior-tree)), a
node graph, or any other interpreter: the runtime queues `Box<dyn Behavior>`
and ticks them without knowing which is which.

## Golden keys: timing is data, not an argument

The runtime keeps time out of the `tick` signature. Before it ticks any
behavior, it publishes the frame's clock into the shared store under two
reserved **golden keys** a behavior can rely on:

| Key | Value | Meaning |
|---|---|---|
| `arora/time` | `U64` | monotonic **nanoseconds** since the runtime started |
| `arora/dt` | `U64` | **nanoseconds** elapsed since the previous step |

A behavior that paces itself — an animation module, a graph time node — reads
them from `ctx.store` like any other slot, so timing composes as ordinary data
rather than a special tick parameter. They live under the reserved `arora/`
namespace (`golden::is_golden`) and stay local to the device: the runtime never
forwards them out over the bridge. The names and the predicate are in the
[`golden`](src/golden.rs) module.

Part of the device runtime interfaces, with
[`arora-hal`](https://docs.rs/arora-hal) and
[`arora-bridge`](https://docs.rs/arora-bridge).
