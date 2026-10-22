# arora-node-graph

Node graphs for Arora: a Vizij node graph (`GraphSpec`, its JSON form) runs
as the detached nodes of an `arora-behavior-tree`
([proposal](../../docs/proposal-behavior-languages.md)). The crate holds:

- **the node vocabulary as module functions** (`functions`): one function of
  the module `@arora/node-graph` per node kind, and `NodeGraphModule`, which
  answers their calls in-process through a bridge over the device's;
- **the importer** (`import`): a graph translated node for node into a tree
  and its detached nodes, as `BehaviorTree::try_from_groot_xml` reads a Groot
  file;
- **`TreeRuntime`** (`tree_runtime`): a graph run on the tree behind the
  reference evaluator's surface, for a host such as an editor or Vizij's
  `ProcessingGraph`;
- **the reference evaluator** (`GraphRuntime`, `evaluate_all`): the engine
  the graph format was defined on. A node's function runs its per-node code,
  and the equivalence test compares the two engines on every fixture.

## On the behavior tree

```rust
use arora_node_graph::import::import_json;
use arora_node_graph::tree_runtime::TreeRuntime;
use arora_shape::{TypedPath, Value};

// A graph's JSON as Vizij writes it: a spec, or a document holding one.
let (spec, _) = import_json(include_str!("../fixtures/sine-driver.json"), &Default::default())?;
let mut rt = TreeRuntime::default();
rt.load(&spec)?;
rt.dt = 1.0 / 60.0;
rt.t += rt.dt;
rt.set_input(TypedPath::parse("driver/time.seconds")?, Value::F32(0.5), None);
rt.evaluate()?;
for write in rt.writes.iter() {
    println!("{} = {:?}", write.path, write.value);
}
```

How a graph translates:

| In the graph | On the tree |
|---|---|
| a node | a detached call of its kind's function: it runs when what it outputs is read, once per tick, and only when its arguments changed |
| an input port | an optional parameter of the port's name; an edge reads the source's port: `@node`, or a field of the structure a node with ports beyond `out` returns |
| an edge's selector | a `select` node between the two |
| an inline default | a literal argument |
| a node parameter | a literal argument, `param.<field>` |
| hidden state (`spring`, `damp`, `slew`, IK) | the mutable parameter `state`, as a value: a smoother's buffers in its input's shape, an IK node's joints and last solve |
| `time`, `oscillator`, the smoothers' step | the parameters `t` and `dt`, read from the variables `ImportOptions` names (`arora/time` and `arora/dt` by default) |
| an `input` node | a read of the store key its path names |
| an `output` node with a path | a sink: its return value bound to the store key of its path, written after the tree ticks; the later-listed writer of a key wins |
| an `output` node with no path (a keyed batch) | listed in `ImportedGraph::keyed_outputs`, for its host to apply |
| `taskrun`, `spawn` | leaves under the root `PARALLEL`, ticked at every tick, as they act at each evaluation |
| `externalfunction` | placed by its function's description (`placement`): left out when the function returns nothing and writes nothing back; a leaf ticked at every tick when it takes no parameter, as its result can only come from outside its arguments, or when no description names it; a detached call otherwise |

What differs from the reference evaluator:

- **What runs.** The reference evaluates every node at every evaluation. On
  the tree a node runs when something reads it and its arguments changed;
  `Evaluation::Every` observes every node, as an editor does, and
  `Evaluation::Demanded` runs what the outputs read, as a device does. A
  node nothing reads does not fail a demanded evaluation.
- **Shapes.** Ports carry values; a port's shape is its declared output
  shape, the shape staged with an input, or the one its value infers.
- **Variadic ports.** A function declares `VARIADIC_PORTS` (64) ports of its
  kind's variadic group; the importer refuses a graph wiring one past them.
