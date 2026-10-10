# Proposal: a behavior interface composed of interpreters

Status: draft, for review.
Date: 2026-10-10.

A device runs behavior trees and node graphs side by side. They are
complementary: a behavior tree decides what starts, runs and ends — actions,
skills — and a node graph turns values into values every tick — rigs,
mappings, controllers. Semio Studio runs its emulated devices as the same
Arora runtime as real ones, so a robot gets node graphs and a face gets
behavior trees; neither language belongs to one kind of device.

This document states how a device's behavior interface is composed of several
interpreters, the way a HAL is composed of components
([HAL components](proposal-hal-components.md)), and how the graphs it runs
share the store.

In short:

- **One behavior module, several interpreter components.** Each component is
  named and declares the graph types it runs, and which versions of each.
- **A graph travels with its type and version.** Both are explicit parameters
  of load, edit and spawn. A component serves a type's major version, so two
  versions of one type can run side by side.
- **The device advertises its interpreters and what runs.** `interpreters()`
  and `branches()` are functions of the behavior module, listed by
  `DescribeMethods` and called over the Studio Bridge like any method.
- **Every loaded or spawned graph is a branch, and branches run in parallel.**
  The behavior interface owns them, whatever interpreter runs each: their
  order, their tick, halting and run policies.
- **Two parallel branches writing one key is an error.** It is found when a
  graph is loaded, spawned or edited, from what each graph declares it
  writes, and otherwise at the first write that reveals it.
- **Within one graph, the structure is the order**, as in a behavior tree.
- **The behavior tree and the node graph are the default interpreters**, both
  in arora-sdk. The node graph moves from vizij-rs with its git history and
  without anything specific to Vizij.
- **Vizij writes its skills as behavior trees**, whose leaves run node graphs
  where they compute.

---

## 1. What exists

**The runtime.**
- **One interpreter per device**, set at build
  (`AroraBuilder::with_behavior_interpreter`). The default is an empty
  behavior tree.
- **The interpreter is a module** under `interpreter_module::ID`, with four
  functions ([design decisions](design_decisions.md)):
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
links, which move values between node slots, and structure: `root` and each
node's ordered `children`. Nothing in it says which interpreter reads it.

**The behavior tree** (`arora-behavior-tree`) keeps a runner, a `PARALLEL`
root. A LOAD grafts the main behavior under it, and each SPAWN grafts a
fragment beside it: a run-status decorator over a leaf that calls the spawned
`Call`. The runner lists the runs from a map, so the order among them is not
defined.

