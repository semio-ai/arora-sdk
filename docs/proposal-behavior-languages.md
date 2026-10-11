# Proposal: one behavior tree, with node graphs as reactive nodes

Status: draft, for review.
Date: 2026-10-11.

A device runs two kinds of behavior. Some behaviors decide what starts, runs
and ends, such as actions and skills. Other behaviors turn values into values
every tick, such as rigs, mappings and controllers. People author the first
kind as behavior trees and the second kind as node graphs. Both mental models
are worth supporting. Semio Studio runs its emulated devices on the same Arora
runtime as real devices. Thus a robot gets node graphs, and a face gets
behavior trees. Neither language belongs to one kind of device.

This document states how a device stores and runs both kinds as **one
language, the behavior tree**. In that language, a node graph is a `reactive`
node. The document also states how authors see the two kinds as two views of
the same stored graph.

In short:

- **One language: the behavior tree.** A device stores and runs a node graph
  as a `reactive` tree node. Its children are compute nodes. The links
  between those children are the edges of the graph. No other data describes
  the graph.
- **A reactive node is change-driven.** Each tick, it runs a compute node
  only when something that the node reads changed. A result that does not
  change stops the change there. The reactive node is `Running` while its
  parent ticks it. When the tree moves on, the reactive node is halted, and
  its state goes.
- **A node graph ends at its reactive node.** Edges join nodes inside one
  reactive node and nowhere else. A value passes between reactive nodes, or to
  the rest of the tree, through a store key.
- **`layers` orders reactive nodes by precedence.** It ticks every child in
  order. When two children write one key, the write of the later child wins.
- **A node calls a module function. The function is never a node.** `call`
  is a tree leaf that calls the function when ticked. `apply` is a compute
  node that calls the function as its `when` slot says. The caller decides
  when to call the function and what happens to its result. The caller also holds
  the values of the mutable parameters of the function.
- **The behavior host runs branches in parallel.** It owns their order,
  halting and run policies. It refuses two branches that write one key. Its
  components serve versions of the behavior-tree format.
- **The node graph moves from vizij-rs into arora-sdk** with its git history.
  It becomes the compute vocabulary and the evaluator of the `reactive` node.
  It also becomes the importer that reads the Vizij node-graph format into a
  tree.
- **Authoring has several views of one stored graph:**
  - the tree
  - a reactive node that opens as a node graph, for edits
  - the same reactive node, drawn as the tree node that the device stores
  - an overview of the keys that join reactive nodes.

---

## 1. What exists

**The runtime.**
- **One interpreter per device.** The build sets it
  (`AroraBuilder::with_behavior_interpreter`). The default is an empty
  behavior tree.
- **The interpreter is a module** under `interpreter_module::ID`. It has four
  functions
  ([`interpreter_module`](../crates/arora-behavior/src/interpreter_module.rs)):
  - `LOAD(Graph)` replaces the main behavior.
  - `EDIT(GraphDiff)` edits the main behavior.
  - `SPAWN(Call, RunPolicy) -> TaskHandle` starts a task run.
  - `HALT(TaskId)` stops a task run.
- **`DescribeMethods` lists the methods that an interpreter implements** (its
  `described_methods`). It does not list the four functions above. A client
  gets their ids from `arora-behavior`.
- **`RunPolicy` has one variant, `Concurrent`.** Two runs that write one key
  overwrite each other. The last write in a tick wins.

**The shared model.** An
[`arora_behavior::Graph`](../crates/arora-behavior/src/graph.rs) carries
three things:
- nodes, each bound to a function id
- links, each of which feeds the input of a node from a literal, a variable
  or the slot of another node
- structure: `root`, and the ordered `children` of each node.

Each interpreter lowers the graph into its own runtime form. Nothing in the
graph says which interpreter reads it. The two interpreters read vocabularies
that do not overlap.

**The behavior tree** (`arora-behavior-tree`) runs these nodes natively: its
control nodes (`SEQ`, `SEQ_STAR`, `FALLBACK`, `PARALLEL`), its status leaves
and its run-fragment nodes. `PARALLEL` succeeds when every child succeeds.
For any other function id in its function index, the tree makes a module
call through `call_bridge.arora_call`.

The arguments of a node are expressions, evaluated each tick:
- a literal
- a variable bound to a store key
- the slot of another node (`NodeArgument`)
- a selection
- a nested `CallExpression { module, function, arguments }`.

The mutable parameters of a module function are bound to variables. Thus the
tree holds the state that the function needs between calls. A
runner, which is a `PARALLEL` root, holds the main behavior and one fragment
for each spawned run.

**The node graph** comes from Vizij. In vizij-rs, `vizij-graph-core` is the
engine, and `vizij-arora-behavior` holds its interpreter, `ProcessingGraph`.
- Each tick, the interpreter reads every subscribed input path from the
  store. It evaluates every node in topological order. Then it writes the
  outputs in one batch. It reports `Running` forever.
