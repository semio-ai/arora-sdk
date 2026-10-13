# Proposal: one behavior tree, with node graphs as a shape of the tree

Status: proof of concept, for review.
Date: 2026-10-12.

A device runs two kinds of behavior. Some behaviors decide what starts, runs
and ends, such as actions and skills. Other behaviors turn values into values
every tick, such as rigs, mappings and controllers. People author the first
kind as behavior trees and the second kind as node graphs. Both mental models
are worth supporting. Semio Studio runs its emulated devices on the same Arora
runtime as real devices. Thus a robot gets node graphs, and a face gets
behavior trees. Neither language belongs to one kind of device.

This document states how a device stores and runs both kinds as **one
language, the behavior tree**. In that language, a node graph is a subtree in
a fixed shape, made only of behavior-tree nodes. The document also states how
authors see the two kinds as two views of the same stored graph.

**This document is a proof of concept.** It shows that a device can store
and run both kinds of behavior as one behavior tree, and that authors keep
both views. Its purpose is agreement on that concept. The names, the
encoding of the shape, the change check and the
interfaces of the host are a sketch of one way to build it. The implementers
of arora-sdk settle the final design and its implementation.

In short:

- **One language: the behavior tree.** A node graph is a `PARALLEL` with one
  pattern for each node of the graph. Each pattern uses standard control
  nodes, a condition leaf and a function leaf. No node holds a graph, and no
  node runs a second language.
- **A node graph is a shape, not a node kind.** The editor opens a subtree as
  a node graph when the subtree has the shape. Nothing in the stored graph
  marks it. Node graphs are thus a subset of what trees express. A tree edit
  that leaves the shape keeps a valid tree, which no longer opens as a node
  graph.
- **Each node of a graph is a guarded call:**
  `FALLBACK[SEQ[changed?, f], SUCCEED]`. `changed?` succeeds when a value
  that `f` reads changed. Then `f` runs and writes its outputs. Otherwise the
  pattern succeeds, and `f` does not run. The parent ticks the node graph
  every tick. A tick with no change costs the checks and nothing more.
- **Edges are local variables.** `f` writes each output to a variable that
  is local to the tree, as a call writes a mutable parameter today. A node
  that reads the output reads that variable. A value passes between node
  graphs, or to the rest of the tree, through a store key.
- **One executor, with no second path.** The executor of the tree runs a
  node graph as it runs any subtree. No flag and no recognized shape selects
  another evaluator. An optimization of a node graph is structure in the
  tree, such as the `changed?` guard, which the same executor runs.
- **`reactive` is a flag for the editor only.** It sits with the
  presentation, beside the graph, and marks a subtree that the author means
  as a node graph. The executor never reads it.
- **`layers` orders node graphs by precedence.** It ticks every child in
  order. When two children write one key, the write of the later child wins.
- **A node calls a module function. The function is never a node.** `call`
  is a tree leaf that calls the function when ticked. `apply` is a node of a
  node graph that calls the function as its `when` slot says. In the tree, an
  `apply` is the function leaf of its pattern. Either node can call any
  function, quick or long-running. The caller decides when to call the
  function and what happens to its result. The caller also holds the values
  of the mutable parameters of the function.
- **All nodes share one kind of scheduling, the scheduling of the tree.** A
  node that is not done gives control back and is polled again next tick. A
  halt cancels it. A `call` returns `Running`. An `apply` keeps its last
  result until a call finishes, and only then do the nodes after it run.
- **The behavior host runs branches in parallel.** It owns their order,
  halting and run policies. It refuses two branches that write one key, and
  it reports the refusal. Its components serve versions of the behavior-tree
  format.
- **The node graph moves from vizij-rs into arora-sdk** with its git history.
  It becomes the compute vocabulary and its value types. It also becomes
  the importer that reads the Vizij node-graph format into a tree.
- **Authoring has several views of one stored graph:**
  - the tree, where each node graph is one block that expands to its patterns
  - a node graph, opened for edits
  - the same node graph, drawn as the subtree that the device stores
  - an overview of the keys that join node graphs.

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
- two built-in tree nodes, `Equal(a, b)` and `Unpack(values, out_0 …
  out_63)`. Each output of `Unpack` is an ordinary port binding to a
  variable, so no node resolves keys from values.
- the functions of the interpreter module in `DescribeMethods`.
- `--groot` in the runner, resolving module leaves through `with_groot`.

