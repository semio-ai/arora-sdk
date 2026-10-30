# Changelog

All notable changes to `arora-behavior`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [10.1.0] - 2026-10-30

### Added

- `BehaviorInterpreter::read`, answering the main behavior as a
  `LoadedBehavior { revision, graph }`: the graph a `load` of it reproduces
  (no node of the interpreter's own, none of a task run's, the root as
  loaded), and its revision, which each `load` and each other change of that
  graph advances by one. It defaults to refusing.
- `interpreter_module::READ`, its call with no argument, and
  `encode_read`, `decode_read`, `encode_read_result` and
  `decode_read_result`. The answer is a key-value of a `u64` `revision` and
  the `graph` in the form `LOAD` takes.
- `built_in::BEHAVIOR_REVISION` (`arora/behavior/revision`): the revision an
  interpreter that answers `read` writes once per change, under the
  runtime's closed `arora/` namespace.
- `TraceReport::revision`, and the report's `revision` field on the wire:
  the behavior's revision when it changed since the trace's previous report,
  and in its first. A struct literal of `TraceReport` names it, or ends with
  `..TraceReport::default()`.

## [10.0.0] - 2026-10-30

### Changed

- **Breaking:** the graph model is leaner: a node holds its own bindings, and
  a binding names its store key.
  - `Node { id, function, bind, children }`: `bind` maps each bound port to
    its `Source`. An input's binding is what feeds it; an output's binding is
    where it is written, a return value written to a key included. Which
    ports are inputs and which outputs, and their types, is the function
    signature's to say. `Io` (with `ty` and `predetermined_key`) is gone: a
    predetermined key is an output bound to a key.
  - `Source` replaces `LinkSource`: `Value(Value)`, `Key(String)` — the store
    key by its path, kept by the interpreter when it starts with `.` —
    `Port(Port)` and `Select { source, path }`. `source_port` takes a
    `Source`.
  - `Graph { root, nodes }`: `nodes` is a list with unique ids, serialized in
    its order. `Graph::links`, `Graph::variables` and `Graph::link_to` are
    gone, and so is `Link`.
  - `GraphDiff { remove, set, unbind, bind, root }`, applied in that order:
    nodes removed (a binding reading one goes with it, and so does the root),
    nodes set whole (in their place, or after the others), single ports
    unbound and bound (`Binding { node, port, source }`), the root set. A
    diff binding a port on no node is refused and changes nothing.
    `GraphDiff::load` is gone: loading a graph is applying
    `Graph::empty().diff(&graph)`.
  - A graph and a diff serialize leanly: an empty field is left out, and a
    `Source` is a single-entry map naming its kind (`{"key": "face/x"}`,
    `{"value": {"f64": 1.0}}`) in every format, so a graph with any value
    bound reads back from YAML. A graph, a node or a diff holding a field it
    does not have is refused, so a payload of arora-behavior 9's shape fails
    to decode rather than reading as an empty edit.
- `interpreter_module`'s `LOAD`, `EDIT` and `SPAWN_GRAPH` carry the new
  forms.

### Added

- `BehaviorInterpreter::trace(TraceOptions)`: start a trace, a task run
  reporting what the behavior does. It answers a `TaskHandle` laid out as a
  spawn's, and `halt` ends it. The default refuses. The `trace` module holds
  `TraceOptions` (the nodes whose values to report, the least time between
  reports), `TraceReport` (the tick and the tick of the previous report, the
  statuses, detached activities and watched values that changed since then,
  as lists, and the nodes an edit restarted), `PortValue`, `NodeStatus` and
  `Activity` with their byte codes, and `encode_report` / `decode_report`:
  the report travels as a key-value of flat arrays, a status or an activity
  a byte, a watched value itself. [`docs/trace.md`](docs/trace.md) describes the report.
- `interpreter_module::TRACE` and `TRACE_ARG`, with `encode_trace` and
  `decode_trace`; `decode_spawn_result` reads the handle `TRACE` answers.
- `Graph::diff(&self, target)`: the minimal diff taking a graph to `target`.
  A node whose function or children change is set whole; a node whose
  bindings alone change gets one entry per binding that changes. Applied, it
  gives `target`'s root and nodes, the graph's own nodes in their order and
  the others after them.

## [9.6.0] - 2026-10-19

### Changed

- `BehaviorInterpreter::spawn_graph`'s contract: a graph run is known by its
  graph's root. Spawning a graph whose root a live run has answers that run's
  handle, the run taking the graph as its program and keeping its id, status
  key and policy. An interpreter that overrides `spawn_graph` answers so; the
  behavior tree's does from arora-behavior-tree 9.1.
  `interpreter_module::SPAWN_GRAPH` and the readme say the same.

## [9.5.0] - 2026-10-18

### Changed

- `Status`'s record (`AroraType::record`, arora-types 3.8) lives in the
  `arora` scope (`arora_types::record::scope::ARORA`, path `arora.Status`)
  whatever names it, so modules of any scope sharing `Status` ship one
  record. Requires arora-types 3.8.

## [9.4.0] - 2026-10-16

### Added

- `ModuleFunction` (moved from `arora-behavior-tree`, which re-exports it)
  and `FunctionIndex`, the device's module functions by function id.
- `BehaviorContext::functions`: the device's `FunctionIndex`, handed to the
  interpreter at each tick. The device owns the index and changes it between
  steps when its modules change; an interpreter reads it when it resolves
  what its behavior calls. A `BehaviorContext` built by struct literal names
  the new field; the device runtime is what builds it.

