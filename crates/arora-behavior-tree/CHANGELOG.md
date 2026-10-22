# Changelog

All notable changes to `arora-behavior-tree`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [9.3.0] - 2026-10-22

### Added

- Detached nodes (`detached`): a call to a module function that the tree's
  nodes list but none holds, being neither the root nor a child of a held
  node, and that holds no children, runs when what it outputs is about to be
  read. It is described as any node is, and binds its outputs the same way:
  its return value through `_ret`, each mutable parameter through the cell its
  argument names; its return value is a port whether or not `_ret` is bound.
  - A node reads it through `@node` or `@node.parameter` (a `NodeArgument` on
    its port), or through the variable an output is bound to. An output bound
    to another node's input feeds it in place of its argument, even one
    naming a store key. A port bound to
    a store key reads the node's current result, not the key. Nothing marks
    the read: lowering finds the detached node that writes each tree-local
    cell, and a node brings the detached nodes it reads up to date before it
    ticks.
  - It runs at most once per tick, and calls its function only when its
    argument values changed. A failed evaluation fails every read of it in
    that tick without calling again, and is tried again at the next. Its
    outputs are written return first, then in its function's parameter order,
    so two that share a destination leave the same value at every load. A store key is read as the store holds it.
  - One with an output bound to a store key is a sink: it is brought up to
    date at every tick, after the tree ticks, even when the tree fails; the
    tick then reports the tree's error. A sink that fails stops no other: the
    others publish, and the failing one publishes nothing that tick. Once every sink is up to date,
    each writes its keys, from its cache when its inputs did not change, so a
    sink reads another sink's key as it was before the tick. Of two sinks of
    one key, the later listed wins. A shared `Graph` lists its root, then its
    other nodes by id.
  - Lowering refuses detached nodes that read each other in a cycle, a
    detached node reading an output of its own other than its state, a call
    in a detached node's arguments writing back a store key, an output bound
    to an input that a call or a selection computes, directly or through the
    ports that alias it, a mutable parameter bound
    to another node's input that names a store key, a read of a port that is
    neither a return value nor a mutable parameter, and a tree-local
    cell that a detached node and another node both write. The writes counted
    include nested calls', at any depth, native nodes' such as `Unpack`'s, and
    a call-valued argument's cell; a call whose function or parameter is
    computed counts every cell its arguments name.
  - `BehaviorTree::detached_plan` (`DetachedPlan`: `every_tick`, `on_demand`,
    `unused`), `detached_calls` and `display_detached`.
- A node's argument can nest calls to module functions (`Expression::Call`),
  evaluated innermost first each time the node ticks.
  - A call's signature is resolved from the function index when the tree is
    lowered, and an absent value passes `None` to an optional parameter.
  - A literal argument of a call is the call site's own cell, so a stateful
    function keeps its mutable parameter from tick to tick; a rebuilt tree
    takes over each call site's state and the variables its calls name.
  - Lowering refuses a call to the interpreter module, an argument for a
    parameter the function does not have, and a mutable parameter fed by a
    selection or a call. When a call computes the parameter's id, the tick
    fails if the function writes back such a parameter.
  - A leaf evaluates its arguments in its function's parameter order; the core
    string nodes compute theirs too, and read a selection's value. A call computing a module, function or
    parameter id keeps a call site's state as an argument's call does.
- `display`: a text form of expressions and nodes, with names from the
  function index (`Names`), on one line with `{}` or broken over lines with
  `{:#}`: `level: {face/level} = add(a: @cm, b: 1f64)`. Lowering errors and
  traces use it.
- The interpreter reads `behavior-tree 1.3` (`GRAPH_TYPE_VERSION`), the
  version that runs detached nodes; it still runs `1.0`, `1.1` and `1.2`
  graphs.

### Changed

- A call to a module function that no node holds runs as a detached node; it
  was never ticked. A node of another kind that no node holds, such as a
  control node an edit adds before the edit that places it, or a node with
  children, is still left aside until a node holds it.

### Fixed

- A tree that does not lower, such as one naming a child that does not exist,
  leaves no tick registered in the engine: the ticks registered before the
  refusal are unregistered.