**The node graph** is Vizij's (vizij-rs: `vizij-graph-core`, the engine;
`vizij-arora-behavior`, its interpreter `ProcessingGraph`):
- Its root is a `layers` runner: the main behavior as one `flow`, then one
  `flow` per run, in spawn order. A later run writes after an earlier one.
  The structure carries that order
  ([vizij-rs#179](https://github.com/vizij-ai/vizij-rs/pull/179)), and a run
  is lowered in place
  ([vizij-rs#176](https://github.com/vizij-ai/vizij-rs/pull/176)).
- A face's main behavior composes its sources in precedence order: its own
  graphs, the mappings (the Studio and ROS4HRI profiles), the animations.
- **`run_behavior(name, behavior)`** is a method the interpreter implements: it
  runs a node graph as a task run beside the main behavior.
- **Skills are node-graph fragments**: `look_at`, `play_viseme` and `say`
  (vizij-rs `vizij-arora-host`). A fragment computes its own lifecycle as
  data: it latches its start time through the store and computes its `Status`
  value. A fragment marked `exclusive` halts the live runs of its function
  when it is spawned.

**Studio's stepping run.** Studio delivers to every device, emulated or real,
a behavior that steps the animation module each tick and writes its outputs
under keys, beside the device's own behavior. It needs four additions to
arora-sdk, which proceed ahead of this proposal:
- `spawn_behavior(graph, policy)`: a graph as a task run;
- two built-in tree nodes, `Equal(a, b)` and `WriteKeys(keys, values)`, whose
  key table is a literal;
- the interpreter module's functions in `DescribeMethods`;
- the runner's `--groot` resolving module leaves, through `with_groot`.

[Section 6](#6-how-studios-additions-fit) maps each onto this design.

## 2. The cases the design holds

1. **A robot** runs its own behavior tree. Studio spawns its stepping run
   beside it.
2. **A Vizij face** runs its composed node graph, with skill runs and programs
   spawned and halted beside it.
3. **A robot with a face** runs both: a tree for the robot, a node graph for
   the face, and runs of either type.
4. **A skill that decides and computes**: a tree whose leaf runs a node-graph
   controller for as long as the leaf runs.
5. **Two versions of one graph type**: a node graph written for a newer format
   runs beside one written for the older format, each on the component that
   reads it.
6. **Two runs driving one key**: the second is refused, rather than both
   writing in turn.

## 3. Design

### 3.1 One behavior module, several interpreter components

- A device's behavior interface (the **behavior host**) is built from
  interpreter components, each under a name unique within it:
  `behavior-tree`, `node-graph`. The components are given at build and do not
  change afterwards. The device holds the host as its one behavior.
- **A component declares the graph types it runs**: for each, a type name and
  the newest version of the format it reads.
- **One component serves a type's major version.** Two components declaring
  the same type and major is a build error, not a precedence rule, as two HAL
  components describing one key is.
- A device built with one interpreter has one component, named `device`, as a
  HAL that is not composed has.
- The runtime registers one module for the host, under the existing
  `interpreter_module::ID`. A remote calls that one module, whatever component
  the call reaches.

The host lives in `arora-behavior`, beside the trait, so the native runtime
and `arora-web` share it. The `BehaviorInterpreter` trait becomes a
component's contract. The host does what no single interpreter should do on
its own: routing, ordering, lifecycles and write tracking.

```rust
pub struct GraphType {
    pub name: String,              // "behavior-tree", "node-graph"
    pub version: semver::Version,  // the format's version
}

pub trait BehaviorInterpreter {
    /// The graph types this component runs: per type and major, the newest
    /// version it reads.
    fn graph_types(&self) -> Vec<GraphType>;
    /// What `graph` writes, before it runs. Refuses a graph it cannot run.
    fn writes(&self, ty: &GraphType, graph: &Graph) -> Result<WriteSet, BehaviorError>;
    /// What `branch` would write once `diff` applies, without applying it.
    fn writes_after(&self, branch: BranchId, diff: &GraphDiff) -> Result<WriteSet, BehaviorError>;
    fn load(&mut self, branch: BranchId, ty: &GraphType, graph: Graph, scope: BranchScope)
        -> Result<(), BehaviorError>;
    fn edit(&mut self, branch: BranchId, ty: &GraphType, diff: GraphDiff)
        -> Result<(), BehaviorError>;
    fn tick(&mut self, branch: BranchId, ctx: &mut BehaviorContext)
        -> Result<BehaviorStatus, BehaviorError>;
    fn halt(&mut self, branch: BranchId) -> Result<(), BehaviorError>;
    fn remove(&mut self, branch: BranchId);
}
```

- A component holds several graphs, one per branch the host gives it, and
  ticks the one it is asked to. It does not know which branches are loaded
  graphs and which are runs.
- `BranchScope` carries what a run binds: its key prefix, onto which every
  component maps the `task/` placeholder keys a graph names, and its
  arguments.

### 3.2 Graph types and versions

- **The type names the language**: how a graph's nodes, links and structure
  are read. Every type uses the one `Graph` model and `GraphDiff` edits.
- **The version is the format's**, not a crate's. A minor version adds to the
  format (a new node kind); a major changes what an existing graph means.
- **A graph states the version it is written for**: the oldest version that has
  everything it uses. A component reading version `1.3` of a type runs any
  `1.x` graph with `x ≤ 3`. It refuses a `1.4` graph, naming the version it
  reads.
- **Type and version are parameters of the call, beside the graph.** The host
  routes on them before the graph is decoded, and checks an edit against the
  branch it targets: a diff of another type or major is refused. An edit can
  raise a branch's minor version up to the component's.
- The first versions are what runs now:
  - `behavior-tree 1.0`: what `arora-behavior-tree` 8.1 lowers;
  - `node-graph 1.0`: the encoding `vizij-arora-behavior` 5.1 decodes,
    `layers` and `flow` included. A graph with no structure is read as a
    `flow` of its nodes by id.

### 3.3 The behavior module's functions

Each function is described, so `DescribeMethods` lists it by name.

| Function | Arguments | Returns | Effect |
|---|---|---|---|
| `load_graph` | `name: string`, `graph_type: string`, `graph_version: string`, `graph` | unit | Loads `graph` as the branch `name`, replacing a graph already loaded under that name, in its place. |
| `unload_graph` | `name: string` | unit | Halts the branch `name` and removes it. |
| `edit_graph` | `branch: string`, `graph_type: string`, `graph_version: string`, `diff` | unit | Edits a branch: a loaded graph by its name, a run by its task id. |
| `spawn_graph` | `graph_type: string`, `graph_version: string`, `graph`, `policy: RunPolicy` | `TaskHandle` | Starts `graph` as a task run. |
| `spawn` | `call: Call`, `policy: RunPolicy` | `TaskHandle` | Starts a method as a task run (3.5). |
| `halt` | `task: TaskId` | unit | Stops a run. Idempotent. |
| `interpreters` | — | list | The components and the graph types they run (3.4). |
| `branches` | — | list | What runs, and what each writes (3.4). |

- `graph` and `diff` are described as the dynamic record type: `Graph` and
  `GraphDiff` have no Arora type of their own.
- `LOAD`, `EDIT`, `SPAWN` and `HALT` keep their ids. `LOAD` and `EDIT` act on
  the branch `main`, of the type the device's first component declares.

### 3.4 Advertisement

- **`interpreters()`** lists one record per component and graph type, keyed by
  `component`, as the HAL module's `models()` lists components:

  | Field | Type | Meaning |
  |---|---|---|
  | `component` | `string` | The component's name. |
  | `graph_type` | `string` | A type it runs. |
  | `graph_version` | `string` | The newest version of that major it reads. |

  The components do not change while the device runs, so a client asks once.
- **`branches()`** lists what runs, in tick order:

  | Field | Type | Meaning |
  |---|---|---|
  | `branch` | `string` | A loaded graph's name, or a run's task id. |
  | `parent` | `Option<string>` | The branch that started it, for a graph a tree leaf runs (3.8). |
  | `component` | `string` | The component running it. |
  | `graph_type`, `graph_version` | `string` | Its type and version. |
  | `writes` | `{ keys: [string], prefixes: [string] }` | What it declares it writes (3.6). |

- **Where they are reached:**
  - **Studio Bridge**: the existing `DescribeMethods` and `Call`; no new
    `AroraOp`. Studio's client gains typed helpers beside `retrieveModels`.
  - **WebSocket bridge**: its `ListMethods` and `Invoke`.
  - **ROS 2 bridge**: its service plane exposes a described method whose
    types ROS 2 can carry, as it does any other. A graph has no ROS message
    type, so loading and editing are not offered over ROS. A skill returns
    `Status`, so it stays a ROS action.

### 3.5 Branches run in parallel

- **A branch is a loaded graph or a run.** A loaded graph is the device's
  standing behavior: it is ticked every step until it is unloaded, whatever
  status it reports. A run starts from a spawn and ends at a terminal status
  or a halt.
- **What a spawn runs:**
  - a graph (`spawn_graph`), on the component its type and version select;
  - a skill (3.9): its graph, the same way;
  - a module function returning `Status`, run by the host itself: it calls the
    function each tick until the function returns a terminal status. The
    behavior tree's run-call leaf and the node graph's `TaskRun` node no
    longer host spawned calls.
- **Order.** Each step the host ticks every branch once: the loaded graphs in
  load order, then the runs in spawn order. A branch added during a tick runs
  from the next tick.
  - Two branches never write one key (3.6), so the order never decides which
    value a key keeps. It decides only whether a reader sees another branch's
    write of the same tick or of the previous one, and it is the same every
    time the device runs the same branches.
  - "Parallel" means no precedence between branches, not threads: the runtime
    is one thread, and the host ticks one branch at a time.
- **Halting.**
  - `halt` asks the component to stop the branch. A tree halts its running
    nodes, which can take more than one tick.
  - A halted run ends `Failure` on its status key, as a halted run does now.
  - Halting writes nothing back: returning a key to rest is the client's
    step.
  - `unload_graph` halts a loaded graph the same way.
- **Run policies:**
  - **`Concurrent`**: the run joins the others. A write overlap with any branch
    refuses the spawn.
  - **`Replace`**: the runs whose writes overlap the new run's are halted
    before its first tick. An overlap with a loaded graph still refuses the
    spawn: a run does not halt the device's own behavior. A new viseme takes
    over from the one before this way.
  - A skill states the policy it is spawned under when the caller gives none.
- **A failure stays in its branch.** A `tick` error is transient, as now, and
  is raised on the device's behavior error. A run that ends `Done(Err)` is
  dropped. A loaded graph that ends `Done(Err)` is unloaded, and the error
  names it.

### 3.6 Two parallel branches never write one key

**An output is a store key a branch writes.**
- A module call writes no key: its return reaches the store only through the
  node that writes it.
- Keys under `arora/` belong to the runtime: the clock, and each run's own
  keys under `arora/tasks/…/<run>/`, which only that run writes. They never
  overlap.

**Statically, from the graph.** Before a graph is loaded or spawned, and
before an edit applies, its component states its write set: keys, and
prefixes that cover every key below them on `/` boundaries (`prefix_covers`).
- **Behavior tree**: a variable or predetermined key an output port writes,
  and a `WriteKeys` table, which is a literal for this reason.
- **Node graph**: each `output` node's `path`. An `output` that writes the keys
  its records name (`key_field`/`value_field`, as the animations source does)
  states a `prefix` its keys stay under; one with no `prefix` is tracked
  dynamically only.
- **A graph a tree leaf runs** (3.8): its write set is part of its parent's.

The host refuses the call when the new set overlaps a set another branch
holds. The call fails naming the key and the other branch, and nothing
changes.

**Dynamically, at the write.** Each branch writes through a view of the store
the host gives it.
- The first write of a key that no write set names claims the key for that
  branch, until the branch ends.
- A write to a key another branch holds is a conflict. The write is not
  applied, and the branch that made it stops:
  - a run ends `Failure`, and its result names the key and the other branch;
  - a loaded graph is unloaded, and the device's behavior error says why.
- A key outside an `output`'s declared `prefix` is a conflict of the same
  kind.
- The cost is one lookup per written key. Studio's stepping run for 300
  tracks writes 600 keys a tick, so 600 lookups.

A graph a tree leaf runs writes under its parent's claims, so the host never
sets the two against each other. An overlap between them is within the parent
graph, which its component checks.

**Within one graph, the component checks its parallel parts** when the graph
is loaded or edited:
- a tree's `PARALLEL`: two children whose write sets overlap refuse the graph;
- a node graph's `flow`: two `output` nodes on one path refuse the graph.

Ordered parts are not parallel: a `SEQ`'s children write one after the other,
and in a `layers` the later child takes precedence by design (3.7).

### 3.7 Within a graph, the structure is the order

This is a behavior tree's rule, and the node graph follows it
([vizij-rs#179](https://github.com/vizij-ai/vizij-rs/pull/179)):

1. **A tick walks the structure from the root to the leaves.** Each composite
   runs its children as its function decides, in listed order unless it says
   otherwise.
2. **A node reads and writes the store when it runs.**
3. **The order is part of the graph.** It lives in `root` and `children`, so it
   survives encoding and decoding, and travels through load and edit. A node
   no composite holds runs after every placed node, in id order.

Each type's composites say how their children run:

| Type | Composite | Its children | Each tick |
|---|---|---|---|
| `behavior-tree` | `SEQ`, `FALLBACK`, `PARALLEL`, … | sub-behaviors with a status | ticked as the control decides, by status |
| `node-graph` | `layers` | parts of a behavior, in precedence order | each runs, in order; the later wins a key both write |
| `node-graph` | `flow` | a dataflow network | each node after every node it reads from, ties in listed order |

A face's composed node graph is one `layers` of `flow`s — its own graphs, the
mappings, the animations — so their precedence is written in the graph and
checked as one graph. Runs are not children of that `layers`: each is a branch
of its own (3.5).

### 3.8 A tree leaf runs a graph

`run_graph(graph_type, graph_version, graph)` is a built-in tree leaf, with
the graph as a literal:
- on its first tick it starts the graph as a branch, on the component the
  type selects, with the leaf's branch as its parent;
- it reports `Running` while that branch runs, and the branch's terminal
  status when it ends. A node graph ends by writing its `task/status`, or
  runs until halted;
- when the leaf is halted — its tree moves on, or its own branch is halted —
  it halts the branch it started.

Languages meet only through this leaf, the store and the host: no executor
reads another's composites.

### 3.9 Skills are behavior trees

A skill is a method implemented by a graph: its contract (function id, name,
signature) and a graph of a given type and version. The host registers it,
describes it, and spawns its graph on the component the type selects. Its
parameters are the `task/` keys the graph reads.

Vizij writes its skills as behavior trees. Their lifecycle — start, wait, end,
fail — is control, which a tree states directly. What they compute each tick
stays a node graph, run by a `run_graph` leaf:
- **`say`**: the provider's `say` call and the lipsync controller run side by
  side until the call ends; then a controller brings the lips to rest.
- **`look_at`**: a `FALLBACK` over the policies. `glance` and `reset` run the
  gaze controller for the settle time, then succeed; `track`, `idle` and
  `random` run it until halted; an unsupported policy fails with `ENOTSUP` on
  the result key.
- **`play_viseme`**: the lipsync controller for one shape, under `Replace`.

The skill contracts stay where they are declared (vizij-rs
`vizij-arora-host`). A face bundle's `skill::<id>` entry carries its graph
type and version beside the graph, and the authoring app (vizij-web
`vizij-authoring`) writes them.

## 4. The default interpreters move into arora-sdk

The default device composes the `behavior-tree` and `node-graph` components.
Both live in arora-sdk, so a robot runs node graphs and the browser runtime
offers them with no Vizij crate. arora-sdk stays free of Vizij: what moves is
the engine, its generic node vocabulary and its interpreter.

### 4.1 What moves, what stays

| vizij-rs | In arora-sdk | Stays in vizij-rs |
|---|---|---|
| `vizij-graph-core` 2.2 | **`arora-node-graph`**: the engine, the plan, the node vocabulary (arithmetic, logic, time, smoothing, vectors, noise, blending, records, IK behind the `urdf_ik` feature), the external-function seam | — |
| `vizij-api-core` 2.0 | **`arora-shape`**: shapes, typed paths, the value vocabulary (vec2–4, quat, color, transform), blend, coercion, write batches, the graph-spec JSON forms | — |
| `vizij-arora-behavior` 5.1 | **`arora-node-graph-interpreter`**: the component, its codec, the `layers` and `flow` composites | the `look_at`, `play_viseme` and `say` registrations and the rig prefix of a skill's controls, into `vizij-arora-host` |
| `vizij-graph-wasm`, `vizij-graph-registry-export` | — | the editor's wasm bindings and registry export, over `arora-node-graph` |
| `vizij-arora-host` | — | the face's composition, the standard and ROS4HRI mappings, the profiles, the skills |
| `vizij-animation-core` | — | as is, over `arora-shape` |

- **No Vizij node kind enters `arora-node-graph`.** The node vocabulary that
  moves is generic. What is specific to Vizij is content — the face's sources,
  the mappings, the skills — and a node only Vizij needs is a function of a
  Vizij module, which an `external_function` node calls.
- **Wire ids stay.** The value vocabulary's type and field ids, the composite
  function ids (`layers`, `flow`) and the node type names are carried by saved
  graphs, module headers and Studio's introspection, so they keep their
  values. The Rust names lose their `Vizij` prefix (`VizijKind` becomes
  `ShapeKind`).
- **The npm packages keep their names** (`@vizij/node-graph-wasm`), built in
  vizij-rs over the arora crates.

### 4.2 The procedure

The history moves with `git filter-repo`, which rewrites every commit's paths,
so `git log` and `git blame` on the new paths reach back to the first commit
with no `--follow`. The three crates have never moved within vizij-rs, so one
path each covers their history.

```sh
# 1. Extract, in a fresh clone: filter-repo rewrites the clone it runs in.
git clone https://github.com/vizij-ai/vizij-rs.git vizij-extract
cd vizij-extract
git filter-repo \
  --path crates/node-graph/vizij-graph-core/ \
  --path crates/api/vizij-api-core/ \
  --path crates/interop/vizij-arora-behavior/ \
  --path fixtures/node_graphs/ \
  --path-rename crates/node-graph/vizij-graph-core/:crates/arora-node-graph/ \
  --path-rename crates/api/vizij-api-core/:crates/arora-shape/ \
  --path-rename crates/interop/vizij-arora-behavior/:crates/arora-node-graph-interpreter/ \
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

## 5. Versions and compatibility

| Crate or surface | Change |
|---|---|
| `arora-behavior` (9.1) | Minor: `GraphType`, the typed functions' ids and encoders, `RunPolicy::Replace` (the enum is non-exhaustive). Major: the component trait and the host. |
| `arora-behavior-tree` (8.1) | Minor: it declares `behavior-tree 1.x`; `Equal`, `WriteKeys`, `run_graph`. Major: the runner scaffold goes, with the component trait. |
| `arora` (12.2), `arora-web` (9.0) | Minor: the typed functions, described. Major: the builder composes components, and `Concurrent` refuses an overlap it accepted before. |
| `arora-node-graph`, `arora-node-graph-interpreter`, `arora-shape` | New, `1.0.0`. |
| `vizij-graph-core`, `vizij-api-core`, `vizij-arora-behavior` | No further release once vizij-rs depends on the arora crates. |
| `vizij-arora-host` (6.2) | Major: it registers the skills, as trees. |
| Studio Bridge | No new `AroraOp`, so `studio-bridge-msgs` and the device client do not change. `studio-bridge-studio-client` gains typed helpers: a minor. No `arora-bridge` change, so no studio-bridge re-pin. |
| WebSocket and ROS 2 bridges | No change: they list and call described functions already. |
| Graph formats | `behavior-tree 1.0` (now), `1.1` (`Equal`, `WriteKeys`), `1.2` (`run_graph`); `node-graph 1.0` (now), `1.1` (an `output`'s `prefix`). |

## 6. How Studio's additions fit

| Addition | In this design |
|---|---|
| `spawn_behavior(graph, policy)` | `spawn_graph` with `graph_type = behavior-tree`. If the addition takes `graph_type` and `graph_version` from the start, checked against the one interpreter's type, Studio's call does not change later. |
| `Equal`, `WriteKeys` | `behavior-tree 1.1` nodes. `WriteKeys`' literal key table gives the stepping run a static write set: on a robot whose own tree writes one of those keys, the spawn is refused instead of the two fighting. |
| The interpreter module's functions in `DescribeMethods` | The first part of 3.3: every function of the behavior module is described. |
| The runner's `--groot` through `with_groot` | Unchanged. The tree loads as the branch `main`, of type `behavior-tree`. |

Vizij's `run_behavior` becomes `spawn_graph` with `graph_type = node-graph`,
and its `exclusive` fragments become spawns under `Replace`.

## 7. Stages

Each stage lands on its own. Stages 1, 2 and 5 are additive.

1. **Typed graphs.** The typed functions join the module and are described.
   The one interpreter declares its graph types, and the untyped functions act
   for it. `interpreters()` and `branches()` answer from it.
2. **The node graph moves into arora-sdk** (section 4). Behavior does not
   change; vizij-rs depends on the arora crates.
3. **The behavior host.** The component trait; branches in parallel, in the
   order 3.5 states; module-call runs hosted by the host; skills registered on
   the host. The default device composes `behavior-tree` and `node-graph`,
   each behind a default feature. The tree's runner scaffold and the node
   graph's `layers` runner give way to branches. Majors of `arora-behavior`,
   `arora-behavior-tree` and `arora`.
4. **Write tracking.** Write sets, the static and dynamic checks, the
   within-graph checks, `Replace`. Landed with stage 3, the two share one
   `arora` major.
5. **Skills as trees.** `run_graph`; `look_at`, `play_viseme` and `say`
   rewritten as trees; bundle skill entries typed.

## 8. Alternatives

| Alternative | Why not |
|---|---|
| One interpreter that runs every language, handing each composite to its executor | Couples every language into one crate and one lowering; cannot serve two versions of one type side by side; parallel runs stay a concern of that one interpreter. |
| One module per interpreter | A client would pick the module for each graph. One entry point routes on the type, as the one HAL module serves every component. |
| The type and version as fields of `Graph` | The host routes and checks the version before it decodes the graph, and a diff has no place for them. |
| The last writer wins between branches, in a declared order | Two branches driving one key is almost always a mistake, and an order only decides which one is lost, silently. Precedence that is meant is written inside one graph, in a `layers`. |
| Runs grafted into an interpreter's own graph | Each interpreter re-implements runs, order, halting and policies, and a run of one type cannot sit beside a graph of another. |
| The node graph left in vizij-rs, composed from there | The default device would depend on Vizij, and a robot or the browser runtime could not run node graphs with stock Arora. |
| The code copied into arora-sdk without its history | Loses `blame` and the reasons commit messages carry. |
| `git subtree add` | Keeps the commits, but at their vizij-rs paths: `git log` on the new paths stops at the merge. |
| Skills kept as node graphs | Their lifecycle is computed as data — a start time latched through the store, a `Status` value built from comparisons — where a tree states it. |

## 9. Open questions

- **A run over a loaded graph.** A face's program and skills are runs, and
  some write controls its animations also write. Under 3.6 such a spawn is
  refused. Should the face arbitrate inside its own graph (the run writes an
  input its `layers` ranks), or should a policy let a run take precedence over
  a loaded graph, ticked after it, while overlaps between runs stay refused?
- **Who stops at a dynamic conflict.** 3.6 stops the branch whose write finds
  the key held. Should a loaded graph rather keep running, with the write
  refused and reported?
- **Data-chosen keys within one graph.** Two parts of one graph writing a key
  that only data names follow the graph's order. Should the component track
  them as the host does between branches?
- **Names**: the crates (`arora-node-graph`, `arora-node-graph-interpreter`,
  `arora-shape`), the graph types (names, or ids as functions have), and the
  host.
- **Notifying a change of branches**: a key the host writes when a branch
  starts or ends, beside `branches()`.
- **The untyped functions** (`LOAD`, `EDIT`, and `SPAWN_BEHAVIOR` once it
  lands): until which major they stay.
- **The HAL's keys.** Once a HAL describes the keys it writes
  ([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)), should a branch
  writing one be refused the same way?
- **Who may load and spawn.** Any bridge client can load, spawn and halt.
  Does the bridge's access control need a rule for the behavior module's
  functions?
- **When a `flow` reads the store.** Rule 2 has a node read when it runs. The
  node graph reads every input path before it evaluates, so a `flow` sees an
  earlier sibling's write only on the next tick.
- **What an edit keeps.** A structural edit re-lowers. The tree resets its
  nodes' state, while the node graph keeps it. One rule should say what an
  edit keeps in both.