[Section 7](#7-how-studios-additions-fit) maps each addition onto this design.

## 2. The cases the design holds

1. **A robot** runs its own behavior tree. Studio spawns its stepping run
   beside that tree.
2. **A Vizij face** runs its composed behavior, which is a `layers` of node
   graphs. Skill runs and programs are spawned and halted beside it.
3. **A robot with a face** runs both as branches of one host. One tree is for
   the robot, one tree is for the face, and runs of either start beside them.
4. **A skill that decides and computes.** The control of a tree states the
   lifecycle. Node graphs in the tree compute what the skill drives, while
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
  subtree in the shape below. It uses these nodes and nothing else:
  - the control nodes `PARALLEL`, `FALLBACK` and `SEQ`, as in
    `behavior-tree 1.0`
  - the status leaves `SUCCEED` and a new `RUNNING`, which always returns
    `Running`
  - a new condition leaf, `changed?` (3.2)
  - function leaves: the compute vocabulary (3.3) and `apply` (3.5).
- **No node holds a graph.** The executor runs each node by the semantics of
  that node. No second interpreter runs a node graph. No leaf bridges two
  languages.
- **A function leaf is not new.** The tree already calls a module function
  from a node, and binds its mutable parameters to variables. The shape adds
  two things: a guard that runs the function only when its inputs change,
  and local variables that join the leaves.

The shape:

```
PARALLEL                     the node graph
  FALLBACK                   one pattern for each node of the graph,
    SEQ                        in dependency order
      changed?               watches the variables that f reads
      f                      reads its inputs from variables, and writes
                               each output to a variable
    SUCCEED                  nothing changed: nothing to do
  RUNNING                    keeps the node graph active while it is ticked
```

**A subtree reads as a node graph when all of these conditions are true:**
- It is a `PARALLEL` that succeeds when all children succeed, with a
  `RUNNING` leaf last.
- Each other child is a pattern as above, and its `f` has no children.
- Each `changed?` watches exactly the variables that its `f` reads.
- Each edge is a local variable (3.4) that exactly one `f` writes, and only
  nodes of the subtree read.
- The edges make no loop. Thus the patterns have an order in which each
  writer comes before its readers.

The check names the first condition that fails, and the editor shows the
reason. A pattern that comes before a pattern that it reads still makes a
node graph. The check flags it, because that reader sees the value of the
previous tick.

**Why `PARALLEL`.** A node graph keeps every node current every tick. Of the
three composites, only `PARALLEL` does that:

| Composite | Each tick | When a child returns `Running` |
|---|---|---|
| `SEQ` | Starts again at the first child | Stops there |
| `SEQ_STAR` | Resumes at the child that was `Running` | Stops there. Earlier children do not tick again until the sequence ends. |
| `PARALLEL` | Ticks every child, in order | Ticks the rest |

`PARALLEL` succeeds only when every child succeeds, and `RUNNING` never
succeeds. Thus a node graph is `Running` for as long as its parent ticks it.

**The shape needs the textbook `PARALLEL`, with no memory.** Parallel is one
of the three classical control nodes, beside Sequence and Fallback. It routes
each tick to every child. It succeeds when M of its N children succeed, and
it fails when N − M + 1 children fail
([Colledanchise and Ögren, *Behavior Trees in Robotics and AI*, §1.3](https://arxiv.org/abs/1709.00084)).
Arora's `PARALLEL` is that node with M = N. `succeed_on: any` gives M = 1.
Libraries differ on one point that the shape depends on: whether a child that
already succeeded is ticked again.

| Library | Parallel | A child that already succeeded |
|---|---|---|
| Textbook | N children, threshold M | Ticked every tick |
| Arora `arora-behavior-tree` | `PARALLEL`, M = N | Ticked every tick |
| [BehaviorTree.CPP](https://github.com/BehaviorTree/BehaviorTree.CPP/blob/master/src/controls/parallel_node.cpp) | `Parallel`, success and failure thresholds | Not ticked again until the Parallel ends |
| [py_trees](https://py-trees.readthedocs.io/en/devel/composites.html) | `Parallel`, with a policy: success on all, on one, or on selected children | Not ticked again while the policy is synchronised |
| [Unreal Engine](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-behavior-tree-node-reference-composites) | `Simple Parallel`: one main task and one background subtree | Not applicable |

A Parallel with memory does not tick a pattern again after it succeeds. Thus
a node in the graph would run one time and not again. The shape is therefore
defined on the textbook Parallel, which Arora has. The same split applies to
sequences. Arora's `SEQ` is the textbook Sequence (BehaviorTree.CPP's
`ReactiveSequence`). `SEQ_STAR` is the textbook Sequence with memory, an
extension that the book marks with "∗" (BehaviorTree.CPP's
`SequenceWithMemory`).

**`layers` is a new composite, for precedence between node graphs.**

| Composite | Its children | Each tick | Status |
|---|---|---|---|
| `SEQ`, `FALLBACK`, `PARALLEL`, … | sub-behaviors | ticked as the control decides, by status | from its children |
| `layers` | sub-behaviors, in precedence order | every child that has not ended, in order. When two children write one key, the later child wins. | see below |

**`layers` ends when every child has ended.**
- A child that ends with `Success` or `Failure` does not tick again. Its last
  writes stay. The other children continue.
- `layers` is `Running` while any child is `Running`.
- When every child has ended, `layers` is `Failure` if any child failed.
  Otherwise, it is `Success`.
- When one child fails, the composite does not fail, and it does not halt
  the other children. `layers` states precedence, not control.
- A halt of `layers` halts every child that still runs.

Thus a `layers` of node graphs runs until a halt stops it, as the standing
behavior of a face does.

`PARALLEL` gets a new option, `succeed_on: any`. With it, `PARALLEL` succeeds
when one child succeeds, and it halts the other children. A tree uses it to
run a node graph for as long as another child runs. An example is a
controller that runs until a settle time ends (3.10).

### 3.2 How a node graph runs

**`changed?` decides whether a node runs.** It is a condition leaf. It has
one slot for each variable that it watches, a `seen` slot and an `again`
slot. It succeeds when one of these conditions is true:
- A watched variable took a different value since `changed?` last
  succeeded. A watched variable can be an edge, a store key or the clock
  (`arora/time`).
- Its `again` slot is true. The function leaf sets `again` when it must
  run on the next tick with no change:
  - a stateful function that has not settled, for example a `damp` that
    still eases
  - an `apply` whose `when` is `every_tick` (3.5)
  - an `apply` with a call in progress, which the executor polls once each
    tick (3.5).
- It has not succeeded since its node graph started.

**How `changed?` compares.** Each variable carries a version. A write
advances the version only when the new value is different from the old
value. `changed?` keeps in `seen` the newest version that it saw. Thus each
check compares integers. The executor compares values one time, at the
write, not one time for each reader each tick.

**A result that does not change stops the change.** When `f` writes a value
equal to its last value, the version does not advance. The `changed?` leaves
downstream fail, and their nodes stay as they were, unless something else
that they read changed.

**A node waits for a first value.** An edge has no value until the node
upstream produces one, for example an `apply` before its first call
finishes. `changed?` fails while a watched edge has no value. Thus its `f`
does not run, and an `output` node in that position writes nothing.

**A node reads and writes when it runs.** An `input` node reads its key from
the store when it runs, in tree order. Any tree node reads a variable in the
same way. An `output` node writes its key when its value changes.

Inside a node graph, the value of an edge is always the value of this tick,
because the writer comes before the reader. In all other positions, the tick
order decides. Such a reader can be in another node graph of the same tree,
in a tree node or in another branch. If it runs after the writer, it reads
the value of this tick. If it runs before the writer, it reads the value of
the previous tick (3.4, 3.7).

**Lifecycle.**
- A node graph is `Running` while its parent ticks it. It never succeeds by
  itself. Its parent decides when it stops. The parent moves to another
  child, or a `PARALLEL` resolves, or a halt occurs.
- **A halt drops the state of the node graph.** This state is its local
  variables, the state of every stateful node and the `seen` of every
  `changed?`. The halt also cancels each call in progress in the node graph
  (3.5). When its branch runs again, the node graph starts with no state, and
  every node runs on its first tick.
- A stateful node starts from its first input. Thus `damp`, `spring` and
  `slew` get an `initial` input. When `initial` reads the current value of a
  key, a restarted branch eases from that value. It does not jump.
- **An edit keeps the state of the nodes that it does not change.** A node
  keeps its id across an edit, so a spring keeps its velocity. An edit that
  changes a pattern clears its `seen`. Thus that node runs on the next tick.

**A loop is not a node graph.** The tree can run one: the reader that comes
first reads the value of the previous tick. But the shape requires an order
with no loop, and the node-graph view refuses an edge that makes one. To feed
a value back from one tick to the next tick, use a store key. An `output`
node writes the value, and an `input` node reads it next tick.

### 3.3 The compute vocabulary

The compute functions are the generic vocabulary of the node graph, which
moves into arora-sdk (section 5). Each is a function leaf. It reads its inputs
from its slots, and writes each output to the variable bound to it. A
stateful function keeps its state in a variable bound to its `state` slot,
and sets `again` while it has not settled. The vocabulary includes arithmetic, logic,
ranges, time, oscillators, noise, smoothing (`damp`, `spring`, `slew`),
vectors, blending and records. IK is behind the `urdf_ik` feature. The
vocabulary also includes these nodes:

| Node | Does |
|---|---|
| `input` | Reads the store key named by the predetermined key of its slot. |
| `output` | Writes its value to the store key named by the predetermined key of its slot, when the value changes. |
| `time` | Gives the device clock, `arora/time`. The tree states the time since a branch started, with `Wait`. |
| `damp`, `spring`, `slew` | Work as now. A new `initial` input sets their start state when it has a link (3.2). |
| `apply` | Calls a module function, quick or long-running (3.5). |

**One executor.** The executor runs a node graph as it runs any subtree.
It has no second path for node graphs. No flag and no recognized shape
selects another evaluator, because two paths for one meaning must agree on
every tick. A difference between them is a bug that only one path shows.
Thus:
- **An optimization of a node graph is structure in the tree.** The
  `changed?` guard is one. A node whose inputs did not change costs one check
  and runs nothing, because the tree says so.
- **An optimization of the executor applies to every tree.** Examples are
  links resolved one time at load, and versions compared as integers. Such an
  optimization does not know about node graphs.

### 3.4 Where a node graph ends

**Edges are local variables.** A function leaf binds each output to a
variable named for the node and the slot, `.node.port`. A variable whose name
starts with `.` is local to the tree. It never reaches the store, and the
write analysis does not count it. A local variable belongs to the node that
writes it. It lives while that node is ticked, and a halt drops it. In a node
graph, exactly one leaf writes each edge, and only nodes of the same node
graph read it. A port link between two tree nodes keeps its
`behavior-tree 1.0` meaning: an argument that reads the slot of another node
(`NodeArgument`).

**Between node graphs, a value passes through a store key.** Inside a node
graph, the value of an edge has a known time. It is the value of this tick,
because the writer runs first. Between two node graphs, that is not true:
- The reader can tick before the writer and read the value of last tick.
- The branch of the writer possibly does not run at all, for example when it
  is another option of a `FALLBACK`. Then its last value stays.

A named key makes both cases visible. The write analysis (3.8) already reads
keys: who writes the key, who reads it, and whether two parallel branches
write it.

**Precedence between node graphs is a `layers`.** A face has one `layers`,
with one node graph for each source: its own graphs, the
mappings and the animations. When two sources write one key, the later
source wins. Today, the listed order of sources in the one `flow` of a face
gives the same result. With `layers`, the precedence is structure, and the
write analysis can read it.

A face that is already composed into one `flow` imports as one node graph
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

| | `call`, a tree leaf | `apply`, a node of a node graph |
|---|---|---|
| Called | When ticked | As its `when` slot says: `on_change` calls when an argument changed, and `every_tick` calls every tick |
| Arguments | Literals or store keys | Literals, store keys, or edges from other nodes of its node graph |
| A value return | Goes to the key bound to `result`, and the leaf succeeds | Its `result` output |
| A `Status` return | Sets the status of the leaf. `Running` keeps the leaf `Running`. | Its `status` output. The function reports its results through its mutable parameters. |
| A mutable parameter | The caller holds it: in the key that it is bound to, or with the node while its branch runs | The same. Each mutable parameter is also an output. |

In the tree, an `apply` is the function leaf of its pattern. Its `when`, and
a call in progress, set `again` on its `changed?` (3.2).

**What a caller can know from a description:**
- A function that returns `Status` reports progress. It is a long-running
  function, polled until it ends ([async functions](async-functions.md)).
  Either caller can call it. For such a function, `apply` uses `on_change`
  by default, and it polls a call in progress every tick.
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

**A long-running call holds its last result.** Each call has an invocation
id, under which the module keeps the progress of the call. The caller polls
the same invocation each tick until the function returns `Success` or
`Failure`:
- While a call is in progress, an `apply` keeps the outputs that the last
  finished call produced. Its outputs do not change, so the nodes after it
  do not run. When the call finishes, the outputs change, and those nodes run.
  This is forward propagation, as in an event-driven node graph.
- A `call` in progress returns `Running`. Its siblings keep running, as for
  any `Running` leaf.
- Before the first call finishes, an `apply` has no result to keep. The
  nodes after it wait (3.2).

This is the scheduling of the tree. The node graph takes it as it is, and
adds no scheduler of its own.

**Arguments that change during a call.** The `while_busy` slot of an `apply`
of a long-running function decides what occurs:
- `restart` cancels the call and starts a new call with the new arguments.
  Under steady change, no call finishes.
- `finish` lets the call end, then starts one more call with the latest
  arguments.

**A halt cancels calls.** A halt cancels each call in progress under the
halted nodes. The module then drops the progress that it keeps for the
invocation.

**Cooperative scheduling has a limit.** It handles long durations. It does
not handle a long single step. A function that does heavy work inside one
poll holds the tick for that time, whichever caller calls it. Thus a function
that a caller calls must return from each poll quickly. Longer work runs off
the tick, and the function polls for its end
([async functions](async-functions.md)).

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
  - each `flow` to a node graph
  - a `layers` composite in the graph to a `layers` of those node graphs
  - each `ExternalFunction` to an `apply`.

  A graph with no structure is one `flow`. The root `layers` runner of
  `ProcessingGraph` holds runs today. Those runs are not part of a loaded
  graph. Each run is a branch of the host (3.7).
- **The lowering keeps node ids.** Thus a branch loaded as `node-graph 1.0`
  also accepts an edit typed `node-graph 1.0`. The host lowers the diff in
  the same way, node for node, and applies it as a tree edit. Such a diff can
  change only the nodes of a node graph and its edges. The host refuses a
  diff that changes structure, and it names the change.
- **After a load, the type of each branch is `behavior-tree`.** The branch
  accepts `behavior-tree` edits, whatever its load type was. A branch loaded
  as `behavior-tree` refuses a `node-graph` edit.
- The versions:
  - `behavior-tree 1.0`: what `arora-behavior-tree` 8.1 lowers.
  - `1.1`: `Equal`, `Unpack`.
  - `1.2`: `RUNNING`, `changed?`, local variables, the compute vocabulary as
    function leaves, `layers`, and the `succeed_on` option of `PARALLEL`.
  - `1.3`: `call`, `apply`, with `when` and `while_busy`.

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
  ends. Such a root can be a node graph, a `layers` of node graphs or an
  imported node-graph fragment.
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
  nodes, which drops the state of its node graphs.
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

### 3.8 The host refuses two parallel branches that write one key

**An output is a store key that a branch writes.** The runtime owns the keys
under `arora/`. These keys are the clock, and the keys of each run under
`arora/tasks/…/<run>/`. Only that run writes its own keys.

**The component analyzes the write set from the graph.** No declaration
beside the graph gives the write set. The set contains:
- the key of each `output` node
- the `result` key of each `call`, and the key bound to any mutable
  parameter of its target (read and written)
- the keys bound to the mutable parameters of each `apply`
- the keys bound to the outputs of each `Unpack`, which are port bindings
  like any other.

The host keeps the set with the branch. On every edit, the host tells the
component to analyze the graph again, before the edit applies. Some keys come
only from data, for example the keys that the records of an `output` carry.
These keys are not in the set. The host finds them at the write.

**At load, spawn and edit, the host compares write sets.** If the analyzed
set overlaps the set or the claims of another branch, the host refuses the
call. The refusal returns an error that names the key and the other branch. The policy of a run
can allow the overlap (3.7):
- Under `Replace`, the host halts the overlapping runs first.
- Under `Overlap`, the run joins whatever it overlaps.

**At the write, claims apply.** Each branch writes through a view
of the store. The first write of a key that no analyzed set names claims the
key for that branch, until the branch ends. A write to a key that another
branch holds does not apply, and it fails:
- A tree node that writes, such as `Unpack` or a `call`, returns
  `Failure`. The tree reacts as its controls say.
- An `output` node in a node graph does not write. The tick reports the
  error on the behavior error of the device. The other writes of the node
  graph apply.

Thus each refusal and each failed write is reported. None is silent.

**Inside one graph, the component checks the parallel parts.** It does this
check when the graph loads and when an edit occurs. It refuses the graph in
these cases:
- The children of a `PARALLEL` write one key.
- Two `output` nodes in one node graph write one key.

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
tick is a node graph in the branch that needs it:
- **`look_at`** is a `FALLBACK` over the policies. The branch of each policy
  checks `task/policy`, then runs its gaze.
  - `glance` and `reset` run their gaze under `PARALLEL(succeed_on: any)`,
    beside a `Wait` for the settle time. Thus the run succeeds when the wait
    ends.
  - `track`, `idle` and `random` run their gaze until a halt.
  - An unsupported policy writes `ENOTSUP` to `task/result` and fails.

  Each gaze is a small node graph. It holds the target, rests or wanders.
- **`say`** runs, under `PARALLEL(succeed_on: any)`, a `call` of the `say`
  function of the provider beside the lipsync node graph. That function
  returns `Status`. After that, a node graph moves the lips to rest.
- **`play_viseme`** is the lipsync node graph for one shape. The host
  spawns it under `Replace`.

**Behaviors build on other behaviors.** A skill is a described method that
returns `Status`. The host serves it as a long-running function:
- A call starts a run of its graph.
- Each poll returns the status of the run.
- A cancel halts the run.

Thus a `call` leaf in one tree can call a skill that another tree
implements. The tree waits for it as for any long-running function.

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

- **The tree.** Authors edit control nodes and leaves here. Each node graph
  shows as one block. The block shows the keys that the node graph reads and
  writes, from the write analysis. It also shows how many of its nodes ran on
  the last tick. A toggle expands the block into its patterns, and each pattern into
  its nodes, for edits as a tree. A tree edit that leaves the shape is valid.
  The subtree then shows as a plain subtree, with the reason from the shape
  check.
- **The node graph.** A node graph opens as its network. Its patterns are the
  nodes of the graph, and its edge variables are the edges. Authors edit it
  in place:

  | In the node graph | In the stored tree (`GraphDiff`) |
  |---|---|
  | Add a node | Add a pattern for the function, in dependency order, before the `RUNNING` leaf |
  | Connect an output to an input | Bind the input to the edge variable of the output, and add that variable to the watches of its `changed?` |
  | Disconnect | Make the input a literal, and remove the watch |
  | Set a value on an input | Make the link of the input a literal |
  | Change the key of an `input` or `output` node | Set the predetermined key of its slot |
  | Delete a node | Remove its pattern and its edge variables |

  The view refuses what 3.1, 3.2 and 3.4 refuse:
  - a control node inside a node graph
  - a loop
  - an edge to another node graph
- **The node graph as stored.** This view draws the node graph as the tree
  view draws any subtree, scoped to the node graph. Each pattern shows as one
  card that names its call, and expands to its nodes.
- **The overview.** It shows every node graph, and every tree node that
  touches the store. They are joined through the keys that they read and
  write. Each node shows the conditions under which it runs.

**Presentation is not part of the graph.** The authoring tool keeps the
positions for each view, and the labels, beside the graph. They are never in
the `Graph` that a device runs. The tool lays out a node that has no
position. The shape check needs no stored marker. Thus a graph authored as a
tree opens as a node graph, and the reverse, with nothing more stored.

**The `reactive` flag is presentation.** The authoring tool keeps it with the
positions, keyed by the id of a `PARALLEL`. It records that the author means
the subtree as a node graph. A flagged subtree shows as a node-graph block.
When a tree edit takes it out of the shape, the editor shows it as a node
graph that no longer reads as one, with the reason from the shape check. An
unflagged subtree in the shape also opens as a node graph. The executor never
reads the flag.

## 5. The node graph moves into arora-sdk

The default device runs node graphs with no Vizij crate. Thus the engine,
its generic node vocabulary and its importer move into arora-sdk. arora-sdk
stays free of Vizij.

### 5.1 What moves, what stays

| vizij-rs | In arora-sdk | Stays in vizij-rs |
|---|---|---|
| `vizij-graph-core` 2.4 | **`arora-node-graph`**: the engine, the plan, the node vocabulary (arithmetic, logic, time, smoothing, vectors, noise, blending, records, IK behind the `urdf_ik` feature), the external-function seam. `arora-behavior-tree` calls its node vocabulary as function leaves (3.3). | — |
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
| `arora-behavior-tree` (8.1) | Minor: it declares `behavior-tree 1.x`. It adds `Equal` and `Unpack` (1.1), then `RUNNING`, `changed?`, local variables, the compute vocabulary, `layers` and the `succeed_on` option of `PARALLEL` (1.2) over `arora-node-graph`, then `call` and `apply` (1.3). Major: the runner scaffold goes, with the component trait. |
| `arora` (12.3), `arora-web` (9.0) | Minor: the typed functions, with descriptions. The importer accepts `node-graph 1.0`. Major: the builder composes components, and `Concurrent` refuses an overlap that it accepted before. |
| `arora-node-graph`, `arora-node-graph-import`, `arora-shape` | New, `1.0.0`. |
| `vizij-graph-core`, `vizij-api-core`, `vizij-arora-behavior` | No more releases after vizij-rs depends on the arora crates. `ProcessingGraph` retires, because the graphs of a face run as node graphs of the tree. |
| `vizij-arora-host` (6.6) | Major: it composes a face as a `layers` with one node graph for each source. It registers the skills as trees. |
| Studio Bridge | No new `AroraOp`. Thus `studio-bridge-msgs` and the device client do not change. `studio-bridge-studio-client` gets typed helpers, which is a minor change. |
| WebSocket and ROS 2 bridges | No change. They already list and call described functions. |
| Graph formats | `behavior-tree 1.0` (now), `1.1`, `1.2`, `1.3` (3.6). The host reads and lowers `node-graph 1.0`. It never runs `node-graph 1.0` as itself. |

## 7. How Studio's additions fit

| Addition | In this design |
|---|---|
| `spawn_behavior(graph, policy)` | `spawn_graph` with `graph_type = behavior-tree`. If the addition takes `graph_type` and `graph_version` from the start, the call of Studio does not change later. |
| `Equal`, `Unpack` | `behavior-tree 1.1` nodes. The outputs of `Unpack` are port bindings. Thus the analysis reads the keys of the stepping run from its graph. If the tree of a robot writes one of those keys, the host refuses the spawn, so the two do not fight. |
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
3. **Node graphs in the tree.**
   - `RUNNING`, `changed?`, local variables and the compute vocabulary as
     function leaves, in `arora-behavior-tree` (`behavior-tree 1.2`).
   - `layers`, and the `succeed_on` option of `PARALLEL`.
   - The `node-graph 1.0` importer.

   The graphs of a face run as node graphs of the tree. `ProcessingGraph`
   retires. vizij-arora-host composes a face as a `layers` with one node
   graph for each source. Thus the write tracking of stage 4 finds the
   precedence of the face in its structure.
4. **The behavior host.**
   - The component trait.
   - Branches in parallel, in the order that 3.7 states.
   - Module-function runs that the host hosts.
   - Write tracking (3.8), with `Replace` and `Overlap`.

   This stage takes majors of `arora-behavior`, `arora-behavior-tree` and
   `arora`.
5. **Callers and skills.**
   - `call` and `apply` (`behavior-tree 1.3`), each able to poll a
     long-running function, with invocation ids, `while_busy` and
     cancellation on halt.
   - The direct module-node encoding, read as `call`.
   - `look_at`, `play_viseme` and `say`, rewritten as trees.
   - Typed skill entries in bundles.
6. **Keys that only data names, inside one graph.** A component tracks these
   keys between the parts of one graph, as the host does between branches.

## 9. Alternatives

| Alternative | Why not |
|---|---|
| The node graph as a second interpreter beside the tree. The host composes the two, and a `run_graph` leaf lets a tree run a node graph. | One store gets two executors, two lowerings and two write analyses. A bridge leaf is necessary at each point where control meets dataflow. Precedence between a tree and a node graph comes only from tick order or `Overlap`. As a shape of the tree, a node graph is part of one graph. It is analyzed and checked with the rest of the graph. |
| A `reactive` operator with semantics of its own, as the container of a node graph | Its children then follow rules that no other node follows. Thus the executor interprets two languages, one inside the other. The shape uses only tree nodes. Thus the semantics of the tree alone define what a node graph does. |
| A flag, or a recognized shape, that sends a node graph to a faster evaluator with the same result | Two paths for one meaning must agree on every tick, and a difference is a bug that only one path shows. It is a second executor under another name. Speed comes from the structure of the tree, which one executor runs. |
| Function nodes mixed into the tree with no container, triggered by a rule of the executor when a key that they read is set | This also gives one language. But the trigger is a rule that the tree does not show, and the executor must find the nodes that a write affects. In the shape, the trigger is a `changed?` leaf in the tree, and the container is a standard `PARALLEL`. |
| Metadata that marks which tree nodes make a node graph | A tree edit can make the metadata disagree with the tree. The shape check reads the tree itself. When a subtree no longer reads as a node graph, the check says why. |
| A node graph stored apart from the tree, as authoring metadata beside it | This gives two sources of truth. The stored node graph and the tree compiled from it can disagree. |
| A node graph drawn over the whole tree, with control as nodes that carry status as a value | Status as data is what the skill fragments of a node graph compute today. It cannot be read back into tree structure. Control stays in the tree. A node graph is a subtree of the tree. |
| Edges between node graphs | Inside a node graph, the value of an edge is the value of this tick. Between node graphs, the value depends on tick order and on whether the branch of the writer runs. A key makes both visible to the reader and to the write analysis. |
| A `SEQ` or `SEQ_STAR` as the container of a node graph | Both stop at a child that is `Running`. Thus one long-running call holds up every node after it. `PARALLEL` ticks every child every tick. |
| A node graph that evaluates every node every tick, as the node graph does today | This is correct, and it is simpler. But a settled mapping then costs a full evaluation every tick while it runs. With `changed?`, a settled node costs one integer comparison for each variable that it watches. |
| A module function placed as the node itself, with the parent deciding how the call occurs | No place can say how to call a function (every tick or on change) when its description does not say. Also, the return binding of a tree leaf (`_ret`) looks like a parameter but is not one. A caller node holds both, and the function stays a function. |
| Long-running functions callable only from a tree leaf | A node graph then cannot use a slow source, and a function that changes from quick to long-running breaks every `apply` of it. With one kind of scheduling, both callers poll it, and an `apply` holds its last result while it waits. |
| A pending node that makes every node after it pending, every tick, until its call finishes | A continuous behavior, such as a rig, then freezes while one input is slow. Holding the last result keeps the graph continuous. The nodes after it run only when a new result arrives. |
| The type and version as fields of `Graph` | The host routes and checks the version before it decodes the graph. A diff has no place for these fields. |
| The last writer wins between branches, in a declared order | Two branches that drive one key are almost always a mistake. An order decides only which write the device silently loses. A `layers` states intended precedence. `Overlap` is the explicit exception. |
| The node graph left in vizij-rs | The default device then depends on Vizij. A robot or the browser runtime cannot run node graphs with stock Arora. |
| The code copied into arora-sdk without its history, or with `git subtree add` | A copy loses `blame` and the reasons in the commit messages. `git subtree add` keeps the commits at their vizij-rs paths. Then `git log` on the new paths stops at the merge. |

## 10. Open details

The design above holds without these details. Each detail is to be
investigated before the stage that it touches.

- **The cost of the shape.** The shape runs the same calls as a dedicated
  node-graph evaluator. It also ticks about four tree nodes for each node of
  the graph. In a TypeScript model of this design, the shape cost 1.0 to 2.2
  times a dedicated evaluator while values changed. It cost less on a wide
  graph that had settled. These numbers do not predict the cost in Arora.
  Before stage 3 lands, measure the shape against the full evaluation of
  today, on the composed graph of a face and on the stepping run of Studio.
  If the shape is too slow, the remedy is structure or a general
  optimization of the executor (3.3), not a second path.
- **Guards over groups of nodes.** One `changed?` in front of a group of
  patterns can skip a whole settled part of a graph with one check. It is
  open whether the shape allows such a guard, and whether the editor writes
  it.
- **Effects that a description does not state.** With `when` on `apply`, an
  author can call a function every tick. It is open whether the description
  of a module function should also declare this, with a flag beside `mutable`.
- **`TaskRun` and `Spawn` nodes.** They state control as dataflow. It is open
  whether the importer lowers them to tree structure. The other option is
  that the importer refuses a graph that uses them, until its control is
  rewritten.
- **Faces composed before `layers`.** A face saved as one composed `flow`
  imports as one node graph. The check inside one graph refuses its
  sources when two write one key (3.8). It is open whether the importer
  splits a composed face back into its sources. The other option is that the
  check allows the overlap for an imported graph.
- **Invocation ids and cancellation.** A long-running call needs an id. The
  module keeps the progress of the call under that id. The host also needs a
  way to cancel a call inside a node, not only a task run. This is the question of
  [ARORA-87](https://linear.app/semio-ai/issue/ARORA-87).
- **Measuring long steps.** A poll that holds the tick shows only as a slow
  tick. Times for each call and each branch belong to the tracing that
  [ARORA-121](https://linear.app/semio-ai/issue/ARORA-121) proposes. That
  tracing names the branch and the node that cause a slow tick.
- **Outputs that update at different times.** In one node graph, a slow
  call can delay some outputs and not others. Then the outputs describe two
  different moments. It is open whether a node graph needs an option to hold
  all of its writes until no call in it is in progress.
- **Polls with no news.** The executor polls a call in progress every tick.
  It is open whether a module can signal the end of a call, so that the
  executor polls only then.
- **Retiring `Overlap`.** After a `layers` states the precedence of a face
  over its programs and skills, `Overlap` has no user.
- **Names**: the crates, `changed?`, the `reactive` flag of the editor, the `layers`
  composite, the `call` and `apply` nodes, and the host.
- **Notice of a change of branches**: a key that the host writes when a
  branch starts or ends, beside `branches()`.
- **The untyped functions** (`LOAD`, `EDIT`, and `SPAWN_BEHAVIOR` after it
  lands): the major until which they stay.
- **The keys of the HAL.** A HAL can come to describe the keys that it
  writes ([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)). When it
  does, it is open whether a write by a branch to one of those keys fails in
  the same way.
