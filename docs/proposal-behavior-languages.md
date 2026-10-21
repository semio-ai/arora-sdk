# Proposal: a behavior tree with its node graph beside it

Status: accepted direction, for implementation.
Date: 2026-10-21.

A device runs two kinds of behavior. A behavior tree decides what starts,
runs and ends: actions, skills. A node graph turns values into values every
tick: rigs, mappings, controllers. Semio Studio runs its emulated devices on
the same Arora runtime as real ones, so a robot gets node graphs and a face
gets behavior trees. Neither language belongs to one kind of device.

This document states how a device runs both as one behavior: a behavior tree
that holds the decisions, and **compute nodes** beside it that hold the
dataflow. It states how Vizij's node graphs and skills move onto it, and in
which stages. It replaces the design that
[semio-ai/arora-sdk#299](https://github.com/semio-ai/arora-sdk/pull/299)
merged. §8 records why this design was chosen over that one and over the
other proposals.

In short:

- **One behavior, two flows.** The tree is the decision flow. Compute nodes
  are the data flow. A tree leaf reads a compute node's output, `#node`, as
  one of its arguments. No bridge node stands between the two.
- **A compute node is one call to a module function.** Its arguments are
  expressions: literals, variables, store keys, nested calls, and other
  compute nodes' outputs.
- **Compute nodes are evaluated on demand.** A node runs when a ticking leaf
  reads it, or when a node it feeds runs. It runs at most once per tick, and
  calls its function only when its argument values changed. A settled graph
  makes no calls.
- **Sinks write the store every tick.** A compute node whose output is a
  store key is brought up to date after the tree ticks, whether or not the
  tree reads it. Of two sinks of one key, the later one wins.
- **Either flow stands alone.** A tree with no compute nodes is a behavior
  tree as today. A node graph is compute nodes whose sinks run beside a tree
  that does nothing.
- **One interpreter.** The behavior tree's runtime evaluates compute nodes in
  its own expression evaluator. Vizij's `ProcessingGraph` retires once faces
  run on the tree.
- **The node vocabulary becomes module functions.** Vizij's generic nodes
  move into arora-sdk with their history. Each becomes a function whose state
  is a mutable parameter (§4).
- **The integration goes in stages** (§6). The first translates the current
  Vizij faces and measures them against `ProcessingGraph`.

[The NGBT spectrum](ngbt-spectrum.html) simulates this design and the five
alternatives of §8 side by side, on the same examples, with live call counts
and timings. It is one HTML file: download it and open it in a browser. A
copy is also published at
<https://claude.ai/artifact/2KR7xbKViTy1F91gn3fj5y>, for members of the
Semio organization.

---

## 1. What exists

**The runtime.**
- **One interpreter per device**, set at build
  (`AroraBuilder::with_behavior_interpreter`). The default is an empty
  behavior tree.
- **The interpreter is a module** under `interpreter_module::ID`, with five
  functions: `LOAD(Graph)`, `EDIT(GraphDiff)`, `SPAWN(Call, RunPolicy)`,
  `SPAWN_GRAPH(graph_type, graph_version, Graph, RunPolicy)` and
  `HALT(TaskId)`.
- **A graph states its type and version.** `arora_behavior::GraphType` names
  the language and its format version. The behavior tree reads
  `behavior-tree 1.1`: the control nodes, the status leaves, the task-run
  nodes and module function leaves (`1.0`), and the data nodes `Equal` and
  `Unpack` (`1.1`).
- **`RunPolicy` has one variant, `Concurrent`.** Two runs writing one key
  overwrite each other, and the last to write in a tick wins.

**The behavior tree** (`arora-behavior-tree` 9.1) keeps a runner, a
`PARALLEL` root. A LOAD grafts the main behavior under it, and each spawn
grafts a run beside it. A leaf calls a module function. Its return value
reaches the tree through the `_RET_PARAM_ID` binding to a variable.

**The node graph** is Vizij's (vizij-rs: `vizij-graph-core`, the engine;
`vizij-arora-behavior`, its interpreter `ProcessingGraph`):
- Its root is a `layers` runner: the main behavior as one `flow`, then one
  `flow` per run, in spawn order. A later run writes after an earlier one.
- A face's main behavior composes its sources in precedence order: its own
  graphs, the mappings (the Studio and ROS4HRI profiles), the animations.
