# Changelog

All notable changes to `arora-behavior`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [9.2.0] - 2026-10-10

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