## [9.3.0] - 2026-10-13

### Changed

- `interpreter_module::encode_halt` sends the task id as `Value::Uuid`, the
  type `halt`'s `task` parameter is described with. `decode_halt` also reads
  the uuid's string form, which clients built on earlier versions send.

## [9.2.0] - 2026-10-13

### Added

- `GraphType`: the language a `Graph` is written in and the version of that
  language's format, with `GraphType::reads`, which tells whether an
  interpreter reading one version runs a graph written for another (same name
  and major, no newer minor).
- `BehaviorInterpreter::spawn_graph(graph_type, graph, policy)`: start a
  `Graph` as a task run, returning its `TaskHandle`. The graph runs beside
  the main behavior until it ends or is halted, and `apply` reaches its
  nodes. An interpreter refuses a graph type it does not read. The default
  rejects it.
- `interpreter_module::SPAWN_GRAPH` and its argument ids
  (`SPAWN_GRAPH_TYPE_ARG`, `SPAWN_GRAPH_VERSION_ARG`, `SPAWN_GRAPH_GRAPH_ARG`,
  `SPAWN_GRAPH_POLICY_ARG`), with `encode_spawn_graph` and
  `decode_spawn_graph`. The type and version travel as strings beside the
  graph; the call answers with the handle, read with `decode_spawn_result`.

## [9.1.1] - 2026-10-09

### Fixed

- `Graph::apply` costs what the diff costs, not what the graph does. It
  scanned every link of the graph once per link it added and once per node
  it removed, so grafting a hundred nodes into a graph of a few thousand
  links cost about 25 million instructions; it now makes one pass for the
  whole diff — about ten times less on that graft — with the same result,
  link order included.

## [9.1.0] - 2026-09-28

### Added

- `BehaviorInterpreter::described_methods`: the methods an interpreter
  implements itself — task runs it hosts as behavior rather than as a module
  call — by function id, each with its name and frozen signature. The runtime
  lists them in the method index under the interpreter module. The default
  describes none.

## [9.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3.

## [8.5.0] - 2026-09-22

### Added

- `From<TaskId> for Value` / `TryFrom<Value> for TaskId` (a `Value::Uuid`).

### Changed

- `Status`'s value-plane conversions come from `#[derive(AroraType)]`
  (arora-types 2.6) instead of hand-written impls; same wire form, same ids.

## [8.4.0] - 2026-07-30

### Added

- `Status` — the cross-interpreter run-status enum (`Success`/`Failure`/
  `Running`) — now lives here (ARORA-82), beside `BehaviorStatus`/`TaskHandle`,
  defined via `#[derive(AroraType)]` with pinned ids plus the value-plane
  conversions and `declare_status_enumeration`. It moved out of
  `arora-behavior-tree-types` (which re-exports it for compatibility): the node
  graph and the behavior tree both speak `Status`, so neither interpreter's
  crate should own it. Wire form (ids, `Value::Enumeration`) is unchanged.

## [7.0.0] - 2026-07-24

### Breaking

- `LinkSource` gains a **`Select { source, path }`** variant (ARORA-72): reading
  a `Key` sub-path of another source's value is now a composable *operation over
  any source* — `Select { source: Variable(id), .. }` selects into a variable,
  nested `Select`s chain — rather than a field on one source kind. `Port(Port)`
  is unchanged; the break is the added enum variant. The `path` segments are
  resolved **ids** (a `Value::Structure` field id, or an array index), so an
  interpreter reads a sub-path with no registry at runtime — names are resolved
  to ids when a graph is built. Heavier "compute a value" operations (calls,
  arithmetic) are nodes, not link sources.

- Re-pinned to `arora-types` 2.1 for `Key::select`, which a `Select` source's
  value is projected through at read time (selection lives with `Key`/`Value`).

### Added

- `source_port(&LinkSource)`: the `Port` a source ultimately reads from, through
  any `Select` wrappers.

## [6.0.0] - 2026-07-23

### Breaking

- Renamed the "golden" concept to "built-in" (ARORA-59): the `golden` module is
  now `built_in`, and `golden::is_golden` is now `built_in::is_built_in`. The
  reserved keys and their wire names (`arora/time`, `arora/dt`, the `arora/`
  prefix) are unchanged — only the Rust identifiers and prose changed.

## [5.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 (its types are part of this API).

### Changed

- The golden keys (`arora/time`, `arora/dt`) travel outbound with every other
  change; a remote that does not want them filters them on its side.

## [4.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [4.0.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

## [3.1.0] - 2026-07-10

### Breaking

- The golden behavior edit — a Call reaches interpreter.apply through the engine (PR 5b)
- Call_bridge, and edition that defers lowering to the tick
- A slot's direction is which Node list holds it
- Lower Groot import onto the shared Graph; arora glue

## [3.0.0] - 2026-07-09

### Breaking

- Shared graph model + GraphDiff + BehaviorInterpreter::apply

## [2.0.0] - 2026-07-08

### Breaking

- Rename Behavior trait to BehaviorInterpreter

## [1.0.0] - 2026-07-08

### Breaking

- Golden clock keys (time/dt) in the store; drop ctx.dt

### Changed

- Link crate pages via docs.rs
- The device story — readme leads with what Arora runs
- Add description + MIT license to leaf crates for crates.io

## [0.1.0] - 2026-06-29

### Added

- A Behavior the runtime ticks, in an arora-behavior crate (VIZ-33)
