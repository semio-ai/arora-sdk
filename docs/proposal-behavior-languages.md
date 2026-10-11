# Proposal: one behavior tree, with node graphs as reactive nodes

Status: draft, for review.
Date: 2026-10-11.

A device runs behaviors that decide what starts, runs and ends — actions,
skills — and behaviors that turn values into values every tick — rigs,
mappings, controllers. People author the first as behavior trees and the
second as node graphs, and both mental models are worth supporting. Semio
Studio runs its emulated devices as the same Arora runtime as real ones, so a
robot gets node graphs and a face gets behavior trees; neither belongs to one
kind of device.

This document states how a device stores and runs both as **one language, the
behavior tree**, in which a node graph is a `reactive` node, and how the two
are offered to authors as two views of the same stored graph.

In short:

- **One language: the behavior tree.** A node graph is stored and run as a
  `reactive` tree node. Its children are compute nodes, and the links between
  those children are the graph's edges. No other data describes the graph.
- **A reactive node is change-driven.** Each tick it runs a compute node only
  when something it reads changed, and a result that comes out the same stops
  the change there. It is `Running` for as long as its parent ticks it; when
  the tree moves on, it is halted and its state goes.
- **A node graph ends at its reactive node.** Edges join nodes inside one
  reactive node and nowhere else. A value passes between reactive nodes, or to
  the rest of the tree, through a store key.
- **`layers` orders reactive nodes by precedence**: it ticks every child in
  order, and a later child's write of a key wins.
- **A module function is called by a node, never placed as one.** `call`, a
  tree leaf, calls it when ticked; `apply`, a compute node, calls it as its
  `when` slot says. The caller decides when the function is called and what
  happens to its result, and holds its mutable parameters' values.
- **The behavior host runs branches in parallel**, owns their order, halting
  and run policies, and refuses two branches writing one key. Its components
  serve versions of the behavior-tree format.
- **The node graph moves from vizij-rs into arora-sdk** with its git history,
  as the compute vocabulary and evaluator of the `reactive` node, and as the
  importer that reads Vizij's node-graph format into a tree.
- **Authoring has several views of one stored graph**: the tree, a reactive
  node opened and edited as a node graph, the same node drawn as the tree node
  it is stored as, and an overview of the keys that join reactive nodes.

---

## 1. What exists

**The runtime.**
- **One interpreter per device**, set at build
  (`AroraBuilder::with_behavior_interpreter`). The default is an empty
  behavior tree.
- **The interpreter is a module** under `interpreter_module::ID`, with four
  functions
  ([`interpreter_module`](../crates/arora-behavior/src/interpreter_module.rs)):
  - `LOAD(Graph)` replaces the main behavior;
  - `EDIT(GraphDiff)` edits it;
  - `SPAWN(Call, RunPolicy) -> TaskHandle` starts a task run;
  - `HALT(TaskId)` stops one.
- **`DescribeMethods` lists the methods an interpreter implements** (its
  `described_methods`), but not those four functions. A client knows their
  ids from `arora-behavior`.
- **`RunPolicy` has one variant, `Concurrent`.** Two runs writing one key
  overwrite each other; the last to write in a tick wins.

**The shared model.** An
[`arora_behavior::Graph`](../crates/arora-behavior/src/graph.rs) carries
nodes bound to a function id, links that feed a node's input from a literal,
a variable or another node's slot, and structure: `root` and each node's
ordered `children`. Each interpreter lowers it into its own runtime form;
nothing in it says which interpreter reads it, and the two interpreters read
disjoint vocabularies.

**The behavior tree** (`arora-behavior-tree`) ticks its control nodes
(`SEQ`, `SEQ_STAR`, `FALLBACK`, `PARALLEL`, which succeeds when every child
has), its status leaves and its run-fragment nodes natively, and dispatches
any other function id in its function index as a module call through
`call_bridge.arora_call`. A node's arguments are expressions: a literal, a
variable bound to a store key, another node's slot (`NodeArgument`), a
selection, or a nested `CallExpression { module, function, arguments }`,
evaluated each tick. A module function's mutable parameters are bound to
variables, so the tree holds the state the function needs between calls. A
runner, a `PARALLEL` root, holds the main behavior and one fragment per
spawned run.

**The node graph** is Vizij's (vizij-rs: `vizij-graph-core`, the engine;
`vizij-arora-behavior`, its interpreter `ProcessingGraph`):
- Each tick it reads every subscribed input path from the store, evaluates
  every node in topological order, and writes the outputs in one batch. It
  reports `Running` forever.