- When the interpreter's edited graph does not lower, the previous lowering
  stays registered with its state, and the edit's fresh nodes stay fresh,
  until a lowering succeeds: undoing the edit resumes the tree where it was.

## [9.2.0] - 2026-10-22

### Added

- `Scatter(values, keys, within)` (`SCATTER_FUNCTION_ID`,
  `SCATTER_VALUES_PARAM_ID`, `SCATTER_KEYS_PARAM_ID`,
  `SCATTER_WITHIN_PARAM_ID`), a natively ticked data node: writes element `i`
  of the array `values` to the store key named by element `i` of the array
  `keys`, and succeeds; a `Unit` element leaves its key as it is. The keys are
  data, so a tree writes a table of any length in one tick, and a table whose
  keys change needs no edit. `within`, a literal key prefix fixed at lowering,
  bounds the keys: lowering refuses a `within` that is not a literal string, is
  empty, ends with `/` or starts with `.`. A tick checks before it writes and
  writes all its keys or none: it fails when `values` or `keys` is not an
  array, a key is not a string, or the lengths differ; it errors when a key is
  outside `within` or named twice. The node keeps the slots its keys resolved
  to until a tick brings other keys. Groot reads and writes it as
  `<Scatter values="…" keys="…" within="…"/>`, and the palette declares it.
- `BehaviorTree::with_key_resolver`: the resolver a tree's `Scatter` nodes
  resolve their keys through while it ticks. The interpreter gives its trees
  the Direct convention's, over its own handle on the store
  (`DataStore::clone_box`); a tree built without one runs, and its `Scatter`
  nodes error when they write.
- The interpreter reads `behavior-tree 1.2` (`GRAPH_TYPE_VERSION`), the
  version that adds `Scatter`; it still runs `1.0` and `1.1` graphs.

### Changed

- The `arora-types` requirement is `3.6`: `Scatter` relies on its
  change-only slots and `prefix_covers`.

## [9.1.0] - 2026-10-19

### Added

- A run is known by its root. `spawn_graph` of a graph whose root a live run
  has starts no second run: that run takes the graph as its program, and the
  spawn answers the run's `TaskHandle`. Only what differs changes: the nodes
  and links that differ, the run's nodes the graph does not have (which
  leave, but for those the main behavior or another run also holds), and the
  variables it declares anew. The run's nodes the graph leaves as they are keep
  their state. A client that lost a run's `TaskId` takes the run over by
  spawning its graph again, and a spawn of the graph a run already runs
  changes nothing. Such a spawn used to be refused, its node "already
  part of the device's behavior". A graph with a live run's root that also
  holds a node of the main behavior or of another run, or the run's
  decorator, is still refused.

### Fixed

- A `halt` keeps the main behavior's root when an edit has placed it under the
  run too. The halt used to fail, the device's behavior left referring to the
  root it removed, and the run no longer tracked.

## [9.0.1] - 2026-10-19

### Fixed

- A change of the interpreter's graph leaves the behaviors it does not touch
  where they stand. The tree lowered from the changed graph takes over the
  state of every node the change left in place: a `seq_star`'s place, a run's
  latch, a tree-local variable such a node reads. A node the change adds,
  replaces, relinks or re-keys starts afresh, and so does a tree-local
  variable only such nodes read: a `load` keeps one a run also reads. A
  `spawn`, `spawn_graph`, `edit`, `halt` or `load`, or a run ending, used to
  restart the main behavior and every run.
- A graph that does not lower leaves the previous lowering in place, unticked,
  so the state carries over to the next lowering that succeeds.

## [9.0.0] - 2026-10-16

### Changed

- **Breaking:** the interpreter holds no function index; the device hands it
  over at each tick through `BehaviorContext::functions`, so a module loaded,
  reloaded or unloaded on a running device is seen without rebuilding the
  interpreter.
  - `BehaviorTreeInterpreter::new()` takes no argument (and the interpreter
    implements `Default`).
  - `BehaviorTreeInterpreter::load_groot(xml, functions)` takes the index its
    tags resolve against.
  - `lower_behavior_tree`, `run_behavior_tree` and
    `BehaviorTreeRuntime::setup` take `&FunctionIndex` instead of
    `Rc<HashMap<Uuid, ModuleFunction>>`.
