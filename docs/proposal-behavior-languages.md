# Proposal: node graphs as detached nodes of the behavior tree

Status: accepted direction, for implementation.
Date: 2026-10-22.

A device runs two kinds of behavior. A behavior tree decides what starts,
runs and ends: actions, skills. A node graph turns values into values every
tick: rigs, mappings, controllers. Semio Studio runs its emulated devices on
the same Arora runtime as real ones, so a robot gets node graphs and a face
gets behavior trees. Neither language belongs to one kind of device.

This document states how a device runs both as one behavior tree, whose nodes
are described one way whether the tree holds them or not. It states how
Vizij's node graphs and skills move onto it, and in which stages. It replaces
the design that [semio-ai/arora-sdk#299](https://github.com/semio-ai/arora-sdk/pull/299)
merged. §8 records why this design was chosen over that one and over the
other proposals.

In short:

- **One node, attached or detached.** Every node is described the same way:
  a function, and one argument per parameter. Its inputs take values, and its
  outputs (its return value and its mutable parameters) bind to variables,
  store keys or other nodes' inputs. A node the tree holds is **attached**: it
  runs when its parent ticks it. A call no node holds is **detached**: it
  runs when its outputs are about to be read.
- **Detaching a node states a contract**: when it computes does not matter,
  only that its outputs are up to date when they are used.
- **Reads are deduced, not marked.** A node reads another node's output as
  `@node._ret` or `@node.parameter`, or through the variable the output is bound
  to. Lowering finds which detached node writes what each node reads, and
  brings it up to date first. No expression is added for it.
- **A detached node runs at most once per tick, and only when its inputs
  changed.** A settled graph makes no calls, and an untaken branch's graph
  does not run.
- **An output bound to a store key makes a sink**, written at every tick after
  the tree ticks. Of two sinks of one key, the later one wins.
- **Either flow stands alone.** A tree with no detached nodes is a behavior
  tree as today. A node graph is detached nodes whose sinks run beside a tree
  that does nothing.
- **One interpreter.** Vizij's `ProcessingGraph` retires once faces run on the
  tree. Vizij's generic node vocabulary moves into arora-sdk with its history,
  as module functions whose state is a mutable parameter (§4), as animation
  already did.
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
  `HALT(TaskId)`. `spawn`, `spawn_graph` and `halt` are described, so
  `DescribeMethods` lists them; `LOAD` and `EDIT` are reached by id.
- **A graph states its type and version.** `arora_behavior::GraphType` names
  the language and its format version. The behavior tree reads
  `behavior-tree 1.3`: the control nodes, the status leaves, the task-run
  nodes and module function leaves (`1.0`), the data nodes `Equal` and
  `Unpack` (`1.1`), the data node `Scatter` (`1.2`), and detached nodes
  (`1.3`).
- **`RunPolicy` has one variant, `Concurrent`.** Two runs writing one key
  overwrite each other, and the last to write in a tick wins.

**The behavior tree** (`arora-behavior-tree` 9.3) keeps a runner, a
`PARALLEL` root. A LOAD grafts the main behavior under it, and each spawn
grafts a run beside it. A leaf calls a module function. Its return value
reaches a variable through its `_ret` argument, and each mutable parameter is
written back to the cell its argument names. The shared `Graph` model carries
nodes and links; a link from another node's port lowers to a
`NodeArgument`, which shares that port's cell.

**Animation is in arora-sdk.** `arora-animation` 1.0.0 is the clip format, its
evaluation and its players. `arora-animation-module` 1.0.0 declares it as the
module `@arora/animation` 3.0.0, which every device, emulated or real, loads
from its module directory ([animation](proposal-animation.md)). They came from
vizij-rs with their history.

**The node graph** is Vizij's (vizij-rs: `vizij-graph-core` 3.1, the engine;
`vizij-arora-behavior` 7.2, its interpreter `ProcessingGraph`):
- A node has named input ports and named output ports, `out` by default. A
  store key is written by an `output` node.
- Its root is a `layers` runner: the main behavior as one `flow`, then one
  `flow` per run, in spawn order. A later run writes after an earlier one.
- A face's main behavior composes its sources in precedence order: its own
  graphs, the mappings (the Studio and ROS4HRI profiles), the animations. The
  animations step vizij-rs's own animation module, which vizij-rs keeps until
  it depends on arora-sdk's.