- Its root is a `layers` runner. The runner holds the main behavior as one
  `flow`, then one `flow` for each run, in spawn order. A later run writes
  after an earlier run
  ([vizij-rs#179](https://github.com/vizij-ai/vizij-rs/pull/179)).
- The main behavior of a face composes its sources in precedence order:
  1. its own graphs
  2. the mappings: ROS4HRI, the Studio-key mapping when the face serves
     Studio, and any mappings that the face embeds
  3. the animations.

  The composition makes one spec, which is one `flow`. In that `flow`, when
  two sources write one key, the write of the later source wins.
- An `ExternalFunction` node calls a module function by id, through the same
  call bridge.
- **Skills are node-graph fragments**: `look_at`, `play_viseme` and `say`
  (vizij-rs `vizij-arora-host`). A fragment computes its own lifecycle as
  data and writes a `Status` value to `task/status`:
  - `look_at` latches its start time through the store and selects with a
    `case` over its policy.
  - The viseme players select on whether the run has ended.
  - `say` hosts the call to the provider in a `TaskRun` node.

**The stepping run of Studio.** Studio delivers to every device, emulated or
real, a behavior that steps the animation module each tick. The behavior
writes its outputs under keys, beside the behavior of the device itself. The
stepping run needs four additions to arora-sdk. These additions proceed ahead
of this proposal:
- `spawn_behavior(graph, policy)`: a graph as a task run.
- two built-in tree nodes, `Equal(a, b)` and `WriteKeys(keys, values)`. The
  key table of `WriteKeys` is a literal.
- the functions of the interpreter module in `DescribeMethods`.
- `--groot` in the runner, resolving module leaves through `with_groot`.

[Section 7](#7-how-studios-additions-fit) maps each addition onto this design.

## 2. The cases the design holds

1. **A robot** runs its own behavior tree. Studio spawns its stepping run
   beside that tree.
2. **A Vizij face** runs its composed behavior, which is a `layers` of
   reactive nodes. Skill runs and programs are spawned and halted beside it.
3. **A robot with a face** runs both as branches of one host. One tree is for
   the robot, one tree is for the face, and runs of either start beside them.
4. **A skill that decides and computes.** The control of a tree states the
   lifecycle. Reactive nodes in the tree compute what the skill drives, while
   their branch runs.
5. **One module function, two placements.** A tree leaf in one behavior calls
   a third-party function. A node graph in another behavior calls the same
   function.
6. **A graph written for an older format** runs beside a graph written for a
   newer format. Each runs on the component that reads its format.
7. **Two runs that drive one key.** The host refuses the second run when it
   spawns, or its write fails. The two runs do not write in turn.

## 3. Design

### 3.1 One language: the behavior tree

- **Every graph that a device runs is a behavior tree.** A node graph is a
  subtree with a `reactive` node at its root. The children of that node are
  compute nodes. The links between those children, with a port source, are
  the edges of the graph.
- **The reactive node is a composite, as `SEQ` is.** The executor of the tree
  owns its semantics. No second interpreter runs it. No leaf bridges two
  languages.
- **The children have a stored order, which is not the evaluation order.**
  The executor evaluates upstream nodes first. The stored order breaks ties.
- **The position of a node sets its kind.** Under a reactive node, a node is
  a compute node. In all other positions, a node is a tree node. The
  component refuses a control node or a leaf under a reactive node. It also
  refuses a compute node in any other position.

| Composite | Its children | Each tick | Status |
|---|---|---|---|
| `SEQ`, `FALLBACK`, `PARALLEL`, … | sub-behaviors | ticked as the control decides, by status | from its children |
| `layers` | sub-behaviors, in precedence order | every child that has not ended, in order. When two children write one key, the later child wins. | see below |
| `reactive` | compute nodes | each node that must run, upstream first | `Running` while ticked |

**`layers` ends when every child has ended.**
- A child that ends with `Success` or `Failure` does not tick again. Its last
  writes stay. The other children continue.
- `layers` is `Running` while any child is `Running`.
- When every child has ended, `layers` is `Failure` if any child failed.
  Otherwise, it is `Success`.
- When one child fails, the composite does not fail, and it does not halt
  the other children. `layers` states precedence, not control.
- A halt of `layers` halts every child that still runs.

Thus a `layers` of reactive nodes runs until a halt stops it, as the standing
behavior of a face does.

`PARALLEL` gets a new option, `succeed_on: any`. With it, `PARALLEL` succeeds
when one child succeeds, and it halts the other children. A tree uses it to
run a reactive node for as long as another child runs. An example is a
controller that runs until a settle time ends (3.10).

### 3.2 How a reactive node runs

**Each tick, a compute node runs when one of these conditions is true:**
- It has not run since its reactive node started.
- A value that it reads is different from the value that it read last tick.
  The value can be a literal, a store key or the output of an upstream node.
  The executor compares values. It does not only ask whether an upstream node
  changed. Thus it also catches an edit: a new literal, a rewired or removed
  link, or a different key.
- It reads the clock (`time`), which moves every tick.
- It is stateful and has not settled, for example a `damp` that still eases.
- It is an `apply` whose `when` is `every_tick` (3.5).

**A result that does not change stops the change.** Sometimes the output of
a node is equal to its output last tick. Then its downstream nodes stay as
they were, unless something else that they read changed.

**A node reads and writes when it runs.** An `input` node reads its key from
the store when it runs, in tree order. Any tree node reads a variable in the
same way. An `output` node writes its key when its value changes.

Inside a reactive node, the value of an edge is always the value of this
tick, because the writer runs before the reader. In all other positions, the
tick order decides. Such a reader can be in another reactive node of the same
tree, in a tree node or in another branch. If it runs after the writer, it
reads the value of this tick. If it runs before the writer, it reads the
value of the previous tick (3.4, 3.7).

**Lifecycle.**
- A reactive node is `Running` while its parent ticks it. It never succeeds
  by itself. Its parent decides when it stops. The parent moves to another
  child, or a `PARALLEL` resolves, or a halt occurs.
- **A halt drops the state of the reactive node.** This state is the last
  value of every compute node and the integration of every stateful node.
  When its branch runs again, the reactive node starts with no state.
- A stateful node starts from its first input. Thus `damp`, `spring` and
  `slew` get an `initial` input. When `initial` reads the current value of a
  key, a restarted branch eases from that value. It does not jump.
- **An edit keeps the state of the nodes that it does not change.** A node
  keeps its id across an edit, so a spring keeps its velocity. The next tick
  runs the nodes that the edit changed.

**The links of a reactive node make no loop.** The evaluation order needs an
order with no loop. Thus the component refuses a load or an edit that makes
a loop. To feed a value back from one tick to the next tick, use a store key.
An `output` node writes the value, and an `input` node reads it next tick.

### 3.3 The compute vocabulary

The compute nodes are the generic vocabulary of the node graph, which moves
into arora-sdk (section 5). The vocabulary includes arithmetic, logic,
ranges, time, oscillators, noise, smoothing (`damp`, `spring`, `slew`),
vectors, blending and records. IK is behind the `urdf_ik` feature. The
vocabulary also includes these nodes:

| Node | Does |
|---|---|
| `input` | Reads the store key named by the predetermined key of its slot. |
| `output` | Writes its value to the store key named by the predetermined key of its slot, when the value changes. |
| `time` | Gives the device clock, `arora/time`. The tree states the time since a branch started, with `Wait`. |
| `damp`, `spring`, `slew` | Work as now. A new `initial` input sets their start state when it has a link (3.2). |
| `apply` | Calls a module function (3.5). |

The engine of the node graph evaluates a reactive node. It uses its plan,
its value types and the state of each node, keyed by node id.

### 3.4 Where a node graph ends

**Edges join compute nodes inside one reactive node.** This rule applies to
each link that has a port source and a compute node at either end. Such a
link is valid only between two children of the same reactive node. The
component refuses a load or an edit that puts such a link between two
reactive nodes.
It also refuses such a link between a child of a reactive node and a tree
node. A port link between two tree nodes keeps its `behavior-tree 1.0`
meaning: an argument that reads the slot of another node (`NodeArgument`).

**Between reactive nodes, a value passes through a store key.** Inside a
reactive node, the value of an edge has a known time. It is the value of
this tick, because the writer runs first. Between two reactive nodes, that
is not true:
- The reader can tick before the writer and read the value of last tick.
- The branch of the writer possibly does not run at all, for example when it
  is another option of a `FALLBACK`. Then its last value stays.

A named key makes both cases visible. The write analysis (3.8) already reads
keys: who writes the key, who reads it, and whether two parallel branches
write it.

**Precedence between reactive nodes is a `layers`.** A face has one
`layers`, with one reactive node for each source: its own graphs, the
mappings and the animations. When two sources write one key, the later
source wins. Today, the listed order of sources in the one `flow` of a face
gives the same result. With `layers`, the precedence is structure, and the
write analysis can read it.

A face that is already composed into one `flow` imports as one reactive node
(3.6). In that node, the component refuses two sources that write one key
(3.8).
This refusal continues until the face is composed as a `layers`
(section 10).

### 3.5 Module functions and the nodes that call them

**A module function is a function.** Its description, in
`arora_types::module::high`, has two parts: its parameters and its return
type. Each parameter has a type, a `mutable` flag and a default.

The design aim for behavior-tree node functions is that they are stateless.
A function passes the state it needs across calls in a mutable parameter,
which the caller holds
([arora-behavior-tree](../crates/arora-behavior-tree/readme.md)). This aim
is not a guarantee:
- A long-running function keeps state for each invocation in its module,
  behind a handle ([async functions](async-functions.md)).
- A host module can keep state of its own
  ([design decisions](design_decisions.md)).

**Two nodes call module functions.** Each node names its function with the
literal on its `target` slot. Each node has one slot for each parameter of
the function:

| | `call`, a tree leaf | `apply`, a compute node |
|---|---|---|
| Called | When ticked | As its `when` slot says: `on_change` calls when an argument changed, and `every_tick` calls every tick |
| Arguments | Literals or store keys | Literals, store keys, or edges from sibling compute nodes |
| Return | A `Status` return sets the status of the leaf. Any other return goes to the key bound to `result`, and the leaf succeeds. | Its `result` output |
| A mutable parameter | The caller holds it: in the key that it is bound to, or with the node while its branch runs | The same |

**What a caller can know from a description:**
- A function that returns `Status` reports progress. It is a long-running
  function, polled until it ends ([async functions](async-functions.md)).
  Only `call` can call it, because a reactive node has no status for the
  function to set.
- A function with a mutable parameter carries state from call to call. For
  such a function, `apply` uses `every_tick` by default.
- For any other function, `apply` uses `on_change` by default.

**The caller owns `when`, because a description cannot say that a function
keeps state inside its module, or that it acts on the world.** An author who knows
that a function must run every tick sets `every_tick` on the calling node.
The function stays the same.

**The calling node decides how the call occurs. The function does not
change.** To move a call from the tree into a node graph, replace a
`call` with an `apply` that has the same target and arguments.

**The tree can still read its direct encoding.** In behavior-tree 1.x, a node
whose function id is a module function reads as a `call` of that function.
Its `_ret` binding reads as `result`. An `ExternalFunction` of a node graph
reads as an `apply`.

### 3.6 Graph types and versions

- **One graph type runs: `behavior-tree`.** Its version is the version of
  the format. A minor version adds to the format, for example a new node
  kind. A major version changes what an existing graph means.
- **A graph states the oldest version that has everything it uses.** A
  component that reads `1.4` runs any `1.x` graph with `x ≤ 4`. It refuses a
  newer graph and names the version that it reads.
- **The call carries the type and the version as parameters, beside the
  graph.** The host routes on them before it decodes the graph. The host also
  checks an edit against the branch that the edit targets.
- **`node-graph` is an import format.** The host lowers a load or spawn typed
  `node-graph 1.0` to a behavior tree before it sends the graph to a
  component. (`node-graph 1.0` is the encoding that `vizij-arora-behavior`
  5.4 decodes.) The lowering maps:
  - each `flow` to a reactive node
  - a `layers` composite in the graph to a `layers` of those reactive nodes
  - each `ExternalFunction` to an `apply`.

  A graph with no structure is one `flow`. The root `layers` runner of
  `ProcessingGraph` holds runs today. Those runs are not part of a loaded
  graph. Each run is a branch of the host (3.7).
- **The lowering keeps node ids.** Thus a branch loaded as `node-graph 1.0`
  also accepts an edit typed `node-graph 1.0`. The host lowers the diff in
  the same way, node for node, and applies it as a tree edit. Such a diff can
  change only compute nodes and the links between them. The host refuses a
  diff that changes structure, and it names the change.
- **After a load, the type of each branch is `behavior-tree`.** The branch
  accepts `behavior-tree` edits, whatever its load type was. A branch loaded
  as `behavior-tree` refuses a `node-graph` edit.
- The versions:
  - `behavior-tree 1.0`: what `arora-behavior-tree` 8.1 lowers.
  - `1.1`: `Equal`, `WriteKeys`.
  - `1.2`: `reactive`, the compute vocabulary, `layers`, the `succeed_on`
    option of `PARALLEL`.
  - `1.3`: `call`, `apply`.

### 3.7 The behavior host and its branches

The behavior interface of a device (the **behavior host**) runs branches. It
owns what no single graph should own: their order, their lifecycles, the run
policies and the write tracking.

**Components serve versions of the format.**
- The build makes the host from interpreter components. Each component has a
  name that is
  unique in the host. Each component declares the `behavior-tree` majors
  that it reads.
- Two components that serve one major cause a build error.
- A device built with one interpreter has one component, named `device`.
- The host is in `arora-behavior`, so the native runtime and `arora-web`
  share it. The runtime registers the host as one module under
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

`BranchScope` carries what a run binds: its key prefix and its arguments.
The component maps the `task/` placeholder keys that a graph names onto the
key prefix.

**A branch is a loaded graph or a run.**
- A loaded graph is the standing behavior of the device. The host ticks it
  every step until an unload, whatever status it reports.
- A run starts from a spawn. It ends in one of three ways:
  - a halt
  - a terminal status of its root
  - a terminal `Status` that the run writes to its own `task/status` key
- The third way is how a run reports that it is done when its root never
  ends. Such a root can be a reactive node, a `layers` of reactive nodes or
  an imported node-graph fragment.
- A spawn runs a graph (`spawn_graph`), a skill (3.10), or a module function
  that returns `Status`. The host calls such a function each tick until it
  ends.

**Order.** Each step, the host ticks every branch one time. It ticks the
loaded graphs in load order, then the runs in spawn order. A branch added
during a tick runs from the next tick. Two branches do not write one key
(3.8). Thus the order decides only one thing. It decides whether a reader
sees the write of another branch from this tick or from the previous tick.
The order is the same each time the device runs the same branches. Here,
"parallel" means no precedence between branches. It does not mean threads.

**Halting.**
- `halt` asks the component to stop the branch. A tree halts its running
  nodes, which drops the state of its reactive nodes.
- A halted run ends with `Failure` on its status key.
- A halt writes nothing back. To return a key to rest is a step for the
  client.
- `unload_graph` halts a loaded graph in the same way.

**Run policies:**
- **`Concurrent`**: the run joins the other branches. The host refuses the
  spawn if the run writes a key that any branch writes.
- **`Replace`**: before the first tick of the new run, the host halts each
  run that writes a key that the new run writes. The host still refuses the
  spawn if the new run writes a key that a loaded graph writes. A new viseme
  replaces the previous viseme in this way.
- **`Overlap`**: the run joins even when it writes keys that other branches
  write. When two branches write one key, the later branch in tick order
  wins. This policy serves runs whose precedence over a loaded graph is not
  yet written as a `layers`. An example is the programs and skills of a face
  over its animations.
- A skill states the policy that the host uses when the caller gives none.

**A failure stays in its branch.** A `tick` error is transient. It is raised
on the behavior error of the device. The host drops a run that
ends with `Done(Err)`. The host unloads a loaded graph that ends with
`Done(Err)`, and the error names the graph.

### 3.8 Two parallel branches never write one key

**An output is a store key that a branch writes.** The runtime owns the keys
under `arora/`. These keys are the clock, and the keys of each run under
`arora/tasks/…/<run>/`. Only that run writes its own keys.

**The component analyzes the write set from the graph.** No declaration
beside the graph gives the write set. The set contains:
- the key of each `output` node
- the `result` key of each `call`, and the key bound to any mutable
  parameter of its target (read and written)
- the keys bound to the mutable parameters of each `apply`
- the key table of `WriteKeys`, which is a literal so that the analysis can
  read it.

The host keeps the set with the branch. On every edit, the host tells the
component to analyze the graph again, before the edit applies. Some keys come
only from data, for example the keys that the records of an `output` carry.
These keys are not in the set. The host finds them at the write.

**At load, spawn and edit, the host compares write sets.** If the analyzed
set overlaps the set or the claims of another branch, the host refuses the
call. The refusal names the key and the other branch. The policy of a run
can allow the overlap (3.7):
- Under `Replace`, the host halts the overlapping runs first.
- Under `Overlap`, the run joins whatever it overlaps.

**At the write, claims apply.** Each branch writes through a view
of the store. The first write of a key that no analyzed set names claims the
key for that branch, until the branch ends. A write to a key that another
branch holds does not apply, and it fails:
- A tree node that writes, such as `WriteKeys` or a `call`, returns
  `Failure`. The tree reacts as its controls say.
- An `output` node in a reactive node does not write. The tick reports the
  error on the behavior error of the device. The other writes of the
  reactive node apply.

**Inside one graph, the component checks the parallel parts.** It does this
check when the graph loads and when an edit occurs. It refuses the graph in
these cases:
- The children of a `PARALLEL` write one key.
- Two `output` nodes in one reactive node write one key.

Ordered parts are not parallel. The children of a `SEQ` write one after the
other. In a `layers`, the later child has precedence by design.

### 3.9 The functions of the behavior module

Each function has a description. Thus `DescribeMethods` lists each function
by name. The Studio Bridge, the WebSocket bridge and the ROS 2 bridge reach
each function with no new wire op.

| Function | Arguments | Returns | Effect |
|---|---|---|---|
| `load_graph` | `name: string`, `graph_type: string`, `graph_version: string`, `graph` | unit | Loads `graph` as the branch `name`. If a graph is already loaded under that name, `graph` replaces it in its position. |
| `unload_graph` | `name: string` | unit | Halts the branch `name` and removes it. |
| `edit_graph` | `branch: string`, `graph_type: string`, `graph_version: string`, `diff` | unit | Edits a branch: a loaded graph by its name, or a run by its task id. |
| `spawn_graph` | `graph_type: string`, `graph_version: string`, `graph`, `policy: RunPolicy` | `TaskHandle` | Starts `graph` as a task run. |
| `spawn` | `call: Call`, `policy: RunPolicy` | `TaskHandle` | Starts a module function as a task run (3.7). |
| `halt` | `task: TaskId` | unit | Stops a run. Idempotent. |
| `interpreters` | — | list | The components, and the format versions that they read. |
| `branches` | — | list | What runs, in tick order, and what each branch writes. |

- The descriptions give `graph` and `diff` the dynamic record type, because
  `Graph` and `GraphDiff` have no Arora type of their own.
- `LOAD`, `EDIT`, `SPAWN` and `HALT` keep their ids. `LOAD` and `EDIT` act on
  the branch `main`.
- For each branch, `branches()` lists:
  - its name or task id
  - its parent branch
  - its component
  - its type and version
  - the policy of a run
  - the keys that it writes (analyzed)
  - the keys that it claimed.
- Over the Studio Bridge, these functions are commands, as any other
  command is.
- ROS 2 has no message type for a graph. Thus the ROS 2 bridge does not
  offer load or edit. A skill returns `Status`, so it stays a ROS action.

### 3.10 Skills are behavior trees

A skill is a method that a graph implements. It has a contract (function id,
name, signature) and a behavior tree. The host registers the skill, describes
it and spawns its graph. The parameters of the skill are the `task/` keys
that the graph reads.

The lifecycle of a skill is start, wait, end and fail. This lifecycle is
control, and the tree states control directly. What the skill drives each
tick is a reactive node in the branch that needs it:
- **`look_at`** is a `FALLBACK` over the policies. The branch of each policy
  checks `task/policy`, then runs its gaze.
  - `glance` and `reset` run their gaze under `PARALLEL(succeed_on: any)`,
    beside a `Wait` for the settle time. Thus the run succeeds when the wait
    ends.
  - `track`, `idle` and `random` run their gaze until a halt.
  - An unsupported policy writes `ENOTSUP` to `task/result` and fails.

  Each gaze is a small reactive node. It holds the target, rests or wanders.
- **`say`** runs, under `PARALLEL(succeed_on: any)`, a `call` of the `say`
  function of the provider beside the lipsync reactive node. That function
  returns `Status`. After that, a reactive node moves the lips to rest.
- **`play_viseme`** is the lipsync reactive node for one shape. The host
  spawns it under `Replace`.

The skill contracts stay where they are declared (vizij-rs
`vizij-arora-host`). The `skill::<id>` entry of a face bundle carries the
graph type and version of the skill, beside the graph.

**What changes for a branch that stops.** In one node graph, the gaze of
every policy runs every tick, whether the policy is selected or not. Thus a
smoothed value has history when its policy becomes selected. In the tree,
the tree halts a branch that is not selected. That branch costs nothing and
keeps nothing. Its smoothing starts with no history when the policy becomes
selected. Where this difference is visible, the `initial` input of a
smoothing node starts it from the current value of the key.

## 4. Authoring: one stored graph, several views

The device stores and runs the tree. Each view shows the tree. Each edit in
a view is an edit of the tree.

- **The tree.** Authors edit control nodes and leaves here. A reactive node
  shows as one block. The block shows the keys that the node reads and
  writes, from the write analysis.
- **The node graph.** A reactive node opens as its network. Its children are
  the nodes of the graph, and its links are the edges of the graph. Authors
  edit it in place:

  | In the node graph | In the stored tree (`GraphDiff`) |
  |---|---|
  | Add a node | Add a compute node, and append it to the `children` of the reactive node |
  | Connect an output to an input | Add a link with a port source. It replaces the link of the input. |
  | Disconnect | Remove the link |
  | Set a value on an input | Make the link of the input a literal |
  | Change the key of an `input` or `output` node | Set the predetermined key of its slot |
  | Delete a node | Remove it from `children`, with its links |

  The view refuses what 3.1, 3.2, 3.4 and 3.5 refuse:
  - a control node inside a reactive node
  - a loop
  - an edge to another reactive node
  - an `apply` of a function that returns `Status`.
- **The reactive node as stored.** This view draws the reactive node as the
  tree view draws any tree node. The reactive node is above its ordered
  children. The links between children are drawn apart from the edges of
  the tree.
- **The overview.** It shows every reactive node, and every tree node that
  touches the store. They are joined through the keys that they read and
  write. Each node shows the conditions under which it runs.

**Presentation is not part of the graph.** The authoring tool keeps the
positions for each view, and the labels, beside the graph. They are never in
the `Graph` that a device runs. The tool lays out a node that has no
position. Thus a graph authored as a tree opens as a node graph, and the
reverse, with nothing more stored.

## 5. The node graph moves into arora-sdk

The default device runs node graphs with no Vizij crate. Thus the engine,
its generic node vocabulary and its importer move into arora-sdk. arora-sdk
stays free of Vizij.

### 5.1 What moves, what stays

| vizij-rs | In arora-sdk | Stays in vizij-rs |
|---|---|---|
| `vizij-graph-core` 2.4 | **`arora-node-graph`**: the engine, the plan, the node vocabulary (arithmetic, logic, time, smoothing, vectors, noise, blending, records, IK behind the `urdf_ik` feature), the external-function seam. `arora-behavior-tree` uses it to evaluate its `reactive` node. | — |
| `vizij-api-core` 2.0 | **`arora-shape`**: shapes, typed paths, the value vocabulary (vec2–4, quat, color, transform), blend, coercion, write batches, the graph-spec JSON forms | — |
| `vizij-arora-behavior` 5.4 | **`arora-node-graph-import`**: the `node-graph 1.0` codec, and its lowering to a behavior tree (3.6) | the registrations of `look_at`, `play_viseme` and `say`, and the rig prefix of the controls of a skill. They move into `vizij-arora-host`. |
| `vizij-graph-wasm`, `vizij-graph-registry-export` | — | the wasm bindings and registry export of the editor, over `arora-node-graph` |
| `vizij-arora-host` | — | the composition of the face, the standard and ROS4HRI mappings, the profiles, the skills |
| `vizij-animation-core` | — | as is, over `arora-shape` |

- **No Vizij node kind goes into `arora-node-graph`.** What is specific to
  Vizij is content: the sources of the face, the mappings and the skills. A
  function that only Vizij needs is a function of a Vizij module. An `apply`
  calls it.
- **Wire ids stay.** Saved graphs, module headers and the introspection of
  Studio carry the type ids and field ids of the value vocabulary, and the
  node type names. Thus these ids keep their values. The Rust names lose
  their `Vizij` prefix (`VizijKind` becomes `ShapeKind`).
- **The npm packages keep their names** (`@vizij/node-graph-wasm`). vizij-rs
  builds them over the arora crates.

### 5.2 The procedure

The history moves with `git filter-repo`, which rewrites the paths of every
commit. Thus `git log` and `git blame` on the new paths reach back to the
first commit with no `--follow`. The three crates never moved within
vizij-rs, so one path for each crate covers its history. The fixtures moved
one time, from the tests of the wasm package. Thus the procedure keeps both
of their paths.

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

3. **Make ordinary commits on the same branch.** Thus the merge itself changes
   nothing that it imports. The commits:
   - rename the packages, add them to the workspace, and make their
     dependencies path dependencies on the crates of arora-sdk
   - move the skill registrations (`gaze`, `speech`, `viseme`) out, and
     remove the `vizij-arora-host` dependency
   - point the tests, the benchmark and the README at the moved fixtures, in
     place of `vizij-test-fixtures`
   - fix the relative links of the moved READMEs, which the Markdown link
     check reads.
4. **Land the pull request as a merge commit.** A squash drops the imported
   history. A rebase replays it as new commits.
5. **Publish the first version of each new crate by hand**, `1.0.0`. Then
   register the trusted publisher of each crate. The release workflow then
   publishes every later version.
6. **In vizij-rs**, make the crates depend on the arora crates. Move the
   skill registrations into `vizij-arora-host`, and remove the three crates.
   Their last published versions stay on crates.io.

The license of the vizij-rs crates is "MIT OR Apache-2.0". This license
allows them under the MIT license of arora-sdk.

## 6. Versions and compatibility

| Crate or surface | Change |
|---|---|
| `arora-behavior` (9.1) | Minor: `GraphType`, the ids and encoders of the typed functions, `RunPolicy::Replace` and `RunPolicy::Overlap` (the enum is non-exhaustive). Major: the component trait and the host. |
| `arora-behavior-tree` (8.1) | Minor: it declares `behavior-tree 1.x`. It adds `Equal` and `WriteKeys` (1.1), then `reactive`, `layers` and the `succeed_on` option of `PARALLEL` (1.2) over `arora-node-graph`, then `call` and `apply` (1.3). Major: the runner scaffold goes, with the component trait. |
| `arora` (12.3), `arora-web` (9.0) | Minor: the typed functions, with descriptions. The importer accepts `node-graph 1.0`. Major: the builder composes components, and `Concurrent` refuses an overlap that it accepted before. |
| `arora-node-graph`, `arora-node-graph-import`, `arora-shape` | New, `1.0.0`. |
| `vizij-graph-core`, `vizij-api-core`, `vizij-arora-behavior` | No more releases after vizij-rs depends on the arora crates. `ProcessingGraph` retires, because the graphs of a face run as reactive nodes. |
| `vizij-arora-host` (6.6) | Major: it composes a face as a `layers` with one reactive node for each source. It registers the skills as trees. |
| Studio Bridge | No new `AroraOp`. Thus `studio-bridge-msgs` and the device client do not change. `studio-bridge-studio-client` gets typed helpers, which is a minor change. |
| WebSocket and ROS 2 bridges | No change. They already list and call described functions. |
| Graph formats | `behavior-tree 1.0` (now), `1.1`, `1.2`, `1.3` (3.6). The host reads and lowers `node-graph 1.0`. It never runs `node-graph 1.0` as itself. |

## 7. How Studio's additions fit

| Addition | In this design |
|---|---|
| `spawn_behavior(graph, policy)` | `spawn_graph` with `graph_type = behavior-tree`. If the addition takes `graph_type` and `graph_version` from the start, the call of Studio does not change later. |
| `Equal`, `WriteKeys` | `behavior-tree 1.1` nodes. The key table of `WriteKeys` is a literal. Thus the analysis reads the keys of the stepping run from its graph. If the tree of a robot writes one of those keys, the host refuses the spawn, so the two do not fight. |
| The functions of the interpreter module in `DescribeMethods` | The first part of 3.9: every function of the behavior module has a description. |
| The runner flag `--groot` through `with_groot` | No change. The tree loads as the branch `main`. |

The Vizij `run_behavior` becomes `spawn_graph` with
`graph_type = node-graph`, which the host lowers on arrival. Its `exclusive`
fragments become spawns under `Replace`.

## 8. Stages

Each stage lands by itself. In arora-sdk, each stage adds without a break,
except stage 4. Stage 4 takes the majors that it names.

1. **Typed graphs.** The typed functions join the module, with descriptions.
   The one interpreter declares its format versions. The untyped functions
   act for it. `interpreters()` and `branches()` answer from it.
2. **The node graph moves into arora-sdk** (section 5). Behavior does not
   change. vizij-rs depends on the arora crates.
3. **The reactive node.**
   - `reactive`, `layers` and the compute vocabulary in
     `arora-behavior-tree` (`behavior-tree 1.2`), change-driven over
     `arora-node-graph`.
   - The `succeed_on` option of `PARALLEL`.
   - The `node-graph 1.0` importer.

   The graphs of a face run as reactive nodes. `ProcessingGraph` retires.
   vizij-arora-host composes a face as a `layers` with one reactive node for
   each source. Thus the write tracking of stage 4 finds the
   precedence of the face in its structure.
4. **The behavior host.**
   - The component trait.
   - Branches in parallel, in the order that 3.7 states.
   - Module-function runs that the host hosts.
   - Write tracking (3.8), with `Replace` and `Overlap`.

   This stage takes majors of `arora-behavior`, `arora-behavior-tree` and
   `arora`.
5. **Callers and skills.**
   - `call` and `apply` (`behavior-tree 1.3`).
   - The direct module-node encoding, read as `call`.
   - `look_at`, `play_viseme` and `say`, rewritten as trees.
   - Typed skill entries in bundles.
6. **Keys that only data names, inside one graph.** A component tracks these
   keys between the parts of one graph, as the host does between branches.

## 9. Alternatives

| Alternative | Why not |
|---|---|
| The node graph as a second interpreter beside the tree. The host composes the two, and a `run_graph` leaf lets a tree run a node graph. | One store gets two executors, two lowerings and two write analyses. A bridge leaf is necessary at each point where control meets dataflow. Precedence between a tree and a node graph comes only from tick order or `Overlap`. As a reactive node, a node graph is one composite of one language. It is analyzed and checked as part of one graph. |
| A node graph lowered into tree patterns: a change test, the computation and a write for each node | To read the node graph back out, metadata must name which tree nodes make which graph node. A tree edit can break the pattern. A reactive node holds the graph as itself. Thus the view needs only positions. |
| A node graph stored apart from the tree, as authoring metadata beside it | This gives two sources of truth. The stored node graph and the tree compiled from it can disagree. |
| A node graph drawn over the whole tree, with control as nodes that carry status as a value | Status as data is what the skill fragments of a node graph compute today. It cannot be read back into tree structure. Control stays in the tree. A node graph is a reactive node in the tree. |
| Edges between reactive nodes | Inside a reactive node, the value of an edge is the value of this tick. Between reactive nodes, the value depends on tick order and on whether the branch of the writer runs. A key makes both visible to the reader and to the write analysis. |
| A reactive node that evaluates every compute node every tick, as the node graph does today | This is correct, and it is simpler. But a settled mapping then costs a full evaluation every tick while it runs. Change detection costs one comparison for each read, and it does no work for what did not change. |
| A module function placed as the node itself, with the parent deciding how the call occurs | No place can say how to call a function (every tick or on change) when its description does not say. Also, the return binding of a tree leaf (`_ret`) looks like a parameter but is not one. A caller node holds both, and the function stays a function. |
| The type and version as fields of `Graph` | The host routes and checks the version before it decodes the graph. A diff has no place for these fields. |
| The last writer wins between branches, in a declared order | Two branches that drive one key are almost always a mistake. An order decides only which write the device silently loses. A `layers` states intended precedence. `Overlap` is the explicit exception. |
| The node graph left in vizij-rs | The default device then depends on Vizij. A robot or the browser runtime cannot run node graphs with stock Arora. |
| The code copied into arora-sdk without its history, or with `git subtree add` | A copy loses `blame` and the reasons in the commit messages. `git subtree add` keeps the commits at their vizij-rs paths. Then `git log` on the new paths stops at the merge. |

## 10. Open details

The design above holds without these details. Each detail is to be
investigated before the stage that it touches.

- **The cost of change detection.** It costs one comparison for each input
  read, each tick. The alternative is an evaluation of every node. Measure
  both on the composed graph of a face and on the stepping run of Studio,
  before stage 3 lands.
- **Effects that a description does not state.** With `when` on `apply`, an
  author can call a function every tick. It is open whether the description
  of a module function should also declare this, with a flag beside `mutable`.
- **`TaskRun` and `Spawn` nodes.** They state control as dataflow. It is open
  whether the importer lowers them to tree structure. The other option is
  that the importer refuses a graph that uses them, until its control is
  rewritten.
- **Faces composed before `layers`.** A face saved as one composed `flow`
  imports as one reactive node. The check inside one graph refuses its
  sources when two write one key (3.8). It is open whether the importer
  splits a composed face back into its sources. The other option is that the
  check allows the overlap for an imported graph.
- **Retiring `Overlap`.** After a `layers` states the precedence of a face
  over its programs and skills, `Overlap` has no user.
- **Names**: the crates, the `reactive` and `layers` composites, the `call`
  and `apply` nodes, and the host.
- **Notice of a change of branches**: a key that the host writes when a
  branch starts or ends, beside `branches()`.
- **The untyped functions** (`LOAD`, `EDIT`, and `SPAWN_BEHAVIOR` after it
  lands): the major until which they stay.
- **The keys of the HAL.** A HAL can come to describe the keys that it
  writes ([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)). When it
  does, it is open whether a write by a branch to one of those keys fails in
  the same way.