- A lowered tree resolves each node's module function once, when it is
  lowered, and keeps calling the module it resolved until it is lowered
  again. A node whose function the index does not have fails when it ticks,
  with `CallError::FunctionNotFound`, as before.
- `spawn_graph` refuses a graph naming one of the interpreter module's
  functions by id. It no longer refuses a graph naming a function the index
  does not have: that node fails when it ticks, and the run ends `Failure`.
- A selection feeding a parameter its function declares mutable is refused
  when the graph is lowered (the next tick), not when it is applied: whether
  a parameter is mutable is the index's to say. A selection feeding a return
  binding is still refused when applied.
- `ModuleFunction` moved to `arora-behavior`; `arora_behavior_tree` re-exports
  it, with `FunctionIndex`. It now derives `Debug`, `Clone` and `PartialEq`.
- **Breaking:** the `arora_generated` module is gone. The runtime returns
  `arora_behavior::Status`, and a control node's children are
  `arora_behavior_tree_types::TickId`; the generated twins of those types,
  with their buffer functions and raw id constants, are removed. The crate
  has no build script.

## [8.8.0] - 2026-10-13

### Added

- Tree-local variables: a variable whose name starts with `.` (`{.x}`) is a
  cell of the lowered tree. The lowering never asks the resolver for it, so
  it has no store slot, makes no store change and is in no write set.
  `variable::is_tree_local` states the rule.

### Changed

- A variable named with a leading `.` no longer binds the store key of that
  name.
- Groot export writes a variable without a name as `{.<id>}`, so it imports
  back as one tree-local variable; it was written as `{<id>}`, which imported
  as a store key. Every reference to one such variable, a module leaf's
  included, exports the same name.

## [8.7.0] - 2026-10-13

### Added

- Groot tags for the native data and task-run nodes, read and written with
  their arguments as ports: `Equal` (`a`, `b`), `Unpack` (`values`, and the
  optional output ports `out_0` to `out_63`), `RunCall` (`call`) and
  `RunStatus` (`status`; its latch is internal state, seeded on import). A
  port holds `{name}` for a variable, `json:` followed by a value's serde form
  for a typed literal (BehaviorTree.CPP's convention), or plain text for a
  string. Export writes plain text only where it reads back as the same
  value, and `json:` otherwise. Import refuses a port these nodes do not have
  (an `Unpack` output spelled other than `out_<i>`, `i` below 64, included)
  and a missing required one; export refuses a value JSON cannot hold exactly
  (a non-finite float). The Groot palette declares the four nodes with all
  their ports.

### Changed

- The tags `Equal`, `Unpack`, `RunCall` and `RunStatus` name the native
  nodes: a module export named `equal`, `unpack`, `run_call` or `run_status`
  is reachable from Groot under that exact snake_case name only.

## [8.6.0] - 2026-10-13

### Fixed

- A module leaf's argument linked through a selection (`LinkSource::Select`)
  receives the selected part of its source, as a native node's argument and a
  call expression's argument already do, rather than the whole source value.

### Changed

- A selection is read-only: the interpreter refuses an edit, load or spawn
  that links one to a node's return binding (`_ret`) or to a parameter its
  function declares mutable.
- A selection over an absent value reads as absent (`Unit`), as the absent
  value does; a path that a present value does not have errors the tick.
- `Unpack` relies on the store's change-only notification
  (arora-types 3.6): a value that does not move makes no store change.

## [8.5.0] - 2026-10-13

### Changed

- A tree does not call the interpreter module's own functions (`load`,
  `edit`, `spawn`, `spawn_graph`, `halt`), which act on the interpreter while
  it ticks the tree: a Groot tag does not resolve to them, and a spawned graph
  naming one is refused.

## [8.4.0] - 2026-10-13

### Added