- It evaluates every node of its plan at every tick.
- **Skills are node-graph fragments**: `look_at`, `play_viseme` and `say`
  (vizij-rs `vizij-arora-host`). A fragment computes its own lifecycle as
  data: it latches its start time through the store and computes its
  `Status` value.

**Detached nodes**, the design of §3, are in `arora-behavior-tree` 9.3
([#396](https://github.com/semio-ai/arora-sdk/pull/396), over the call
expressions of [#395](https://github.com/semio-ai/arora-sdk/pull/395)): a
tree runs the calls its graph lists and no node holds, as `behavior-tree
1.3`.

## 2. The cases the design holds

1. **A robot** runs its own behavior tree, with no detached nodes.
2. **A Vizij face** runs its composed node graph as detached nodes, with skill
   runs and programs spawned and halted beside it.
3. **A robot with a face** runs one tree for both: its decisions, and the
   face's dataflow beside them.
4. **A skill that decides and computes**: a tree whose leaves read detached
   nodes for as long as the skill runs.
5. **A graph that only computes**: detached nodes and their sinks, under a
   tree that does nothing.
6. **Control around dataflow**, such as greeting a person and then resting:
   the tree chooses a branch, and only the detached nodes the chosen branch
   reads run.

## 3. Design

### 3.1 One node, attached or detached

A node is the tree's `Node`, whether the tree holds it or not:

| Field | Meaning |
|---|---|
| `id` | The node's id, by which other nodes reference its ports. |
| `function` | The function it calls. |
| `arguments` | One per parameter, by parameter id: an expression, as for any node. |
| `children` | The nodes it ticks, for a control node. A detached node has none. |

**Every node has inputs and outputs.**
- **An input** is a parameter. Its argument is a literal, a variable, a store
  key, a nested call, a selection, or another node's port.
- **An output** is the return value or a mutable parameter. The return value
  is bound by the `_ret` argument; a mutable parameter is written back to the
  cell its argument names. Any number of outputs can be bound, each to a
  tree-local variable or a store key.
- **A port** is read as `@node._ret` (its return value) or `@node.parameter` (a
  mutable parameter's value), or through the variable it is bound to. A
  detached node's return value is a port whether or not `_ret` is bound. A
  port bound to a store key is still read at its current value: `@node._ret` is the
  node's result this tick, while the key holds what the node last published
  (§3.3).

A Vizij node maps port for port: its named output ports are the return value
and the mutable parameters of the function it becomes (§4.3).

**Attachment is the only difference.**
- A node is **attached** when the tree holds it: it is the root, or a child of
  an attached node. It runs when its parent ticks it, as today.
- A call to a module function that no node holds, and that holds no
  children, is **detached**. It runs when its outputs are about to be read.
  Detaching a node says that when it computes does not matter, only that its
  outputs are up to date when they are used.
- Another node no node holds, such as a control node an edit adds before the
  edit that places it, waits until a node holds it, as today.

**The text form is one syntax** for both kinds: a name and a colon when the
node is named, its bound return value before `=`, then the call. Detached
nodes are listed after the tree:

```text
fallback
  below(x: @cm._ret, limit: 10f64)
  below(x: @cm._ret, limit: 30f64)

raw: mul(a: {sensor/distance}, b: 100f64)
cm: clamp(x: @raw._ret, lo: 0f64)
level: {face/level} = add(a: @cm._ret, b: 1f64)
```

### 3.2 Reads are deduced from the source

Nothing marks a read of a graph node. When the tree is lowered, it finds which
detached node writes each tree-local cell: a bound return value, a mutable
parameter's cell, or a variable an output is bound to. A node that reads such
a cell, at any depth of its arguments, reads that detached node. Before it
ticks, the detached nodes it reads are brought up to date; a detached node
brings up to date those it reads first.

A **store key** is read as the store holds it. A value fed back through a key,
or passed from one graph to another through a key, reaches its reader at the
next tick, as between Vizij's sources today.

The **shared `Graph` model already carries this**: a detached node is a graph
node no structure holds, and an edge is a link from its port
(`LinkSource::Port`), which lowers to a `NodeArgument`.

### 3.3 Evaluation

- **Before its reader.** A detached node runs when a node that reads it is
  about to tick, or when a node it feeds runs. A detached node read only by a
  fallback child that does not tick does not run that tick.
- **Once per tick.** The first read of a detached node in a tick brings it up
  to date. Later reads in the tick reuse that result, however many readers it
  has. What the tree writes before the first read reaches the node. What it
  writes after reaches the node at the next tick, so a tick sees one snapshot
  of the graph.
- **Only when its inputs changed.** A detached node calls its function only
  when its argument values differ from those of its last call. A changed input
  runs only the nodes downstream of it, and a result that does not change
  stops the propagation there.
- **What changes at every tick runs at every tick.** A node that reads the
  clock (`arora/time`, `arora/dt`) is called once per tick, however many
  readers it has. A function whose result follows from none of its arguments
  is called once, so what it depends on is passed as an argument.
- **State lives in a mutable parameter.** A literal argument to a mutable
  parameter is the node's own cell. It starts from the literal and holds what
  the function writes back. A changed state is a changed input, so a stateful
  node runs until its state settles.
- **Sinks run every tick.** A detached node with an output bound to a store
  key is brought up to date after the tree ticks, whether or not the tree
  reads it, and even when the tree fails; a sink that fails stops no other,
  and publishes nothing that tick. Once every sink is up to date, each writes
  its keys, from its cache when its inputs did not change. Within a tick, the
  tree and the sinks read the store as it was, so a sink reading another
  sink's key sees it a tick late, as any value fed back through a key does.
  Of two sinks of one key, the later in the list wins whatever reads what, as
  the later writer does in a Vizij `flow`. The shared `Graph` holds its nodes
  unordered, so its list is the root, then the other nodes by id.
- **A detached node nothing reads never runs**, unless it is a sink or feeds
  one.

The **plan** classifies every detached node from what reads it: `every_tick`
holds the sinks and what they read, `on_demand` what the tree reads, and
`unused` the rest (`BehaviorTree::detached_plan()`).

**The build refuses** a reference to no node. **Lowering refuses** detached
nodes that read each other in a cycle, and a detached node reading an output
of its own other than its state, since feedback goes through a store key; a
call in a detached node's arguments writing back a store key, which the node
writes only through its outputs; an output bound to an input a call or a
selection computes, directly or through the ports that alias it, which has no
cell to feed, and a mutable parameter bound to another node's input that names
a store key, where a tree-local variable both bind does the same; a read of a
port that is neither a return value nor a mutable parameter; a tree-local cell
that a detached node and another node both write, counting what a nested call
or a native node such as `Unpack` writes, and every cell a call names when its
function or parameter is computed; a call to the interpreter module; an
argument to no parameter of the function; and a mutable parameter fed by a
call or a selection, neither of which has a cell to hold what the function
writes back.

### 3.4 Either flow alone

- **A tree alone** has no detached nodes, and runs as a behavior tree runs
  today, at no added cost.
- **A graph alone** is detached nodes whose sinks write the store, beside a
  tree that does nothing. The sinks run at every tick whatever the tree does.
- **Most content is both.** The tree states the control that a node graph
  encodes as data today: a latched start time, a `Status` value computed from
  comparisons, a `taskrun` or `spawn` node. The graph keeps only what turns
  values into values.

### 3.5 Lifetime

Detached nodes belong to the graph they are listed in: the main behavior, or
a run. A run's detached nodes go when the run ends. Their state lives in
their mutable parameters' cells, which an edit carries over as it does any
node's. Their caches start afresh after an edit, which costs one call.

They have no lifecycle of their own. A branch the tree stops ticking leaves
its detached nodes as they are, so a smoothing node continues from where it
was when the branch is ticked again. A behavior that needs to restart from
its input resets the state through an argument.

### 3.6 Authoring

A behavior is shown and edited in two views of one stored form:
- **the decision flow**, the attached nodes, as a tree;
- **the data flow**, the detached nodes, as a node graph;
- joined by links, from the port that produces a value to each port that
  reads it.

The text form of §3.1 is the stored form's readable rendering, for reviews,
diffs and agents, and it reads back into the stored form: the
[behavior text language](../crates/arora-behavior-tree/docs/text.md). The
diagrams and the text form in
[The NGBT spectrum](ngbt-spectrum.html) are sketches, to be redesigned or
refined before they reach the editors.

## 4. The node vocabulary moves into arora-sdk

arora-sdk stays free of Vizij. What moves is the generic node vocabulary, as
the functions of Arora modules, and the geometry types. Vizij's graph format,
its value vocabulary and the importer from a graph to a tree stay in vizij-rs,
over them. Animation has already moved the same way.

### 4.1 What moves, what stays

| vizij-rs | In arora-sdk | Stays in vizij-rs |
|---|---|---|
| `vizij-graph-core` 3.1 | **`arora-dataflow-nodes`**: the node vocabulary (arithmetic, logic, time, smoothing, vectors, noise, blending, records, IK behind the `urdf_ik` feature) as the functions of `@arora/dataflow`, `@arora/geometry` and `@arora/kinematics` (§4.3) | `vizij-graph-core` 4: the graph format, the editor registry, the importer from a graph to a tree and its detached nodes, `TreeRuntime`, and `@vizij/graph`'s `input`, `output` and declared-shape check |
| `vizij-api-core` 2.1 | **`arora-geometry`**: vec2–4, quat, transform and the blending of values; **`arora-graphics-types`**: the RGBA colour | `vizij-api-core` 3: shapes, typed paths, coercion, write batches and the graph-spec JSON forms, over `arora-geometry` and `arora-graphics-types` |
| `vizij-arora-behavior` 7.2 | — | `ProcessingGraph`, which runs a face on the tree through `vizij-graph-core` |
| `vizij-graph-wasm`, `vizij-graph-registry-export` | — | the editor's wasm bindings and registry export, over `vizij-graph-core` |
| `vizij-arora-host` | — | the face's composition, the standard and ROS4HRI mappings, the profiles, the skills |
| `vizij-animation-core` 5.0, `vizij-animation-module`, `vizij-animation-wasm` | **Moved**: `arora-animation` and `arora-animation-module` (`@arora/animation`) | until vizij-rs depends on the arora crates, then removed |

- **No Vizij node kind enters `arora-dataflow-nodes`.** What is specific to Vizij
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
with no `--follow`. Animation moved by this procedure
([#372](https://github.com/semio-ai/arora-sdk/pull/372)).

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
6. **In vizij-rs**, `vizij-graph-core` and `vizij-api-core` keep the graph
   format and Vizij's values, over `arora-dataflow-nodes` and
   `arora-geometry`, and the Vizij half of each moved crate returns there.

The vizij-rs crates are licensed "MIT OR Apache-2.0", which allows them under
arora-sdk's Apache-2.0.

### 4.3 Node kinds as functions

A Vizij graph translates node for node. Counted over 64 unique graphs (13
face GLBs, the fixtures, the perf scenes, the host's skill and mapping assets,
the standard adaptations), about 23,050 nodes over 86 node kinds:

| Vizij feature | On the tree (`arora-dataflow-nodes`, and `vizij-graph-core`'s importer) |
|---|---|
| arithmetic, ranges, vectors, logic, noise | one function of `@arora/dataflow` each (`@arora/geometry` and `@arora/kinematics` for the geometry and robotics kinds), called by one detached node per graph node |
| `input` | a call of `@vizij/graph`'s `input`, with a store key as an argument: it applies the node's default when the key is unset, and fits the value to the node's declared shape |
| `output` | a call of `@vizij/graph`'s `output`, its return bound to a store key, with the later sink winning, as the later `output` does in a `flow` |
| a node's declared output shapes | a call of `@vizij/graph`'s `check` after the node, which its readers read in its place |
| the keyed-batch `output`, which writes keys named by data | a detached node its host observes, and whose batch the host writes. A `Scatter` node of the tree writes keys named by data, under a literal prefix or anywhere in the tree's namespace; a detached node's output binds static keys only (§9) |
| non-input params (`half_life`, `case_labels`, noise settings) | literal arguments, `param.<field>` |
| variadic inputs (`operand_N`, `args_N`), most edges | one parameter per port: a function declares 64 ports of its variadic group, and the importer refuses a graph wiring one past them |
| named output ports (`weightedsumvector`, `readrecord`, `urdffk`) | a structure returned, each port a field, read through a selection |
| an edge's selector, which reads a field by name | a `select` node between the two, which reads it on the value alone |
| hidden state (`damp`, `spring`, `slew`, IK) | the function exposes its state as the mutable parameter `state`, as a value; a URDF chain is a cache of the module |
| `time`, `oscillator`, `dt` | functions taking the clock as arguments (`arora/time` and `arora/dt` on a device), so they run once per tick |
| implicit coercion and broadcasting | inside the functions, whose parameters stay dynamically typed |
| `case` and `if` | functions that evaluate every branch, as Vizij does |
| `taskrun`, `spawn` | attached leaves under the root: they act at every tick; a spawn's request is the host's to serve |
| `externalfunction` | an attached leaf, ticked at every tick, as Vizij calls it. A face's animation step calls `@arora/animation`'s `step` with the frame's `dt`: the same arguments every frame, a different result each time, as the module's players advance. A detached node would not call it again. The importer reads the function's description: a call to a function that returns nothing and writes nothing back is left out, as nothing could read it. |
| unused nodes | never evaluated: nothing reads them |

The animation step returns its tracks as values named by data. A `Scatter`
node of the tree ([#399](https://github.com/semio-ai/arora-sdk/pull/399))
writes them under the keys the tracks name, anywhere in the tree's namespace
([#403](https://github.com/semio-ai/arora-sdk/pull/403)).

## 5. What carries over from the previous design

These parts of the design #299 merged do not depend on the language, and
stay:

- **A graph travels with its type and version** in load, edit and spawn, as
  it already does in `SPAWN_GRAPH`. The device declares one type,
  `behavior-tree`, whose format gains detached nodes (§7).
- **Every function of the behavior module is described**: `load` and `edit`
  join `spawn`, `spawn_graph` and `halt` in `DescribeMethods`, and
  `branches()` lists what runs and what each writes.
- **Loaded graphs and runs tick in a stated order**: the loaded graphs in
  load order, then the runs in spawn order.
- **Two runs writing one key is an error.** The keys a tree writes are
  analyzed from the tree, and now include its sinks' keys. A `Scatter` whose
  `within` is empty or left out writes every key of the tree's namespace, as
  a module function's mutable parameter is written whether or not a call
  writes it. An overlap refuses the spawn, or fails the write. `Replace` and `Overlap` join `Concurrent` as
  run policies.
- **Skills are behavior trees.** `look_at`, `play_viseme` and `say` become
  trees whose computation is detached nodes of the run.

These parts no longer apply: the interpreter components and the `node-graph`
component, the `layers` and `flow` composites, whose precedence becomes the
order of sinks, and the `run_graph` leaf.

## 6. Integration plan

Each stage lands on its own and states what it hands to the next.

1. **Faces translated and measured.** The node vocabulary moves into
   arora-sdk (§4.2) and gains its functions (§4.3). An importer translates
   each composed face and fixture into a tree and its detached nodes, in Rust,
   the animation sources calling `@arora/animation`.
   - An equivalence test runs each graph over recorded inputs, on
     `ProcessingGraph` and on the tree, and compares every written key at
     every tick.
   - A benchmark measures time and calls per tick on both, on a settled face
     and on an animating one.
   - It hands over the list of what does not translate, and the cost per
     tick against `ProcessingGraph`. Value comparison costs a pass over a
     large value at every read; if the measure shows it, versions replace it
     (§9).
2. **Detached nodes in the authoring formats.** `behavior-tree 1.3` runs them
   already, from the shared `Graph` and the YAML node list. Groot interchange,
   traces of detached nodes, and an edit that keeps their caches remain.
3. **Runs and skills.** A run's detached nodes are scoped to it and removed
   with it. `look_at`, `play_viseme` and `say` become trees. A face's
   `taskrun` and `spawn` nodes become attached leaves and spawned runs.
4. **Faces on the tree.** Vizij's host loads the imported face as a tree with
   detached nodes. `ProcessingGraph` and `vizij-arora-behavior` retire.
5. **Authoring.** Studio and Vizij's editors show the decision flow and the
   data flow joined by links, and edit both (§3.6).
6. **The device-level parts of §5**: `load` and `edit` described,
   `branches()`, write analysis with sinks, `Replace` and `Overlap`.

## 7. Versions and compatibility

| Crate or surface | Change |
|---|---|
| `arora-behavior-tree` | 9.3: detached nodes, `behavior-tree 1.3`. `Expression` does not change. |
| `arora-behavior` (9.6) | No change for detached nodes: `Graph` carries them. Minor with §5: `Replace` and `Overlap`, since `RunPolicy` is non-exhaustive. |
| `arora` (13.6), `arora-web` | Minor: `load` and `edit` described. Major with §5: `Concurrent` refuses an overlap it accepted before. |
| `arora-dataflow-nodes`, `arora-geometry`, `arora-graphics-types` | New, `1.0.0`. |
| `vizij-graph-core` 4, `vizij-api-core` 3 | Major: over `arora-dataflow-nodes` and `arora-geometry`. |
| `vizij-animation-core`, `vizij-animation-module` | No further release once vizij-rs depends on the arora crates. |
| `vizij-arora-host` | Major: it registers the skills as trees. |
| Studio Bridge, WebSocket and ROS 2 bridges | No change: they list and call described functions already. |
| Graph formats | `behavior-tree 1.3` (9.3) runs the calls no node holds. A Vizij node graph is read by the importer only. |

## 8. Decision record

### 8.1 The decision

Node graphs run as detached nodes of the behavior tree, the design of §3,
first proposed in [#396](https://github.com/semio-ai/arora-sdk/pull/396).
It was chosen over the five designs of §8.2, compared on the examples of
[The NGBT spectrum](ngbt-spectrum.html). Its *Greet, then rest* example
compares them best: control around dataflow, where branches start and stop,
and each design resumes or restarts a branch's smoothing in its own way.

The design was chosen because:

- **It unifies both languages without giving up either.** A tree keeps every
  control node, task run and module leaf. A graph keeps its nodes, its sinks
  and their precedence.
- **Its stored form is the most readable, for people and for agents.** Every
  node is written one way, the tree holds control only, and an edge is a
  reference to a node's port.
- **It matches how behavior is presented to authors**: a decision flow and a
  data flow, joined where a node reads a value.
- **It runs both efficiently.** Pull evaluation skips what no ticking node
  reads, and the input cache skips settled calls.
- **A pure tree stays a pure tree,** with no detached nodes and no cost.
- **A pure graph needs only a minimal tree** for its sinks to run beside.
- **It covers Vizij's motion graphs, and simplifies their content.** The
  control a graph encodes as data today moves into the tree (§3.4).

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
| **NG in BT detached** (chosen) | [semio-ai/arora-sdk#396](https://github.com/semio-ai/arora-sdk/pull/396) | As detached nodes: calls the tree does not hold, run when their outputs are read, and cached |
| NG in BT ∥ | [semio-ai/arora-sdk#397](https://github.com/semio-ai/arora-sdk/pull/397) | As the same nodes, attached as leaves under a root `PARALLEL`, in data order |
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

**NG in BT ∥: graph nodes as leaves in data order.** The nodes of the chosen
design, resolved the same way, attached as plain module leaves under a root
`PARALLEL`, ahead of the tree.
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
- *Why not:* the decision flow lives inside the dataflow graph, as control
  nodes wired by child edges, so the tree's runtime is rebuilt in the graph's
  evaluator. #257 builds ticking and halting. Typed calls with mutable
  parameters, Groot interchange, and the host's runs and policies remain to
  rebuild. Nothing is cached.

## 9. Open details

The design holds without them. Each is settled before the stage it touches.

- **Versions instead of value comparison.** A detached node compares its
  argument values with those of its last call. For a transform or a
  52-element blend-shape array, that is a pass over the value at every read.
  Versions on cells cost one integer comparison, and need the store to
  version its writes.
- **Functions whose result does not follow from their arguments**: a module
  keeping state (the animation module's `step(dt)`, called with the same `dt`
  every frame), a sensor, a random source. A detached node calls them once
  for arguments it has seen, so the importer ticks a graph's call to another
  module at every tick. A function's description stating whether it is pure
  would let a pure one be a detached node, and an effect would let an impure
  one run at every tick it is read rather than at every tick.
- **Keyed outputs on detached nodes.** `Scatter`, a node of the tree, writes
  keys named by data, within a literal prefix or anywhere in the tree's
  namespace. Whether a detached node's output binds such keys itself, as a
  sink, and how the write analysis treats a prefix instead of a key, remain.
  Until then, the host of a translated graph writes its keyed batches.
- **Variadic inputs**: 64 declared ports per group, or an array parameter, or
  functions specialized by arity.
- **Incremental lowering.** A parameter value changes in place, as a literal
  argument set on the lowered tree. Any other edit lowers the whole tree
  again, carrying each node's state: a run grafted onto a face, or a node
  added in an editor, costs a lowering of every node.
- **Restarting state.** Whether a branch ticked anew restarts its smoothing
  by default, or only through an argument (§3.5).
- **Store hops between a face's sources** (ROS4HRI, adaptation, pose driver,
  rig). Kept as sinks and inputs, each hop costs a tick, as today. Read as
  ports within one graph, the hops resolve within the tick: a timing change to
  make deliberately.
- **A call an edit adds before the edit that places it** runs as a detached
  node meanwhile. An authoring tool that adds a leaf and its placement in one
  edit avoids it.
- **Names**: the `@node._ret` and `@node.parameter` syntax, the crates, and the module of node
  functions.
