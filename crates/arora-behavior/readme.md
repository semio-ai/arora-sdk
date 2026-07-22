# arora-behavior

The behavior *interpreters* of [Arora](https://github.com/semio-ai/arora-sdk):
the executors the runtime ticks each step.

Mind the two meanings of "behavior":

- A **behavior** (the noun) is an *authored, editable representation* of what a
  device should do — a behavior tree, a node graph — produced in a visual editor
  (Studio, the Vizij Workspace) and shipped as data.
- A `BehaviorInterpreter` is the *runtime-level executor* that runs one of those.
  It is the thing the runtime actually ticks.

A `BehaviorInterpreter` advances one step at a time —
`tick(&mut BehaviorContext)` — and reports whether it is `Running` or `Done`.
The context hands it the shared data store (read inputs, write intent) and the
module-call bridge. The behavior tree is one interpreter
([`arora-behavior-tree`](https://docs.rs/arora-behavior-tree)'s
`BehaviorTreeInterpreter`); a node graph is another. The runtime queues
`Box<dyn BehaviorInterpreter>` and ticks them without knowing which is which.

Implement `BehaviorInterpreter` to add a new *kind of executor* — a new
authored-behavior representation the runtime can run. Hand-implementing it to
hard-code one particular behavior in Rust is a corner case, not the promoted
path: author a behavior in an editor and let an interpreter run it.

**How it works, with diagrams:** [`docs/interpreter-workflow.md`](docs/interpreter-workflow.md)
walks the interpreter lifecycle — load, time update, ticks, graph updates, and
"keeps ticking" — grounded in the source. See a concrete interpreter in
[`arora-behavior-tree`](../arora-behavior-tree/docs/nodes.md) and the whole
device loop in [`arora`](../arora/docs/runtime-and-data-flow.md).

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