- Its root is a `layers` runner: the main behavior as one `flow`, then one
  `flow` per run, in spawn order. A later run writes after an earlier one
  ([vizij-rs#179](https://github.com/vizij-ai/vizij-rs/pull/179)).
- A face's main behavior composes its sources in precedence order — its own
  graphs, the mappings (ROS4HRI, the Studio-key mapping when serving Studio,
  and any the face embeds), the animations — into one spec, one `flow`, in
  which a later source's write of a key wins.
- An `ExternalFunction` node calls a module function by id through the same
  call bridge.
- **Skills are node-graph fragments**: `look_at`, `play_viseme` and `say`
  (vizij-rs `vizij-arora-host`). A fragment computes its own lifecycle as
  data and writes a `Status` value to `task/status`: `look_at` latches its
  start time through the store and selects with a `case` over its policy;
  the viseme players select on whether the run has ended; `say` hosts the
  provider's call in a `TaskRun` node.

**Studio's stepping run.** Studio delivers to every device, emulated or real,
a behavior that steps the animation module each tick and writes its outputs
under keys, beside the device's own behavior. It needs four additions to
arora-sdk, which proceed ahead of this proposal:
- `spawn_behavior(graph, policy)`: a graph as a task run;
- two built-in tree nodes, `Equal(a, b)` and `WriteKeys(keys, values)`, whose
  key table is a literal;
- the interpreter module's functions in `DescribeMethods`;
- the runner's `--groot` resolving module leaves, through `with_groot`.

[Section 7](#7-how-studios-additions-fit) maps each onto this design.

## 2. The cases the design holds

1. **A robot** runs its own behavior tree. Studio spawns its stepping run
   beside it.
2. **A Vizij face** runs its composed behavior: a `layers` of reactive nodes,
   with skill runs and programs spawned and halted beside it.
3. **A robot with a face** runs both as branches of one host: a tree for the
   robot and a tree for the face, and runs of either.
4. **A skill that decides and computes**: a tree whose control states the
   lifecycle and whose reactive nodes compute what it drives, for as long as
   their branch runs.
5. **One module function, two placements**: a third-party function called
   from a tree leaf in one behavior and from inside a node graph in another.
6. **A graph written for an older format** runs beside one written for a
   newer one, each on the component that reads it.
7. **Two runs driving one key**: the second is refused when it spawns, or its
   write fails, rather than both writing in turn.

## 3. Design

### 3.1 One language: the behavior tree

- **Every graph a device runs is a behavior tree.** A node graph is a subtree
  rooted at a `reactive` node: its children are compute nodes, and the links
  between those children, with a port source, are its edges.
- **The reactive node is a composite like `SEQ`.** Its semantics belong to
  the tree's executor; no second interpreter runs it, and no leaf bridges two
  languages.
- **Its children are stored in order, and the order is not the evaluation
  order.** The executor evaluates them upstream first; stored order breaks
  ties.
- **A node is a tree node or a compute node by where it sits**: under a
  reactive node, a compute node; anywhere else, a tree node. A control node or
  a leaf under a reactive node is refused, and so is a compute node anywhere
  else.

| Composite | Its children | Each tick | Status |
|---|---|---|---|
| `SEQ`, `FALLBACK`, `PARALLEL`, … | sub-behaviors | ticked as the control decides, by status | from its children |
| `layers` | sub-behaviors, in precedence order | every child that has not ended, in order; a later child's write of a key wins | see below |
| `reactive` | compute nodes | each that must run, upstream first | `Running` while ticked |

**`layers` ends when every child has.** A child that ends — `Success` or
`Failure` — is not ticked again, and its last writes stay; the others go on.
`layers` is `Running` while any child is; once every child has ended, it is
`Failure` if any child failed and `Success` otherwise. One child failing
neither fails the composite nor halts its siblings: `layers` states
precedence, not control. Halting `layers` halts every child still running.
A `layers` of reactive nodes therefore runs until it is halted, as a face's
standing behavior does.

`PARALLEL` gains `succeed_on: any`, which succeeds when one child has and
halts the rest. It is how a tree runs a reactive node for as long as another
child takes, such as a controller until a settle time (3.10).

### 3.2 How a reactive node runs

**Each tick, a compute node runs when:**
- it has never run since its reactive node started;
- any value it reads differs from the value it read last tick: a literal, a
  store key, an upstream node's output. Comparing values, not only asking
  whether an upstream node changed, also catches an edit: a new literal, a
  rewired or removed link, a different key;
- it reads the clock (`time`), which moves every tick;
- it is stateful and has not settled (a `damp` still easing);
- it is an `apply` whose `when` is `every_tick` (3.5).

**A result that comes out the same stops the change.** A node whose output
equals last tick's leaves its downstream nodes as they were, unless something
else they read changed.

**Reads and writes happen when a node runs.** An `input` node reads its key
from the store when it runs, in tree order, as any tree node reads a
variable; an `output` node writes its key when its value changes. Within a
reactive node an edge's value is always this tick's: the writer runs before
the reader. Anywhere else — another reactive node of the same tree, a tree
node, another branch — a reader sees this tick's value if it runs after the
writer in tick order, and the previous tick's if it runs before (3.4, 3.7).

**Lifecycle.**
- A reactive node is `Running` for as long as its parent ticks it. It never
  succeeds on its own: its parent decides when it stops, by moving to another
  child, by a `PARALLEL` resolving, or by a halt.
- **Halting drops its state**: every compute node's last value and every
  stateful node's integration. When its branch runs again it starts fresh.
  A stateful node starts from its first input, so `damp`, `spring` and
  `slew` gain an `initial` input: fed from a key's current value, a
  restarted branch eases from where the key is instead of jumping.
- **An edit keeps the state of the nodes it leaves.** A node keeps its id
  across an edit, so a spring keeps its velocity; the next tick runs whatever
  the edit changed.

**A reactive node's links form no loop.** Its evaluation order needs one, so
a load or edit that closes a loop is refused. A value fed back from one tick
to the next goes through a store key: an `output` node writes it and an
`input` node reads it next tick.

### 3.3 The compute vocabulary

The compute nodes are the node graph's generic vocabulary, moved into
arora-sdk (section 5): arithmetic, logic, ranges, time, oscillators and
noise, smoothing (`damp`, `spring`, `slew`), vectors, blending, records, IK
behind the `urdf_ik` feature, and:

| Node | Does |
|---|---|
| `input` | Reads the store key on its slot's predetermined key. |
| `output` | Writes its value to the store key on its slot's predetermined key, when the value changes. |
| `time` | The device clock, `arora/time`. Time since a branch started is the tree's to state, with `Wait`. |
| `damp`, `spring`, `slew` | As now, with an `initial` input their state starts from when it is linked (3.2). |
| `apply` | Calls a module function (3.5). |

A reactive node is evaluated by the node graph's engine: its plan, its value
types and its per-node state, keyed by node id.

### 3.4 Where a node graph ends

**Edges join compute nodes inside one reactive node.** A link whose source is
a port and that has a compute node at either end is valid only between two
children of the same reactive node. A load or edit with one between two
reactive nodes, or between a reactive node's child and a tree node, is
refused. A port link between two tree nodes keeps its `behavior-tree 1.0`
meaning: an argument that reads another node's slot (`NodeArgument`).

**Between reactive nodes, a value passes through a store key.** Inside a
reactive node, an edge's value has a definite time: this tick's, since the
writer runs first. Between two reactive nodes neither holds:
- the reader may tick before the writer, and see last tick's value;
- the writer's branch may not be running at all — another option of a
  `FALLBACK` — and its last value stays.

A named key makes both visible, and it is what the write analysis (3.8)
already reads: who writes the key, who reads it, and whether two parallel
branches write it.

**Precedence between reactive nodes is a `layers`.** A face is composed as one
`layers` with a reactive node per source — its own graphs, the mappings, the
animations — so a later source's write of a key wins, as listed order does in
the one `flow` a face is composed into today, and the precedence is
structure the write analysis can read. A face already composed into one
`flow` imports as one reactive node (3.6), in which two sources writing one
key is refused (3.8) until it is composed as a `layers` (section 10).

### 3.5 Module functions and the nodes that call them

**A module function is a function.** Its description, in
`arora_types::module::high`, is its parameters (each with a type, a
`mutable` flag and a default) and its return type. Behavior-tree node
functions are designed to be stateless: state a function needs across calls
is passed in a mutable parameter that the caller holds
([arora-behavior-tree](../crates/arora-behavior-tree/readme.md)). That is a
design aim, not a guarantee: a long-running function keeps per-invocation
state in its module behind a handle ([async functions](async-functions.md)),
and a host module may keep state of its own
([design decisions](design_decisions.md)).

**Two nodes call module functions.** Each names its function by the literal
on its `target` slot and has one slot per parameter of it:

| | `call`, a tree leaf | `apply`, a compute node |
|---|---|---|
| Called | When ticked | As its `when` slot says: `on_change`, when an argument changed; `every_tick` |
| Arguments | Literals or store keys | Literals, store keys, or edges from sibling compute nodes |
| Return | A `Status` decides the leaf's; any other value goes to the key bound to `result`, and the leaf succeeds | Its `result` output |
| A mutable parameter | Held by the caller: in the key it is bound to, or with the node while its branch runs | The same |

**What a description lets a caller rely on.**
- A function that returns `Status` reports progress — a long-running
  function polled until it ends ([async functions](async-functions.md)) — so
  only `call` can call it: a reactive node has no status for it to decide.
- A function with a mutable parameter carries state from call to call, so
  `apply` defaults to `every_tick`.
- Any other function, `apply` defaults to `on_change`.

**`when` belongs to the caller**, because a description cannot say that a
function keeps state inside its module or acts on the world. An author
who knows a function must be called every tick sets `every_tick` on the node
that calls it; the function is the same either way.

**The node a function is placed under decides how it is called; the function
does not change.** Moving a call from the tree into a node graph replaces a
`call` with an `apply`, with the same target and arguments.

**The tree's direct encoding stays readable.** A behavior-tree 1.x node whose
function id is a module function reads as a `call` of that function, its
`_ret` binding as `result`; a node graph's `ExternalFunction` reads as an
`apply`.

### 3.6 Graph types and versions

- **One graph type runs: `behavior-tree`.** Its version is the format's: a
  minor version adds to the format (a new node kind); a major changes what an
  existing graph means. A graph states the oldest version that has everything
  it uses; a component reading `1.4` runs any `1.x` graph with `x ≤ 4` and
  refuses a newer one, naming the version it reads.
- **Type and version are parameters of the call, beside the graph.** The host
  routes on them before the graph is decoded and checks an edit against the
  branch it targets.
- **`node-graph` is an import format.** A load or spawn typed
  `node-graph 1.0` — the encoding `vizij-arora-behavior` 5.4 decodes — is
  lowered to a behavior tree before it reaches a component: each `flow` to a
  reactive node, a `layers` composite in the graph to a `layers` of them,
  each `ExternalFunction` to an `apply`. A graph with no structure is one
  `flow`. The runs that `ProcessingGraph`'s root `layers` runner holds today
  are not part of a loaded graph: each is a branch of the host (3.7).
- **Lowering keeps node ids**, so a branch loaded as `node-graph 1.0` also
  accepts an edit typed `node-graph 1.0`: the diff is lowered the same way,
  node for node, and applied as a tree edit. Such a diff may touch only
  compute nodes and the links between them; one that changes structure is
  refused, naming the change. A branch is typed `behavior-tree` once loaded,
  and accepts `behavior-tree` edits whatever it was loaded as; a branch
  loaded as `behavior-tree` refuses a `node-graph` edit.
- The versions:
  - `behavior-tree 1.0`: what `arora-behavior-tree` 8.1 lowers;
  - `1.1`: `Equal`, `WriteKeys`;
  - `1.2`: `reactive`, the compute vocabulary, `layers`, `PARALLEL`'s
    `succeed_on`;
  - `1.3`: `call`, `apply`.

### 3.7 The behavior host and its branches

A device's behavior interface (the **behavior host**) runs branches and owns
what no single graph should: their order, their lifecycles, run policies and
write tracking.

**Components serve versions of the format.** The host is built from
interpreter components, each under a name unique within it, each declaring
the `behavior-tree` majors it reads. Two components serving one major is a
build error. A device built with one interpreter has one component, named
`device`. The host lives in `arora-behavior`, so the native runtime and
`arora-web` share it, and the runtime registers it as one module under
`interpreter_module::ID`.

```rust
pub struct GraphType {
    pub name: String,              // "behavior-tree"
    pub version: semver::Version,  // the format's version
}

pub trait BehaviorInterpreter {
    /// Per major of the format, the newest version this component reads.
    fn graph_types(&self) -> Vec<GraphType>;
    /// Accepts `graph` for `branch`, and returns the keys it writes, analyzed
    /// from the graph. Refuses a graph it cannot run.
    fn load(&mut self, branch: BranchId, ty: &GraphType, graph: Graph, scope: BranchScope)
        -> Result<WriteSet, BehaviorError>;
    /// The keys `branch`'s graph writes once `diff` applies. Changes nothing.
    fn analyze_edit(&self, branch: BranchId, diff: &GraphDiff)
        -> Result<WriteSet, BehaviorError>;
    fn edit(&mut self, branch: BranchId, ty: &GraphType, diff: GraphDiff)
        -> Result<(), BehaviorError>;
    fn tick(&mut self, branch: BranchId, ctx: &mut BehaviorContext)
        -> Result<BehaviorStatus, BehaviorError>;
    fn halt(&mut self, branch: BranchId) -> Result<(), BehaviorError>;
    fn remove(&mut self, branch: BranchId);
}
```

`BranchScope` carries what a run binds: its key prefix, onto which the
component maps the `task/` placeholder keys a graph names, and its
arguments.

**A branch is a loaded graph or a run.** A loaded graph is the device's
standing behavior, ticked every step until it is unloaded, whatever it
reports. A run starts from a spawn and ends at a halt, when its root reaches
a terminal status, or when it writes a terminal `Status` to its own
`task/status` key. The last is how a run whose root never ends — a reactive
node, a `layers` of them, an imported node-graph fragment — reports that it
is done.
A spawn runs a graph (`spawn_graph`), a skill (3.10), or a module function
returning `Status`, which the host calls each tick until it ends.

**Order.** Each step the host ticks every branch once: the loaded graphs in
load order, then the runs in spawn order. A branch added during a tick runs
from the next tick. Two branches do not write one key (3.8), so the order
decides only whether a reader sees another branch's write of this tick or
the previous one, and it is the same every time the device runs the same
branches. "Parallel" means no precedence between branches, not threads.

**Halting.** `halt` asks the component to stop the branch; a tree halts its
running nodes, which drops its reactive nodes' state. A halted run ends
`Failure` on its status key. Halting writes nothing back: returning a key to
rest is the client's step. `unload_graph` halts a loaded graph the same way.

**Run policies:**
- **`Concurrent`**: the run joins the others. A write overlap with any branch
  refuses the spawn.
- **`Replace`**: the runs whose writes overlap the new run's are halted before
  its first tick. An overlap with a loaded graph still refuses the spawn. A
  new viseme takes over from the one before this way.
- **`Overlap`**: the run joins even where its writes overlap other branches';
  on a key both write, the later in tick order wins. It serves runs whose
  precedence over a loaded graph is not yet written as a `layers` — a face's
  programs and skills over its animations.
- A skill states the policy it is spawned under when the caller gives none.

**A failure stays in its branch.** A `tick` error is transient and raised on
the device's behavior error. A run that ends `Done(Err)` is dropped; a loaded
graph that does is unloaded, and the error names it.

### 3.8 Two parallel branches never write one key

**An output is a store key a branch writes.** Keys under `arora/` belong to
the runtime: the clock, and each run's own keys under `arora/tasks/…/<run>/`,
which only that run writes.

**The write set is analyzed from the graph**, never declared beside it:
- an `output` node's key;
- a `call`'s `result` key, and the key bound to any mutable parameter of its
  target (read and written);
- an `apply`'s mutable parameters bound to keys;
- `WriteKeys`' key table, a literal so that it can be read off.

The host keeps the set with the branch and has the component analyze the
graph again on every edit, before the edit applies. Keys that only data names
— an `output` that writes the keys its records carry — are not in the set;
they are found at the write.

**At load, spawn and edit**, the host refuses the call when the analyzed set
overlaps another branch's set or claims, naming the key and the other branch
— except as a run's policy allows (3.7): under `Replace` the overlapping runs
are halted first, and a run under `Overlap` joins whatever it overlaps.

**At the write**, each branch writes through a view of the store. The first
write of a key no analyzed set names claims it for that branch until the
branch ends. A write to a key another branch holds is not applied and fails:
- a tree node that writes — `WriteKeys`, a `call` — returns `Failure`, and
  the tree reacts as its controls say;
- an `output` node in a reactive node does not write, and the tick reports
  the error on the device's behavior error; the rest of the reactive node's
  writes apply.

**Within one graph, the component checks its parallel parts** when the graph
is loaded or edited:
- a `PARALLEL`'s children whose keys overlap refuse the graph;
- two `output` nodes in one reactive node writing one key refuse the graph.

Ordered parts are not parallel: a `SEQ`'s children write one after the
other, and in a `layers` the later child takes precedence by design.

### 3.9 The behavior module's functions

Each is described, so `DescribeMethods` lists it by name, and the Studio
Bridge, the WebSocket bridge and the ROS 2 bridge reach it with no new wire
op.

| Function | Arguments | Returns | Effect |
|---|---|---|---|
| `load_graph` | `name: string`, `graph_type: string`, `graph_version: string`, `graph` | unit | Loads `graph` as the branch `name`, replacing a graph already loaded under that name, in its place. |
| `unload_graph` | `name: string` | unit | Halts the branch `name` and removes it. |
| `edit_graph` | `branch: string`, `graph_type: string`, `graph_version: string`, `diff` | unit | Edits a branch: a loaded graph by its name, a run by its task id. |
| `spawn_graph` | `graph_type: string`, `graph_version: string`, `graph`, `policy: RunPolicy` | `TaskHandle` | Starts `graph` as a task run. |
| `spawn` | `call: Call`, `policy: RunPolicy` | `TaskHandle` | Starts a module function as a task run (3.7). |
| `halt` | `task: TaskId` | unit | Stops a run. Idempotent. |
| `interpreters` | — | list | The components and the format versions they read. |
| `branches` | — | list | What runs, in tick order, and what each writes. |

- `graph` and `diff` are described as the dynamic record type: `Graph` and
  `GraphDiff` have no Arora type of their own.
- `LOAD`, `EDIT`, `SPAWN` and `HALT` keep their ids. `LOAD` and `EDIT` act on
  the branch `main`.
- `branches()` lists, per branch: its name or task id, its parent branch,
  its component, its type and version, the policy a run was spawned under,
  the keys it writes (analyzed) and the keys it claimed.
- Over the Studio Bridge these are commands like any other. Loading and
  editing are not offered over ROS 2, which has
  no message type for a graph; a skill returns `Status`, so it stays a ROS
  action.

### 3.10 Skills are behavior trees

A skill is a method implemented by a graph: its contract (function id, name,
signature) and a behavior tree. The host registers it, describes it, and
spawns its graph. Its parameters are the `task/` keys the graph reads.

A skill's lifecycle — start, wait, end, fail — is control, which the tree
states directly. What it drives each tick is a reactive node in the branch
that needs it:
- **`look_at`**: a `FALLBACK` over the policies. Each policy's branch checks
  `task/policy`, then runs its gaze. `glance` and `reset` run their gaze
  under `PARALLEL(succeed_on: any)` with a `Wait` for the settle time, so the
  run succeeds when it ends; `track`, `idle` and `random` run theirs until
  halted; an unsupported policy writes `ENOTSUP` to `task/result` and fails.
  Each gaze is a small reactive node: hold the target, rest, or wander.
- **`say`**: a `call` of the provider's `say`, which returns `Status`, beside
  the lipsync reactive node under `PARALLEL(succeed_on: any)`; then a
  reactive node brings the lips to rest.
- **`play_viseme`**: the lipsync reactive node for one shape, spawned under
  `Replace`.

The skill contracts stay where they are declared (vizij-rs
`vizij-arora-host`). A face bundle's `skill::<id>` entry carries its graph
type and version beside the graph.

**What changes for a branch that stops.** In a single node graph every
policy's gaze is computed every tick, selected or not, so a smoothed value
has history when its policy is selected. In the tree, an unselected branch is
halted: it costs nothing and keeps nothing, and its smoothing starts fresh
when selected. Where that difference shows, a smoothing node's `initial`
input starts it from the key's current value.

## 4. Authoring: one stored graph, several views

What is stored and run is the tree. The views are drawn from it, and every
edit made in one is an edit of the tree.

- **The tree.** Control and leaf nodes are edited here. A reactive node shows
  as one block, with the keys it reads and writes from the write analysis.
- **The node graph.** A reactive node opens as the network it is: its
  children are the graph's nodes and its links are the graph's edges. It is
  edited in place:

  | In the node graph | In the stored tree (`GraphDiff`) |
  |---|---|
  | Add a node | Add a compute node; append it to the reactive node's `children` |
  | Connect an output to an input | Add a link with a port source, replacing the input's link |
  | Disconnect | Remove the link |
  | Set a value on an input | Make the input's link a literal |
  | Change an `input` or `output` node's key | Set its slot's predetermined key |
  | Delete a node | Remove it from `children`, with its links |

  The view refuses what 3.1, 3.2, 3.4 and 3.5 refuse: a control node inside a
  reactive node, a loop, an edge to another reactive node, and an `apply` of
  a function that returns `Status`.
- **The reactive node as stored.** The same node drawn as the tree draws any
  tree node: the reactive node above its ordered children, with the links
  between children drawn apart from the tree's own edges.
- **The overview.** Every reactive node, and every tree node that touches the
  store, joined through the keys they read and write, with the conditions
  under which each runs.

**Presentation is not part of the graph.** Positions per view and labels are
kept by the authoring tool beside the graph, never in the `Graph` a device
runs. A missing position is laid out automatically, so a graph authored as a
tree opens as a node graph, and the reverse, with nothing else stored.

## 5. The node graph moves into arora-sdk

The default device runs node graphs with no Vizij crate, so the engine, its
generic node vocabulary and its importer move into arora-sdk. arora-sdk stays
free of Vizij.

### 5.1 What moves, what stays

| vizij-rs | In arora-sdk | Stays in vizij-rs |
|---|---|---|
| `vizij-graph-core` 2.4 | **`arora-node-graph`**: the engine, the plan, the node vocabulary (arithmetic, logic, time, smoothing, vectors, noise, blending, records, IK behind the `urdf_ik` feature), the external-function seam. `arora-behavior-tree` evaluates its `reactive` node with it. | — |
| `vizij-api-core` 2.0 | **`arora-shape`**: shapes, typed paths, the value vocabulary (vec2–4, quat, color, transform), blend, coercion, write batches, the graph-spec JSON forms | — |
| `vizij-arora-behavior` 5.4 | **`arora-node-graph-import`**: the `node-graph 1.0` codec and its lowering to a behavior tree (3.6) | the `look_at`, `play_viseme` and `say` registrations and the rig prefix of a skill's controls, into `vizij-arora-host` |
| `vizij-graph-wasm`, `vizij-graph-registry-export` | — | the editor's wasm bindings and registry export, over `arora-node-graph` |
| `vizij-arora-host` | — | the face's composition, the standard and ROS4HRI mappings, the profiles, the skills |
| `vizij-animation-core` | — | as is, over `arora-shape` |

- **No Vizij node kind enters `arora-node-graph`.** What is specific to
  Vizij is content — the face's sources, the mappings, the skills — and a
  function only Vizij needs is a function of a Vizij module, which an `apply`
  calls.
- **Wire ids stay.** The value vocabulary's type and field ids and the node
  type names are carried by saved graphs, module headers and Studio's
  introspection, so they keep their values. The Rust names lose their
  `Vizij` prefix (`VizijKind` becomes `ShapeKind`).
- **The npm packages keep their names** (`@vizij/node-graph-wasm`), built in
  vizij-rs over the arora crates.

### 5.2 The procedure

The history moves with `git filter-repo`, which rewrites every commit's paths,
so `git log` and `git blame` on the new paths reach back to the first commit
with no `--follow`. The three crates have never moved within vizij-rs, so one
path each covers their history. The fixtures moved once, from the wasm
package's tests, so both of their paths are kept.

```sh
# 1. Extract, in a fresh clone: filter-repo rewrites the clone it runs in.
git clone https://github.com/vizij-ai/vizij-rs.git vizij-extract
cd vizij-extract
git filter-repo \
  --path crates/node-graph/vizij-graph-core/ \
  --path crates/api/vizij-api-core/ \
  --path crates/interop/vizij-arora-behavior/ \
  --path fixtures/node_graphs/ \
  --path npm/@vizij/node-graph-wasm/tests/fixtures/ \
  --path-rename crates/node-graph/vizij-graph-core/:crates/arora-node-graph/ \
  --path-rename crates/api/vizij-api-core/:crates/arora-shape/ \
  --path-rename crates/interop/vizij-arora-behavior/:crates/arora-node-graph-import/ \
  --path-rename fixtures/node_graphs/:crates/arora-node-graph/fixtures/ \
  --path-rename npm/@vizij/node-graph-wasm/tests/fixtures/:crates/arora-node-graph/fixtures/

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
   - move the skill registrations out (`gaze`, `speech`, `viseme`) and drop the
     `vizij-arora-host` dependency;
   - point the tests, the benchmark and the README at the moved fixtures, in
     place of `vizij-test-fixtures`;
   - fix the moved READMEs' relative links, which the Markdown link check
     reads.
4. **The pull request lands as a merge commit.** A squash would drop the
   imported history, and a rebase would replay it as new commits.
5. **The new crates' first versions are published by hand**, `1.0.0`; then
   each crate's trusted publisher is registered, and the release workflow
   publishes every later version.
6. **In vizij-rs**, the crates depend on the arora crates, the skill
   registrations move into `vizij-arora-host`, and the three crates are
   removed. Their last published versions stay on crates.io.

The vizij-rs crates are licensed "MIT OR Apache-2.0", which allows them under
arora-sdk's MIT.

## 6. Versions and compatibility

| Crate or surface | Change |
|---|---|
| `arora-behavior` (9.1) | Minor: `GraphType`, the typed functions' ids and encoders, `RunPolicy::Replace` and `RunPolicy::Overlap` (the enum is non-exhaustive). Major: the component trait and the host. |
| `arora-behavior-tree` (8.1) | Minor: it declares `behavior-tree 1.x`; `Equal`, `WriteKeys` (1.1); `reactive`, `layers`, `PARALLEL`'s `succeed_on` (1.2), over `arora-node-graph`; `call`, `apply` (1.3). Major: the runner scaffold goes, with the component trait. |
| `arora` (12.3), `arora-web` (9.0) | Minor: the typed functions, described; `node-graph 1.0` accepted through the importer. Major: the builder composes components, and `Concurrent` refuses an overlap it accepted before. |
| `arora-node-graph`, `arora-node-graph-import`, `arora-shape` | New, `1.0.0`. |
| `vizij-graph-core`, `vizij-api-core`, `vizij-arora-behavior` | No further release once vizij-rs depends on the arora crates. `ProcessingGraph` retires: a face's graphs run as reactive nodes. |
| `vizij-arora-host` (6.6) | Major: it composes a face as a `layers` of one reactive node per source, and registers the skills, as trees. |
| Studio Bridge | No new `AroraOp`, so `studio-bridge-msgs` and the device client do not change. `studio-bridge-studio-client` gains typed helpers: a minor. |
| WebSocket and ROS 2 bridges | No change: they list and call described functions already. |
| Graph formats | `behavior-tree 1.0` (now), `1.1`, `1.2`, `1.3` (3.6). `node-graph 1.0` is read and lowered, never run as itself. |

## 7. How Studio's additions fit

| Addition | In this design |
|---|---|
| `spawn_behavior(graph, policy)` | `spawn_graph` with `graph_type = behavior-tree`. If the addition takes `graph_type` and `graph_version` from the start, Studio's call does not change later. |
| `Equal`, `WriteKeys` | `behavior-tree 1.1` nodes. `WriteKeys`' key table is a literal, so the analysis reads the stepping run's keys off its graph: on a robot whose own tree writes one of those keys, the spawn is refused instead of the two fighting. |
| The interpreter module's functions in `DescribeMethods` | The first part of 3.9: every function of the behavior module is described. |
| The runner's `--groot` through `with_groot` | Unchanged. The tree loads as the branch `main`. |

Vizij's `run_behavior` becomes `spawn_graph` with `graph_type = node-graph`,
lowered on arrival, and its `exclusive` fragments become spawns under
`Replace`.

## 8. Stages

Each stage lands on its own. In arora-sdk, every stage but 4 is additive;
stage 4 takes the majors it names.

1. **Typed graphs.** The typed functions join the module and are described.
   The one interpreter declares its format versions, and the untyped
   functions act for it. `interpreters()` and `branches()` answer from it.
2. **The node graph moves into arora-sdk** (section 5). Behavior does not
   change; vizij-rs depends on the arora crates.
3. **The reactive node.** `reactive`, `layers` and the compute vocabulary in
   `arora-behavior-tree` (`behavior-tree 1.2`), evaluated change-driven over
   `arora-node-graph`; `PARALLEL`'s `succeed_on`; the `node-graph 1.0`
   importer. A face's graphs run as reactive nodes, and `ProcessingGraph`
   retires. vizij-arora-host composes a face as a `layers` of one reactive
   node per source, so that stage 4's write tracking finds the face's
   precedence in its structure.
4. **The behavior host.** The component trait; branches in parallel, in the
   order 3.7 states; module-function runs hosted by the host; write tracking
   (3.8), with `Replace` and `Overlap`. Majors of `arora-behavior`,
   `arora-behavior-tree` and `arora`.
5. **Callers and skills.** `call` and `apply` (`behavior-tree 1.3`); the
   direct module-node encoding read as `call`; `look_at`, `play_viseme` and `say` rewritten as
   trees; bundle skill entries typed.
6. **Data-chosen keys within one graph.** A component tracks the keys that
   only data names between the parts of one graph, as the host does between
   branches.

## 9. Alternatives

| Alternative | Why not |
|---|---|
| The node graph as a second interpreter beside the tree, composed by the host, with a `run_graph` leaf for a tree to run a node graph | Two executors, two lowerings and two write analyses for one store; a bridge leaf wherever control meets dataflow; precedence between a tree and a node graph only through tick order or `Overlap`. As a reactive node, a node graph is one composite of one language, analyzed and checked as part of one graph. |
| A node graph lowered into tree patterns (a change test, the computation and a write per node) | Reading the node graph back out needs metadata that names which tree nodes form which graph node, and a tree edit can break the pattern. A reactive node holds the graph as itself, so the view needs nothing beyond positions. |
| A node graph stored apart from the tree, as authoring metadata beside it | Two sources of truth: the stored node graph and the tree it is compiled into can disagree. |
| A node graph drawn over the whole tree, with control as nodes that carry status as a value | Status as data is what a node graph's skill fragment computes today, and it cannot be read back into tree structure. Control stays the tree's; a node graph is a reactive node within it. |
| Edges between reactive nodes | Inside a reactive node an edge's value is this tick's; between reactive nodes it depends on tick order and on whether the writer's branch runs. A key makes both visible to the reader and to the write analysis. |
| A reactive node that sweeps every compute node every tick, as the node graph does today | Correct, and simpler, but a settled mapping then costs a full evaluation per tick for as long as it runs. Change detection costs one comparison per read and does no work for what did not change. |
| A module function placed as the node itself, its calling decided by where it sits | There is no place to say how a function is called — every tick or on change — when its description does not, and a tree leaf's return binding (`_ret`) poses as a parameter. A caller node holds both, and the function stays a function. |
| The type and version as fields of `Graph` | The host routes and checks the version before it decodes the graph, and a diff has no place for them. |
| The last writer wins between branches, in a declared order | Two branches driving one key is almost always a mistake, and an order only decides which one is lost, silently. Precedence that is meant is a `layers`; `Overlap` is the explicit exception. |
| The node graph left in vizij-rs | The default device would depend on Vizij, and a robot or the browser runtime could not run node graphs with stock Arora. |
| The code copied into arora-sdk without its history, or with `git subtree add` | The first loses `blame` and the reasons commit messages carry; the second keeps the commits at their vizij-rs paths, so `git log` on the new paths stops at the merge. |

## 10. Open details

The design above holds without them; each is to be investigated before the
stage it touches.

- **The cost of change detection.** A comparison per input read per tick,
  against a sweep of every node. Measured on a face's composed graph and on
  Studio's stepping run before stage 3 lands.
- **Effects a description does not state.** `when` on `apply` lets an author
  call a function every tick; whether a module function's description should
  also declare it (a flag beside `mutable`) is open.
- **`TaskRun` and `Spawn` nodes.** They express control as dataflow; whether
  the importer lowers them to tree structure or refuses a graph that uses
  them, until its control is rewritten, is open.
- **Faces composed before `layers`.** A face saved composed into one `flow`
  imports as one reactive node, whose sources writing one key the
  within-graph check refuses (3.8). Whether the importer splits a composed
  face back into its sources, or the check allows it for an imported graph,
  is open.
- **Retiring `Overlap`.** Once a face's precedence over its programs and
  skills is written as a `layers`, `Overlap` has no user left.
- **Names**: the crates, the `reactive` and `layers` composites, the `call`
  and `apply` nodes, and the host.
- **Notifying a change of branches**: a key the host writes when a branch
  starts or ends, beside `branches()`.
- **The untyped functions** (`LOAD`, `EDIT`, and `SPAWN_BEHAVIOR` once it
  lands): until which major they stay.
- **The HAL's keys.** Once a HAL describes the keys it writes
  ([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)), whether a
  branch writing one fails the same way.
