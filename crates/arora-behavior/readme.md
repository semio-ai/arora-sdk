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

## Task runs

A task run is a behavior the interpreter hosts beside the main one, followed
and stopped through the `TaskHandle` it answers with. `spawn` starts one from
a module `Call`; `spawn_graph` starts one whose program is a `Graph`, ticked
every step until it ends or is halted, its nodes reachable by `apply` like the
main behavior's. A graph travels with its `GraphType`: the language it is
written in and the version of that language's format, which the interpreter
checks against what it reads. `halt` stops either. Each is a function of the interpreter
module (`interpreter_module`), so a remote reaches them with an ordinary
`Call`.

## Methods an interpreter implements

An interpreter that hosts task runs (`spawn`, `halt`) may implement some
methods itself: a skill whose run is a node-graph fragment, say, rather than a
module call. `described_methods` lists them by function id, each with its name
and frozen signature. The runtime puts them in the device's method index under
the interpreter module (`interpreter_module::ID`), so a remote discovers one
over method introspection and spawns it through that module like any task
run.

- A direct call to such a method fails, saying to spawn it: a task run has no
  single call to answer.
- A function id that a module describes too fails the build: a method has one
  implementation.

## Built-in keys: timing is data, not an argument

The runtime keeps time out of the `tick` signature. Before it ticks any
behavior, it publishes the frame's clock into the shared store under two
reserved **built-in keys** a behavior can rely on:

| Key | Value | Meaning |
|---|---|---|
| `arora/time` | `U64` | monotonic **nanoseconds** on the runtime's timeline, from the time it starts at (zero unless its builder sets one) |
| `arora/dt` | `U64` | **nanoseconds** elapsed since the previous step |

A behavior that paces itself — an animation module, a graph time node — reads
them from `ctx.store` like any other slot, so timing composes as ordinary data
rather than a special tick parameter. They live under the reserved `arora/`
namespace (`built_in::is_built_in`) and stay local to the device: the runtime never
forwards them out over the bridge. The names and the predicate are in the
[`built_in`](src/built_in.rs) module.

Part of the device runtime interfaces, with
[`arora-hal`](https://docs.rs/arora-hal) and
[`arora-bridge`](https://docs.rs/arora-bridge).