- Two natively ticked data nodes, in `nodes`:
  - `Equal(a, b)` (`EQUAL_FUNCTION_ID`, `EQUAL_A_PARAM_ID`, `EQUAL_B_PARAM_ID`):
    Success when the two values are equal — same type and same value —
    Failure otherwise.
  - `Unpack(values, out_0 … out_63)` (`UNPACK_FUNCTION_ID`,
    `UNPACK_VALUES_PARAM_ID`, `UNPACK_OUT_PARAM_IDS`, `UNPACK_PORTS`,
    `unpack_out_index`): writes element `i` of the array `values` to the
    variable bound to the output port `out_i`, skipping an unbound port and a
    `Unit` element and ignoring elements past the last bound port, and
    succeeds (a value equal to the one a key holds makes no store change);
    fails, writing nothing, when `values` is not an array or is shorter than
    the last bound port needs. Each output is an ordinary port binding to a
    variable, bound when the tree is lowered; lowering refuses an output
    bound to anything else, and two outputs bound to one variable or one name.

  Their inputs are read through any link source, a selection included, and
  must be linked; lowering refuses a tree that breaks either rule.
- The interpreter reads `behavior-tree 1.1` (`GRAPH_TYPE_VERSION`), the
  version that adds `Equal` and `Unpack`; it still runs `1.0` graphs.

### Changed

- An edit is also refused when the edited graph would not lower, so a
  refused output binding or an unlinked argument never stops the device's
  behavior.

## [8.3.0] - 2026-10-13

### Added

- `behavior::GRAPH_TYPE_NAME` (`behavior-tree`), `GRAPH_TYPE_VERSION` (1.0.0)
  and `graph_type()`: the graph type the interpreter reads.
- `BehaviorTreeInterpreter` implements `spawn_graph` for `behavior-tree 1.0`:
  the spawned graph's nodes and links join the scaffold under a run-status
  decorator, whose status key is
  `arora/tasks/<interpreter module>/<spawn_graph>/<run>/status`. The run ticks
  every step beside the main behavior. The spawn is refused when the graph's
  type is not one the interpreter reads, when the graph is not one tree under
  its root, calls a function the device does not have, does not lower,
  reuses a node id the device's behavior holds, has a link reaching outside
  its nodes, or renames a declared variable.

### Changed

- A run is the subtree under its decorator. An edit reaches its nodes by id,
  and a node an edit places under the decorator belongs to the run. The main
  behavior is every node outside the runner and the runs.
- A halt prunes the run's subtree at once, but for the nodes another node
  holds, and drops the variables the run declared that no remaining link
  reads; the run's `Failure` status is written on the next tick. A run that
  ends by itself is pruned on the next tick, or by the next call that changes
  the graph. A run's node ids are therefore free again as soon as it is halted
  or has ended.
- A run whose child errors ends `Failure`: the decorator writes and latches
  it, and the error is the tick's once, so the other runs tick on.
- The runner ticks the main behavior first, then the runs in spawn order.
- An edit applies whole or not at all, and is refused when it would leave a
  child that is not a node or a cycle under the runner, change a run's
  decorator, place the runner or a decorator under another node, or remove a
  run's root. A load is refused when it reuses a run's node id or renames a
  variable a run reads. Removing and re-adding the main behavior's root in one
  edit keeps it the main root.
- An argument that reads another node's argument lowers whatever order the
  nodes are listed in.
- Depends on arora-behavior 9.2.

## [8.2.0] - 2026-10-12

### Added