- **Edits.** A node's parameter value is set in place on the loaded tree
  (`TreeRuntime::set_params`). An edit that changes what the tree is (a
  node's ports, the path an input or output binds, a node added or rewired)
  takes a new load of the whole graph, which keeps every node's state.

The stress example times loading and stepping, and counts each step's calls:

```bash
cargo run --release -p arora-node-graph --example stress -- chain 10000
cargo run --release -p arora-node-graph --example stress -- file graph.json
```

---

# The reference evaluator

`GraphRuntime` and `evaluate_all` turn declarative `GraphSpec` documents into structured values and typed write operations, the semantics the node functions keep.

## Overview

- **Pure Rust runtime** that interprets `GraphSpec` documents using the shared `arora-shape` Value/Shape contract.
- **GraphRuntime** retains staged inputs, node-local state, and cached outputs across frames.
- **evaluate_all** walks the graph in topological order, performs selector projection, enforces declared shapes, and collects sink writes.
- **Extensible node library** covering math, logic, vector ops, time/transition nodes (Spring/Damp/Slew), blending, range tools (including multi-segment piecewise remaps), and (optionally) robotics IK/FK helpers behind the `urdf_ik` feature flag.

## Features

- Deterministic topological evaluation with cycle detection: a node runs after
  every node it reads from, and otherwise in the order the spec lists it, so of
  two outputs writing one path the later-listed wins.
- Input staging model with epoch tracking to prevent stale data from leaking between frames.
- Shape-aware validators (`Shape`, `ShapeId`) for predictable coercions and helpful diagnostics.
- Selector support (`field`, `index`) on edges for structured projection.
- External `WriteBatch` accumulation from sink nodes for host-controlled side effects.
- Optional URDF IK/FK nodes compiled in via the `urdf_ik` feature (enabled by default).

## Installation

```bash
cargo add arora-node-graph
```

Features:

| Feature   | Default | Description                                           |
|-----------|---------|-------------------------------------------------------|
| `urdf_ik` | ✔       | Enables URDF chain parsing + IK/FK nodes (depends on `k` and `urdf-rs`). |

Disable defaults with `--no-default-features` if you want a minimal build.

---

## Quick Start

```rust
use arora_shape::value::vec3;
use arora_shape::{Shape, ShapeId, TypedPath};
use arora_node_graph::{evaluate_all, GraphRuntime};
use arora_node_graph::types::GraphSpec;

let spec: GraphSpec = serde_json::from_str(include_str!("../fixtures/simple-gain-offset.json"))?;

let mut runtime = GraphRuntime::default();

// Stage an IK target for the next frame
let target = TypedPath::parse("robot/arm/ik_target")?;
runtime.advance_epoch();
runtime.set_input(target, vec3([0.1, 0.2, 0.3]), Some(Shape::new(ShapeId::Vec3)));

evaluate_all(&mut runtime, &spec)?;

for (node_id, outputs) in &runtime.outputs {
    println!("node {node_id}: {outputs:#?}");
}
for write in runtime.writes.iter() {
    println!("write {:?} -> {:?}", write.path, write.value);
}
```

---

## Usage

1. **Load or construct a `GraphSpec`.** Use `serde_json` to parse JSON, or build specs programmatically during tests.
   - `spec.validate()` refuses a graph that would fail every evaluation — an edge to a missing node, two edges into one input, two nodes sharing an id, a cycle, or a node leaving one of its `schema::required_inputs` with neither an edge nor an inline default — naming the node at fault. A host checks it before installing a graph.
2. **Create and reuse a `GraphRuntime`.** It retains node-local state (`Spring`, `Slew`, etc.), staged inputs, and cached outputs across frames.
3. **Stage host inputs.**
   ```rust
   runtime.advance_epoch();
   runtime.set_input(path, value, declared_shape);
   ```
   - Declared shapes help coerce numeric data or produce deterministically “null-of-shape” placeholders when staging fails.
4. **Evaluate the graph.**
   ```rust
   let eval = evaluate_all(&mut runtime, &spec)?;
   ```
5. **Consume results.**
   - `eval.nodes` – map of node → output key → `PortValue { value, shape }`.
   - `eval.writes` – `WriteBatch` of `WriteOp { path, value, shape }` emitted by `Output` nodes.
6. **Integrate with hosts.** Apply writes to your own blackboard, engine, or animation runtime as needed.

---

## Key Concepts

### GraphRuntime

- Holds `t`/`dt`, per-node persistent state, staged inputs (`HashMap<TypedPath, StagedInput>`), and cached outputs.
- `advance_epoch` bumps the staging epoch and evicts inputs not refreshed for the current frame.
- Evaluation updates `t`/`dt` when driven by a host such as the Arora behavior interpreter (`vizij-arora-behavior`); if you embed the runtime directly, set them yourself before calling `evaluate_all` if time-based nodes are involved.
- A `dt` of zero is a step with no time in it, and moves nothing that time drives: `Time` and `Oscillator` read the unchanged `t`, `Spring` records its new target and keeps its position and velocity, and `Damp` and `Slew` keep their value (so does a negative or non-finite `dt`). An evaluation at `dt = 0` therefore carries a changed input through every node that does not depend on time — a paused scene showing an edit — and a smoother it reaches takes it as its new target, which shows once time passes; the next step with time in it moves on as if the zero step had not been. An IK node answers a request equal to its last one with its last solution, so it holds too.
- **Plan cache note:** `arora-node-graph` caches a compiled execution plan (topological order + port layouts + resolved input bindings) inside `GraphRuntime`.
  - The plan cache is a *structural* cache. It exists to avoid repeating "planning" work when the graph layout is unchanged frame-to-frame.
  - For best steady-state performance, call `GraphSpec::with_cache()` on load and after any *structural* edit. This seeds/bumps `spec.version` and refreshes `spec.fingerprint`, enabling O(1) cache validation in `PlanCache::ensure_versioned()`.
  - **What counts as structural?** Anything that can change topology, port layouts, or bindings (nodes/edges, selector wiring, input defaults, and variadic port counts like `Split.sizes`) — and the `value` of a `Constant` read through a selector: the plan resolves what such a selector reads when it is built, so the evaluation neither reads nor copies the constant for it.
  - **What is not structural?** Most numeric `NodeParams` changes (e.g., `Oscillator.frequency`, the `value` of a `Constant` only read whole) affect evaluation outputs but do not change port layouts/bindings, so the cached plan remains valid.
  - If you do mutate a `GraphSpec` in-place, you are responsible for keeping the cache key consistent: call `with_cache()` after structural edits or rebuild the runtime/spec to avoid stale layouts.

#### Fingerprints and output shape hints

`GraphSpec::fingerprint` is intended to track *structural* plan validity. In particular, `fingerprint_spec()` includes the `value` of every `Constant` an edge reads through a selector, and `NodeSpec.output_shapes` **keys** (because the presence of declared outputs affects the output port layout), but does not currently include the declared `Shape` values because they do not affect plan layout.

### Selectors

- Edges can include selectors composed of field/index segments.
- Field selectors drill into records or structs (`["state", "translation"]`), while index selectors pick array/list elements (`["translation", 1]`).
- Example: `{"selector": ["translation", 1]}` on a `Transform` output lifts the Y component; chaining `["record_field", 2, "nested_field"]` is also supported.
- Selector evaluation respects shapes; invalid paths throw descriptive errors (“selector index 5 out of bounds for vec3”) instead of guessing. Catch these errors during development to keep graph definitions deterministic.

### Shapes & Values

- Values are `arora_shape::Value` (Arora's `Value`, re-exported); the vizij composites (vec2/vec3/vec4/quat/color/transform) are built and read through the `arora_shape::value` constructors and accessors.
- Shapes (`ShapeId`) describe numeric layouts and support inference. Sequences travel as one wire kind, so declared `Array`/`List`/`Tuple` shapes carry the distinction.
- Declared shapes on node outputs guard against schema drift and provide better error messages.

### Parameter updates

- `NodeParams` live on each `NodeSpec`. Mutate them before evaluation to tweak graph behaviour:
  ```rust
  if let Some(node) = spec.nodes.iter_mut().find(|n| n.id == "gain") {
      node.params.value = Some(arora_shape::value::float(0.5));
  }
  evaluate_all(&mut runtime, &spec)?;
  ```
- Shapes are checked during evaluation; incompatible updates surface as detailed `Err(String)` messages (`"set_param: node 'gain' key 'value' expects Float"` in wasm bindings).

### Cached outputs

- `GraphRuntime.outputs` stores a `HashMap<NodeId, HashMap<String, PortValue>>` containing the last evaluated value for every output port.
- Each `PortValue` carries both the `Value` and its inferred `Shape`. Reuse these snapshots between frames to drive inspectors or diffing tools:
  ```rust
  if let Some(port) = runtime.outputs.get("oscillator").and_then(|ports| ports.get("out")) {
      println!("shape: {:?}, value: {:?}", port.shape.id, port.value);
  }
  ```
- Cached values persist until the node is removed from the spec; the runtime automatically purges entries when graphs change or when you clear the runtime.

### External Writes

- `Output` nodes enqueue writes automatically when `params.path` is set.
- Writes include optional shapes so hosts can validate or coerce downstream.
- Use hosts like the Arora behavior interpreter (`vizij-arora-behavior`) or your own glue to apply them.

### URDF Feature

- Enable `urdf_ik` to pull in robotics helpers (`UrdfIkPosition`, `UrdfIkPose`, `UrdfFk` nodes).
- The WASM build ships with the feature enabled; native builds can opt out to reduce dependencies.

---

## Development & Testing

Run the crate tests (unit + integration):

```bash
cargo test -p arora-node-graph
```

Test the robotics nodes explicitly:

```bash
cargo test -p arora-node-graph --features urdf_ik
```

The benchmark evaluates chains, a mixed "kitchen" graph and the fixtures:

```bash
cargo bench -p arora-node-graph
```

Benchmark ideas:

- Evaluate large graphs under different topologies to profile numeric performance.
- Leverage the graphs in `fixtures/` to validate schema migrations.

---

## Related Packages

- [`vizij-graph-wasm`](https://github.com/vizij-ai/vizij-rs/blob/main/crates/node-graph/vizij-graph-wasm/README.md): wasm-bindgen binding that exposes JSON-friendly APIs plus normalization helpers.
- [`@vizij/node-graph`](https://github.com/vizij-ai/vizij-rs/blob/main/npm/@vizij/node-graph/README.md): npm wrapper around the wasm build with ABI guards and utilities.