- It evaluates every node of its plan at every tick.
- **Skills are node-graph fragments**: `look_at`, `play_viseme` and `say`
  (vizij-rs `vizij-arora-host`). A fragment computes its own lifecycle as
  data: it latches its start time through the store and computes its
  `Status` value.

**Compute nodes**, the design of §3, exist in `arora-behavior-tree` as a
proof of concept ([#396](https://github.com/semio-ai/arora-sdk/pull/396),
over the call expressions of
[#395](https://github.com/semio-ai/arora-sdk/pull/395)). A tree is built
with them in Rust only, through `load_behavior_tree_with_compute`. No stored
format carries them yet.

## 2. The cases the design holds

1. **A robot** runs its own behavior tree, with no compute nodes.
2. **A Vizij face** runs its composed node graph as compute nodes, with skill
   runs and programs spawned and halted beside it.
3. **A robot with a face** runs one tree for both: its decisions, and the
   face's dataflow beside them.
4. **A skill that decides and computes**: a tree whose leaves read compute
   nodes for as long as the skill runs.
5. **A graph that only computes**: compute nodes and their sinks, under a
   tree that does nothing.
6. **Control around dataflow**, such as greeting a person and then resting:
   the tree chooses a branch, and only the compute nodes the chosen branch
   reads run.

## 3. Design

### 3.1 Compute nodes

A compute node is one call to a module function, held beside the tree:

| Field | Meaning |
|---|---|
| `id` | The node's id. Other nodes and the tree reference it by this id. |
| `function` | The module function it calls. |
| `inputs` | One argument per parameter, by parameter id. An argument is an expression, as a tree node's argument is. |
| `output` | Optional. A tree-local variable its return value is written to, or a store key, which makes the node a sink. |
| `volatile` | Call the function at every tick the node is read, even when its arguments did not change. |

- **A node's ports** are its return value, read as `#node`, and the value of
  each of its mutable parameters after its last call, read as
  `#node.parameter`.
- **An argument** is a literal, a variable, a store key, a nested call, or a
  reference to another compute node's port. The references join the compute
  nodes into a graph.
- **The tree reads the graph through the same references.** A leaf's argument
  can be `#cm` as it can be `{sensor/distance}`.

The text form lists one node per line, inputs before their readers:

```text
#raw = mul(a: {sensor/distance}, b: 100f64)
#cm = clamp(x: #raw, lo: 0f64)
#level = add(a: #cm, b: 1f64) -> {face/level}
```

```text
FALLBACK
  below(x: #cm, limit: 10f64)
  below(x: #cm, limit: 30f64)
```

### 3.2 Evaluation

- **On demand.** A node runs when a tree node that ticks reads it, or when a
  node it feeds runs. A node read only by a fallback child that does not
  tick does not run that tick.
- **Once per tick.** The first read of a node in a tick brings it up to
  date. Later reads in the tick reuse that result, however many readers it
  has. What the tree writes before the first read reaches the node. What it
  writes after reaches the node at the next tick, so a tick sees one snapshot
  of the graph.
- **Only when its inputs changed.** Bringing a node up to date first brings
  up to date the nodes it reads. The node then calls its function only when
  its argument values differ from those of its last call. A changed input
  runs only the nodes downstream of it, and a result that does not change
  stops the propagation there.
- **A volatile node calls its function at every tick it is read,** once
  however many readers it has. A sensor, a clock or a random source read as a
  function is volatile. A store key is not: a changed key is a changed input.
- **State lives in a mutable parameter.** A literal argument to a mutable
  parameter is the node's own cell. It starts from the literal and holds what
  the function writes back. A changed state is a changed input, so a stateful
  node runs until its state settles.
- **Sinks run every tick.** A node whose output is a store key is brought up
  to date after the tree ticks, whether or not the tree reads it. It writes
  its key at every tick, from its cache when its inputs did not change. Of two
  sinks of one key, the later in the nodes' order wins, as the later writer
  does in a Vizij `flow`.
- **A node nothing reads never runs**, unless it is a sink or feeds one.

The **plan** classifies every node from what reads it: `every_tick` holds the
sinks and what they read, `on_demand` what the tree reads, and `unused` the
rest (`BehaviorTree::compute_plan()`).

**The build refuses** a cycle, since feedback goes through a store key; a
reference to no compute node; and a compute node that reads a tree node's
argument. **Lowering refuses** a port that is neither a return value nor a
mutable parameter, and a mutable parameter fed by another node's port.

### 3.3 Either flow alone

- **A tree alone** has no compute nodes, and runs as a behavior tree runs
  today, at no added cost.
- **A graph alone** is compute nodes whose sinks write the store, beside a
  tree that does nothing. The sinks run at every tick whatever the tree does.
- **Most content is both.** The tree states the control that a node graph
  encodes as data today: a latched start time, a `Status` value computed from
  comparisons, a `taskrun` or `spawn` node. The graph keeps only what turns
  values into values.

### 3.4 Lifetime

Compute nodes belong to the tree they are loaded with: the main behavior, or
a run. Their caches and state live as long as that tree. A run's compute
nodes go when the run ends.

They have no lifecycle of their own. A branch the tree stops ticking leaves
its compute nodes as they are, so a smoothing node continues from where it
was when the branch is ticked again. A behavior that needs to restart from
its input resets the state through an argument.

### 3.5 Authoring

A behavior is shown and edited in two views of one stored form:
- **the decision flow**, as a tree;
- **the data flow**, as a node graph;
- joined by links, from the compute node that produces a value to each leaf
  that reads it.

The text form of §3.1 is the stored form's readable rendering, for reviews,
diffs and agents. The diagrams and the text form in
[The NGBT spectrum](ngbt-spectrum.html) are sketches, to be redesigned or
refined before they reach the editors.

## 4. The node vocabulary moves into arora-sdk

arora-sdk stays free of Vizij. What moves is the generic node vocabulary, the
value vocabulary, and the engine, which stays as the reference evaluator until
faces leave `ProcessingGraph` (§6).

### 4.1 What moves, what stays

| vizij-rs | In arora-sdk | Stays in vizij-rs |
|---|---|---|
| `vizij-graph-core` 2.2 | **`arora-node-graph`**: the node vocabulary (arithmetic, logic, time, smoothing, vectors, noise, blending, records, IK behind the `urdf_ik` feature) as module functions (§4.3); the engine and plan, as the reference evaluator; the importer from a Vizij graph to compute nodes and a tree | — |
| `vizij-api-core` 2.0 | **`arora-shape`**: shapes, typed paths, the value vocabulary (vec2–4, quat, color, transform), blend, coercion, write batches, the graph-spec JSON forms | — |
| `vizij-arora-behavior` 5.1 | — | `ProcessingGraph`, until faces run on the tree; then it retires |
| `vizij-graph-wasm`, `vizij-graph-registry-export` | — | the editor's wasm bindings and registry export, over `arora-node-graph` |
| `vizij-arora-host` | — | the face's composition, the standard and ROS4HRI mappings, the profiles, the skills |
| `vizij-animation-core`, `vizij-animation-module` | **`arora-animation`** and **`arora-animation-module`**: the clip format, its evaluation and players, and the module ([animation](proposal-animation.md)) | — |

- **No Vizij node kind enters `arora-node-graph`.** What is specific to Vizij
  is content: the face's sources, the mappings, the skills. A node only Vizij
  needs is a function of a Vizij module.
- **Wire ids stay.** The value vocabulary's type and field ids and the node
  type names are carried by saved graphs, module headers and Studio's
  introspection, so they keep their values. The Rust names lose their `Vizij`
  prefix (`VizijKind` becomes `ShapeKind`).
- **The npm packages keep their names** (`@vizij/node-graph-wasm`), built in
  vizij-rs over the arora crates.

### 4.2 The procedure

The history moves with `git filter-repo`, which rewrites every commit's paths,
so `git log` and `git blame` on the new paths reach back to the first commit
with no `--follow`.

```sh
# 1. Extract, in a fresh clone: filter-repo rewrites the clone it runs in.
git clone https://github.com/vizij-ai/vizij-rs.git vizij-extract
cd vizij-extract
git filter-repo \
  --path crates/node-graph/vizij-graph-core/ \
  --path crates/api/vizij-api-core/ \
  --path fixtures/node_graphs/ \
  --path-rename crates/node-graph/vizij-graph-core/:crates/arora-node-graph/ \
  --path-rename crates/api/vizij-api-core/:crates/arora-shape/ \
  --path-rename fixtures/node_graphs/:crates/arora-node-graph/fixtures/

# 2. Merge into arora-sdk, on a branch of its own.
cd ../arora-sdk
git switch -c feat/node-graph-from-vizij origin/main
git fetch ../vizij-extract main:vizij-extract
git merge --allow-unrelated-histories --no-ff vizij-extract
git branch -D vizij-extract
```

3. **Ordinary commits on the same branch**, so the merge itself changes
   nothing it imports:
   - rename the packages, add them to the workspace, and make their
     dependencies path dependencies on arora-sdk's crates;
   - point the tests, the benchmark and the README at the moved fixtures, in
     place of `vizij-test-fixtures`;
   - fix the moved READMEs' relative links, which the Markdown link check
     reads.
4. **The pull request lands as a merge commit.** A squash would drop the
   imported history, and a rebase would replay it as new commits.
5. **The new crates' first versions are published by hand**, `1.0.0`. Then
   each crate's trusted publisher is registered, and the release workflow
   publishes every later version.
6. **In vizij-rs**, the crates depend on the arora crates, and the two moved
   crates are removed. Their last published versions stay on crates.io.

The vizij-rs crates are licensed "MIT OR Apache-2.0", which allows them under
arora-sdk's Apache-2.0.

### 4.3 Node kinds as functions

A Vizij graph translates node for node. Counted over 64 unique graphs (13
face GLBs, the fixtures, the perf scenes, the host's skill and mapping assets,
the standard adaptations), about 23,050 nodes over 86 node kinds:

| Vizij feature | As compute nodes |
|---|---|
| arithmetic, ranges, vectors, logic, noise | one function each, called by one compute node per graph node |
| `input` | a store key as an argument. A default for an unset key is an optional parameter with a default, or a `default(x, d)` function. |
| `output` | a sink, with the later sink winning, as the later `output` does in a `flow` |
| the keyed-batch `output`, which writes keys named by data | not yet: a sink writes one static key (§9) |
| non-input params (`half_life`, `case_labels`, noise settings) | literal arguments |
| variadic inputs (`operand_N`, `args_N`), most edges | an array parameter and an array-building function, or functions specialized by arity |
| multi-output ports | the return value and mutable parameters, read as `#node.parameter`, or a record return read through a selection |
| hidden state (`damp`, `spring`, `slew`, the URDF cache) | the function exposes its state as a mutable parameter |
| `time`, `oscillator`, `dt` | functions reading `arora/time` and `arora/dt` as arguments |
| implicit coercion and broadcasting | inside the functions, whose parameters stay dynamically typed |
| `case` and `if` | functions that evaluate every branch, as Vizij does |
| `taskrun`, `spawn` | tree leaves and spawned runs: they are control |
| `externalfunction` | a compute node: it is a module call already |
| unused nodes | never evaluated: no reader pulls them |

## 5. What carries over from the previous design

These parts of the design #299 merged do not depend on the language, and
stay:

- **A graph travels with its type and version** in load, edit and spawn, as
  it already does in `SPAWN_GRAPH`. The device declares one type,
  `behavior-tree`, whose format gains compute nodes (§7).
- **The behavior module's functions are described**, so `DescribeMethods`
  lists them, and `branches()` lists what runs and what each writes.
- **Loaded graphs and runs tick in a stated order**: the loaded graphs in
  load order, then the runs in spawn order.
- **Two runs writing one key is an error.** The keys a tree writes are
  analyzed from the tree, and now include the store keys of its sinks. An
  overlap refuses the spawn, or fails the write. `Replace` and `Overlap`
  join `Concurrent` as run policies.
- **Skills are behavior trees.** `look_at`, `play_viseme` and `say` become
  trees whose computation is compute nodes of the run.

These parts no longer apply: the interpreter components and the `node-graph`
component, the `layers` and `flow` composites, whose precedence becomes the
order of sinks, and the `run_graph` leaf.

## 6. Integration plan

Each stage lands on its own and states what it hands to the next.

1. **Faces translated and measured.** The node vocabulary moves into
   arora-sdk (§4.2) and gains its functions (§4.3). An importer translates
   each composed face and fixture into compute nodes and a tree, in Rust.
   - An equivalence test runs each graph over recorded inputs, on
     `ProcessingGraph` and on the tree, and compares every written key at
     every tick.
   - A benchmark measures time and calls per tick on both, on a settled face
     and on an animating one.
   - It hands over the list of what does not translate, and the cost per
     tick against `ProcessingGraph`. Value comparison costs a pass over a
     large value at every read; if the measure shows it, versions replace it
     (§9).
2. **Compute nodes in the stored format.** The shared `Graph` model, the
   Groot import and the YAML form carry compute nodes, as `behavior-tree 1.2`.
   An edit keeps the caches and state of the compute nodes it leaves, as it
   keeps a tree node's. Traces cover compute nodes. `arora-behavior-tree`
   takes a major: `Expression` gains `Computed`.
3. **Runs and skills.** A run's compute nodes are scoped to it and removed
   with it. `look_at`, `play_viseme` and `say` become trees. A face's
   `taskrun` and `spawn` nodes become tree leaves and spawned runs.
4. **Faces on the tree.** Vizij's host loads the imported face as a tree with
   compute nodes. `ProcessingGraph` and `vizij-arora-behavior` retire, and
   vizij-rs depends on the arora crates.
5. **Authoring.** Studio and Vizij's editors show the decision flow and the
   data flow joined by links, and edit both (§3.5).
6. **The device-level parts of §5**: described functions and `branches()`,
   write analysis with sinks, `Replace` and `Overlap`.

## 7. Versions and compatibility

| Crate or surface | Change |
|---|---|
| `arora-behavior-tree` (9.1) | Major: `Expression::Computed`, compute nodes, `behavior-tree 1.2`. |
| `arora-behavior` (9.6) | Minor: compute nodes in `Graph` and `GraphDiff`. Major with §5: the run policies and the write analysis. |
| `arora` (13.6), `arora-web` | Minor: the described functions. |
| `arora-node-graph`, `arora-shape` | New, `1.0.0`. |
| `vizij-graph-core`, `vizij-api-core`, `vizij-arora-behavior` | No further release once vizij-rs depends on the arora crates. |
| `vizij-arora-host` | Major: it registers the skills as trees. |
| Studio Bridge, WebSocket and ROS 2 bridges | No change: they list and call described functions already. |
| Graph formats | `behavior-tree 1.2` adds compute nodes. A Vizij node graph is read by the importer only. |

## 8. Decision record

### 8.1 The decision

Node graphs run as compute nodes beside the behavior tree, the design of §3,
first proposed in [#396](https://github.com/semio-ai/arora-sdk/pull/396).
It was chosen over the five designs of §8.2, compared on the examples of
[The NGBT spectrum](ngbt-spectrum.html). Its *Greet, then rest* example
compares them best: control around dataflow, where branches start and stop
and a smoothing restarts.

The design was chosen because:

- **It unifies both languages without giving up either.** A tree keeps every
  control node, task run and module leaf. A graph keeps its nodes, its sinks
  and their precedence.
- **Its stored form is the most readable, for people and for agents.** The
  tree holds control only. The graph is one line per node, and an edge is a
  reference to a node.
- **It matches how behavior is presented to authors**: a decision flow and a
  data flow, joined where the tree reads a value.
- **It runs both efficiently.** Pull evaluation skips what no ticking leaf
  reads, and the input cache skips settled calls.
- **A pure tree stays a pure tree,** with no compute nodes and no cost.
- **A pure graph needs only a minimal tree** for its sinks to run beside.
- **It covers Vizij's motion graphs, and simplifies their content.** The
  control a graph encodes as data today moves into the tree (§3.3).

It is the compromise to build on now, for the short and middle term, given the
current goals, timeframe and knowledge. It is not proven as the long-term
design. How well it serves other graph types is learned by running them on
it, starting with Vizij's faces (§6, stage 1).

### 8.2 The alternatives

Each row is a design the comparison simulates, under its tab name there.

| Design | Pull request | How it runs a node graph |
|---|---|---|
| NG + BT | [semio-ai/arora-sdk#299](https://github.com/semio-ai/arora-sdk/pull/299) | On an interpreter of its own, as a branch the host ticks beside the tree's |
| NG as BT | [semio-ai/arora-sdk#332](https://github.com/semio-ai/arora-sdk/pull/332) | As a fixed shape of plain tree nodes, one guarded pattern per graph node |
| BT with expressions | [semio-ai/arora-sdk#395](https://github.com/semio-ai/arora-sdk/pull/395) | As nested call expressions in the arguments of tree leaves |
| **NG in BT detached** (chosen) | [semio-ai/arora-sdk#396](https://github.com/semio-ai/arora-sdk/pull/396) | As compute nodes beside the tree, evaluated on demand and cached |
| NG in BT ∥ | [semio-ai/arora-sdk#397](https://github.com/semio-ai/arora-sdk/pull/397) | As the same compute nodes, compiled to leaves under a root `PARALLEL`, in data order |
| BT in NG | [vizij-ai/vizij-rs#257](https://github.com/vizij-ai/vizij-rs/pull/257) | The other way round: control nodes in Vizij's node graph tick the nodes wired into them |

**NG + BT: interpreters running graphs as branches.** Behavior trees and
node graphs run on interpreter components of their own. The host ticks each
loaded graph and run as a branch: loaded graphs in load order, then runs in
spawn order. The languages meet only through the store and a `run_graph` leaf,
which starts a graph as a run from the next step.
- *What it keeps:* each language as it is, and the device-level parts of §5.
- *Why not:* it composes the two languages rather than unifying them. A value
  crosses between them only through a store key, and a graph a leaf starts
  runs from the next step. A node graph evaluates every node at every tick.

**NG as BT: node graphs as a shape of the tree.** One language. A node graph
is a `PARALLEL` with one pattern per node,
`FALLBACK[SEQ[changed?, f], SUCCEED]`, and a `RUNNING` leaf last. Edges are
tree-local variables, and `changed?` compares their versions.
- *What it keeps:* one semantics and one executor; settled calls skipped
  through integer version comparisons.
- *Why not:* the stored form is about four tree nodes per graph node, and it
  reads as a node graph only through an editor that recognizes the shape. Every
  pattern is visited at every tick, those of an untaken branch included.

**BT with expressions: call expressions in the tree.** A leaf's arguments
nest calls to module functions, evaluated innermost first each time the leaf
ticks. A cascade reads in place.
- *What it keeps:* the cascade written where it is used; nested calls remain
  valid arguments in the chosen design.
- *Why not:* a value read by several leaves is computed by each, and nothing
  is cached. A shared value needs a leaf of its own and a variable.

**NG in BT ∥: compute nodes as leaves in data order.** The compute nodes of
the chosen design, resolved the same way, compiled to plain module leaves
under a root `PARALLEL`, ahead of the tree.
- *What it keeps:* no evaluation rule beyond the tree's.
- *Why not:* every leaf runs at every tick, whatever branch the tree takes.
  What the tree writes reaches the leaves at the next tick. A smoothing keeps
  running while its branch is inactive.

**BT in NG: control nodes in the node graph.** Vizij's node graph gains
`sequence`, `fallback`, `parallel` and `inverter` nodes, which tick the
nodes wired into their `child_N` inputs. A node that only control reads is
lazy, and runs only when its branch is ticked.
- *What it keeps:* one graph, and lazy branches that cost nothing until they
  are ticked.
- *Why not:* the decision flow becomes child edges inside the dataflow, so the
  two flows are not shown apart. The tree's runtime (task runs, asynchronous
  module calls, halting, Groot interchange) would be rebuilt in the graph's
  evaluator. Nothing is cached.

## 9. Open details

The design holds without them. Each is settled before the stage it touches.

- **Versions instead of value comparison.** A compute node compares its
  argument values with those of its last call. For a transform or a
  52-element blend-shape array, that is a pass over the value at every read.
  Versions on cells cost one integer comparison, and need the store to
  version its writes.
- **Keyed-batch outputs**, which write keys named by data: a sink with a
  dynamic key, and how the write analysis treats it.
- **Variadic inputs**: an array parameter, or functions specialized by arity.
- **Selections on records** read by field name, as Vizij's edges select a
  `KeyValue` field.
- **Restarting state.** Whether a branch ticked anew restarts its smoothing
  by default, or only through an argument (§3.4).
- **Store hops between a face's sources** (ROS4HRI, adaptation, pose driver,
  rig). Kept as sinks and inputs, each hop costs a tick, as today. Merged
  into one graph, the hops resolve within the tick: a timing change to make
  deliberately.
- **Names**: the `#node` syntax, the crates, and the module of node
  functions.