- **The string nodes are core nodes, native to every tree.** `SetString`,
  `UnsetString`, `IsStringSet`, `WaitStringSet` and `RegexMatch` are
  dispatched natively like the control nodes: a tree using them runs with no
  module loaded (it failed with "function … is missing from index" unless
  the test module `test-behavior-tree-nodes` was). They keep that module's
  ids, parameter ids, names and semantics, and stay value-based: a node
  receives the values its ports are bound to and writes through its
  out-parameters, the tree deciding which keys those are
  ([`core_nodes`](src/core_nodes.rs), [docs](docs/nodes.md#core-string-nodes)).
  A Groot tree names them as before, with an empty function index.
- **An optional parameter bound to an unset key receives `None`.** A module
  function's optional parameter (`Option<T>`) bound to a variable that holds
  nothing — a store key not set yet — is passed `None`, and a present value
  `Some(value)`, instead of `()`, which the function refused ("unexpected
  value ()"). An optional out-parameter, or an optional return bound with
  `_ret`, maps onto its key's presence: `Some(v)` writes `v`, creating the
  key, and `None` unsets it.

### Fixed

- `SetString` (Groot) and the `nodes::set_str` builder named the test
  module's `store` function (`b8349b96-…`, an `f32` store) instead of
  `set_str` (`c803889f-…`). They name `set_str` now.

## [8.1.0] - 2026-09-26

### Added

- A Groot tag outside the built-in palette resolves to the indexed module
  function of that name — as declared, or in PascalCase (`Walk` for `walk`,
  `PlayClip` for `play_clip`). A name several loaded modules export is refused
  as ambiguous rather than resolved to whichever the index yields first. The
  exporter writes such nodes under the function's name, so trees round-trip.
  A palette tag (`Cos`, `Store`, `Status`, …) still names the palette's
  function; a module export of the same name is reachable under its exact
  snake_case spelling.

### Fixed

- A Groot return binding (`res="{cosine}"` on a function that does not return
  a `Status`) lowers to a variable reference, so the graph binds it to the
  named store slot and the return reaches the store. As a literal it bound to
  a local cell nobody read.

## [8.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3, arora-buffers 3, arora-behavior 9,
  arora-behavior-tree-types 2 and arora-registry 2.

## [7.0.0] - 2026-07-29

### Changed

- **The runner scaffold**: the interpreter permanently hosts one graph — a
  parallel *runner* root — and everything it executes is grafted under it as
  tree structure. Loading a behavior grafts it as the runner's main child;
  spawning a task run grafts a two-node fragment (a run-status decorator, its
  status key predetermined, over a run-call leaf carrying the spawned `Call` as
  literal link data); halting prunes it. Runs, policies, and the loaded
  behavior are introspectable through `graph()` as ordinary graph data — not
  hidden interpreter state.
- **Reactive tick semantics**: one arora runtime tick is one behavior-tree
  tick. The lowered tree persists across ticks (rebuilt on graph edits,
  unregistering the previous lowering's engine callables — fixing a callable
  leak); `Running` spans ticks instead of spinning the device; a terminal main
  behavior is re-evaluated next tick (a tree is a continuously re-evaluated
  policy); a terminal run latches until pruned. The interpreter is a standing
  policy: it reports `BehaviorStatus::Running` and is never dropped.
- **Breaking**: `graph()` returns `&Graph` (the scaffold always exists);
  `load_groot` takes only the XML (the tree binds to the store at its first
  tick); the raw `load(BehaviorTree)` and `load_graph(Graph, &dyn DataStore)`
  inherent methods are removed — load through the `BehaviorInterpreter` trait.

### Added

- Native task-run scaffolding nodes: `RUN_CALL_FUNCTION_ID` (a leaf carrying a
  full `Call` as one literal parameter, dispatched through the caller — no
  function-index entry needed) and `RUN_STATUS_FUNCTION_ID` (a decorator
  forwarding its child's status onto a bound status output, latching at
  terminal).
- `LoweredTree`/`lower_behavior_tree`: a persistent lowering with callable
  cleanup; `run_behavior_tree` remains as the drain-to-terminal convenience.

## [6.2.0] - 2026-07-24

### Added

- `Expression::Select { source, path }`, mirroring the shared model's
  `LinkSource::Select` (ARORA-72): a `Select` link lowers to it, and the tree
  applies the `Key` path on read (`compute_expression`/`compute_uuid`). The
  selection shares its source's reactive cell; the path is applied per tick, not
  at bind time.

### Changed

- Re-pinned to `arora-behavior` 7 and `arora-types` 2.1 (the `LinkSource::Select`
  change and `Key::select`, ARORA-72); a `Select` expression projects its
  source's value through `Key::select`.

## [6.1.0] - 2026-07-23

### Changed

- Re-pinned to `arora-behavior` 6 (the "golden"→"built-in" rename, ARORA-59). No
  API change here; the built-in clock keys this crate reads are unchanged.

## [6.0.0] - 2026-07-20

### Breaking

- Dispatch follows the one-argument `arora_call`: node calls carry their
  module in `Call::module_id`. Re-pinned to `arora-types` 2 / `arora-engine` 3.

## [5.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [5.0.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

## [4.2.0] - 2026-07-10

### Added

- Predetermined slots bind to the store unless linked (API-consistency PR 6)

## [4.1.0] - 2026-07-10

### Breaking

- 6.0.0 — the runtime's call dispatch is part of its face
- The golden behavior edit — a Call reaches interpreter.apply through the engine (PR 5b)

## [4.0.0] - 2026-07-09

### Breaking

- 4.0.0 — the graph-lowered interpreter is its own major
- Call_bridge, and edition that defers lowering to the tick
- A slot's direction is which Node list holds it
- Lower Groot import onto the shared Graph; arora glue
- Lower the shared Graph model onto the tree
- Shared graph model + GraphDiff + BehaviorInterpreter::apply

## [3.0.0] - 2026-07-09

### Breaking

- 3.0.0 — the empty-ready interpreter is its own major
- Design B — run drains the seams; the device owns them
- Empty-ready BehaviorTreeInterpreter + load

### Fixed

- Typed Value vocabulary + array-of-enum wire conformance (ARORA-55)
- Conform array-of-struct wire layout to serde_uuid (ARORA-55)
- Nested/recursive/dynamic type codegen (ARORA-55)

## [2.0.0] - 2026-07-08

### Breaking

- Rename Behavior trait to BehaviorInterpreter

## [1.0.0] - 2026-07-08

### Breaking

- Golden clock keys (time/dt) in the store; drop ctx.dt

## [0.1.1] - 2026-07-05

### Fixed

- Ship arora-behavior-tree's generated sources in the crate
- Crates.io forbids wildcard version constraints
- Clippy + readme fallout of the semio-record removal

### Changed

- Serde_yaml 0.9 everywhere — one emitter, one parser, formats unchanged

## [0.1.0] - 2026-07-04

### Breaking

- Drop the private semio-record dependency — type records live in arora-types

### Added

- A Behavior the runtime ticks, in an arora-behavior crate (VIZ-33)
- Bind behavior-tree variables to the data store
- Run the opinionated arora runtime on wasm

### Fixed

- Share one cell per variable id
- Keep children when converting a Groot tree
- Use the single in-workspace arora-types everywhere (incl. tests)

### Changed

- Variable cells use a VariableCell abstraction
- Fix module link + note native basic nodes
- Native control nodes; free arora of the BT-nodes module
- Group module-authoring crates under crates/arora-module-authoring/
- Fix relative links after consolidation + arora-engine rename

## [0.0.1] - 2026-06-26

### Added

- Implement cos and add math functions natively
- Support non-Status return_binding on nodes

### Fixed

- Convert string/regex nodes to Groot
- Resolve hand-written lints; allow on generated subtrees
- Use normalized_value() instead of deprecated unescape_value()
- IndexMap-based semio-record and improved WASM building

### Changed

- Bring the behavior-tree crates back into the workspace
- Extract behavior-tree into its own repo; consume it via git
- Drop the extra Box from Groot Node children
- Group engine tests into one to load modules once
- Use artifact dependency env var for behavior-tree-nodes WASM
- Forward WASM artifact dependency path to tests
- Move polly tests from arora-behavior-tree into modules/polly
- Make BehaviorTreeRuntime and ModuleFunction public
- Auto-rebuild behavior-tree-nodes WASM artifact for tests
- Wire cos and add from behavior-tree-nodes
- Replace return_binding with _ret out-parameter
- Type_mismatch now asserts Err instead of should_panic
- Document type mismatch as panic (WASM trap)
- Update READMEs for return_binding, BehaviorTreeRunner, and demo
- Add return_binding tests

