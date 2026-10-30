# arora-behavior

The contract between the [Arora](https://github.com/semio-ai/arora-sdk)
runtime and whatever runs a device's behavior: the `BehaviorInterpreter`
trait the runtime ticks each step, the graph model behaviors are written in,
and the conventions through which clients reach the interpreter — its module,
task runs, and the built-in clock keys.

## Behavior and interpreter

Two things share the word "behavior":

- A **behavior** is data: what a device should do, authored in an editor
  (Semio Studio, the Vizij Workspace) as a `Graph` — nodes bound to
  functions, each holding what its ports are bound to: values, store keys,
  other nodes' ports.
- A **behavior interpreter** (`BehaviorInterpreter`) runs one. A device holds
  exactly one and ticks it once per step; loading a behavior into it, or
  editing the one it holds, changes what the device does.
  [`arora-behavior-tree`](https://docs.rs/arora-behavior-tree)'s
  `BehaviorTreeInterpreter` is one interpreter, Vizij's node graph another.

Implement the trait to add a new *kind* of behavior, a language the runtime
can run. To make a device do something, author a behavior and load it into an
existing interpreter.

## The trait

```rust,ignore
use arora_behavior::{BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus};

struct Idle;

impl BehaviorInterpreter for Idle {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
        // ctx.store: the device's data store — read inputs, write outputs
        // ctx.call_bridge: call module functions
        // ctx.functions: the device's method index, every callable function
        Ok(BehaviorStatus::Running)
    }
}
```

`tick` is the one required method. `Ok(Running)` asks to be ticked again;
`Ok(Done(Ok(())))` and `Ok(Done(Err(_)))` end the interpreter, which the
runtime then drops; an `Err` is transient: the runtime raises it and ticks the
interpreter again next step. The other methods default to refusing: `load`
replaces the behavior with a `Graph`, `apply` edits it with a `GraphDiff`,
`read` answers it with its revision,
`spawn`, `spawn_graph` and `halt` host task runs, `trace` reports what the
ticks do, and `described_methods` lists the methods the interpreter
implements itself. The trait is not `Send`: a
device is stepped on one thread.

Time is not a tick argument: the runtime writes the step's clock to the store
first ([built-in keys](#built-in-keys)).

## Reaching the interpreter

The runtime registers its interpreter as a module under
`interpreter_module::ID`, whose functions are the interpreter's entry points:
`LOAD`, `EDIT`, `READ`, `SPAWN`, `SPAWN_GRAPH`, `TRACE` and `HALT`. A remote client, an
in-process caller or a behavior loads a behavior with an ordinary call:

```rust,ignore
let load = arora_behavior::interpreter_module::encode_load(&graph);
device.call(load)?; // an arora::Arora; a bridge's Call reaches it the same way
```

`interpreter_module` holds every id and an `encode_*` / `decode_*` pair per
function.

## Reading the behavior

`read` answers the main behavior as a `LoadedBehavior`: the `Graph` that a
`load` of it reproduces — none of the interpreter's own structure and none of
a task run's nodes, rooted as loaded or as the last edit set it,
`Graph::empty()` when nothing is loaded — and its `revision`. A client reaches
it through `READ`, a call with no argument (`interpreter_module::encode_read`);
`decode_read_result` reads the answer, a key-value of a `u64` `revision` and
the `graph` in the form `LOAD` takes.

The revision starts at 0 and counts the changes of the main behavior: one more
at each `load`, and at each other change of the graph `read` answers — an
`apply` that changes it, or a task run ending that takes with it a binding of
the main behavior reading it. A refused edit, an edit of a task run alone, a
spawn, a halt that unbinds nothing of the main behavior, and a tick leave it
as it is. The
interpreter writes it to `arora/behavior/revision` (`built_in::BEHAVIOR_REVISION`)
at its first tick and at the tick after each change, and its traces carry it
in the report after a change. A behavior editor that attaches to a device
reads the behavior, follows the key or a trace, and reads again when the
revision is not the one it read: another client changed the behavior.

## Task runs

A **task run** is a behavior the interpreter hosts beside the main one,
started without blocking and followed through the `TaskHandle` it answers
with: the run's `TaskId`, the call that stops it, and its keys — `status`,
`feedback`, `result`, and `update` for steering a live run. The status key
holds a `Status`: `Running`, then `Success` or `Failure` (a halted run ends
`Failure`).

`spawn` starts a run from a module `Call`. `spawn_graph` starts one whose
program is a `Graph`, ticked every step until it ends or is halted, its nodes
reachable by `apply` like the main behavior's. A graph travels with its
`GraphType`: the language it is written in and the version of that language's
format, which the interpreter checks with `GraphType::reads`. A graph run is
known by its graph's root: spawning a graph whose root a live run has answers
that run's handle, the run then running the graph, so a client that lost a
handle takes its run over by spawning its graph again. `halt` stops either
kind. Runs coexist under `RunPolicy::Concurrent`: overlapping writes to one
key are last-write-wins.

## Traces

A **trace** is a task run that reports what the behavior does: `trace`
starts one under `TraceOptions` (the nodes whose values to report, the least
time between reports) and answers its `TaskHandle`. At the end of each tick
that changed something, its feedback key holds a `TraceReport`: the tick, the
nodes whose status changed, the detached nodes whose activity changed, and
the watched nodes' outputs that changed, and the behavior's revision after a
change. `halt` ends it. A behavior editor
starts one when playback starts and halts it when playback stops;
[`docs/trace.md`](docs/trace.md) describes the report and its cost.

## Methods an interpreter implements

An interpreter that hosts task runs may implement some methods itself — a
skill whose run is a graph fragment rather than a module call.
`described_methods` lists them by function id, each with its name and frozen
signature. The runtime puts them in the device's method index under the
interpreter module, so a remote discovers one through method introspection
and spawns it like any task run.

- A direct call to such a method fails, saying to spawn it: a task run has no
  single call to answer.
- A function id that a module describes too fails the build: a method has one
  implementation.

## Built-in keys

Before it ticks the interpreter, the runtime writes the step's clock to the
store under two keys, and an interpreter that answers `read` keeps the
behavior's revision under a third:

| key | value | meaning |
|---|---|---|
| `arora/time` | `U64` | nanoseconds on the device's timeline, from the time it starts at (zero unless its builder sets one) |
| `arora/dt` | `U64` | nanoseconds elapsed since the previous step |
| `arora/behavior/revision` | `U64` | the main behavior's revision, written once per change ([reading the behavior](#reading-the-behavior)) |

A behavior that paces itself reads them like any other key. They live under
the reserved `arora/` namespace (`built_in::PREFIX`, `built_in::is_built_in`),
which belongs to the runtime and its interpreter — the behavior tree keeps its
task runs' keys there too — so a behavior writes no key of its own under it.
The keys travel outbound with every other change: a remote derives the
device's step rate from the clock, or filters the keys out.

## Reference

| module | contents |
|---|---|
| crate root | `BehaviorInterpreter`, `BehaviorContext`, `BehaviorStatus`, `BehaviorError`, `LoadedBehavior`, `FunctionIndex`, `ModuleFunction` |
| [`graph`](src/graph.rs) | `Graph`, `Node`, `Source`, `Port`, `GraphDiff`, `Binding`, `GraphType`: references by id, the form an interpreter takes ([which form to use](../arora-behavior-tree/docs/text.md#which-form-to-use)) |
| [`interpreter_module`](src/interpreter_module.rs) | the interpreter module's ids and its calls' encoders and decoders |
| [`task`](src/task.rs) | `TaskHandle`, `TaskId`, `RunPolicy` |
| [`trace`](src/trace.rs) | `TraceOptions`, `TraceReport`, `NodeStatus`, `Activity`, the report's encoder and decoder |
| [`status`](src/status.rs) | `Status`, its record's ids and version, `declare_status_enumeration` |
| [`built_in`](src/built_in.rs) | `PREFIX`, `TIME`, `DT`, `BEHAVIOR_REVISION`, `is_built_in` |

[`docs/interpreter-workflow.md`](docs/interpreter-workflow.md) walks an
interpreter's life — loading, ticking, editing, task runs — against the
runtime's step. The step itself is in
[`arora`'s runtime page](../arora/docs/runtime-and-data-flow.md).

Part of the device runtime interfaces, with
[`arora-hal`](https://docs.rs/arora-hal) and
[`arora-bridge`](https://docs.rs/arora-bridge).
