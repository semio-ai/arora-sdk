# Proposal: an incremental tick for the behavior tree

Status: proposal, with a spike in `arora-behavior-tree`.
Date: 2026-10-22.

A behavior tree means "tick every node from the root, every tick". Arora's
executor does exactly that, so a tick costs every node on the active path,
whether or not anything those nodes read has changed. This document proposes
an executor that keeps that meaning and does only the work that can change
the result: a node whose inputs, state and status cannot have changed returns
its last status without being ticked.

It builds on the design that
[semio-ai/arora-sdk#396](https://github.com/semio-ai/arora-sdk/pull/396)
merged ([proposal-behavior-languages.md](proposal-behavior-languages.md)) and
changes nothing in it. Detached nodes keep their rules. What changes is the
cost of the attached tree, and with it the cost of writing a node graph as an
attached subtree, the shape that
[semio-ai/arora-sdk#332](https://github.com/semio-ai/arora-sdk/pull/332)
proposed.

In short:

- **A tick keeps its meaning.** Every observable result (store writes, root
  status, the calls every effectful function receives) is the same as today's
  executor gives, tick for tick. A differential test runs both on generated
  trees and compares them after every tick.
- **A clean node is not ticked.** A node is dirty when a cell it reads changed
  value, when it was `Running` last tick, when it has no cached status, or when
  it cannot be cached. Dirtiness propagates to the root. A composite reuses the
  cached status of a clean child without an engine dispatch. A settled tree
  with nothing `Running` makes no dispatch at all.
- **Cells carry versions.** A write advances a cell's version only when the
  value differs. Store-backed cells are checked at the start of a tick, since
  the HAL, bridges and other runs write them too.
- **Effects are never skipped.** A module function may act on the world, so a
  module leaf is ticked as today unless it states that it computes only from
  its arguments. A leaf that returns `Running` is polled every tick.
- **A node graph can be written in the tree.** `PARALLEL[ leaf × N, RUNNING ]`
  costs a call for each leaf whose inputs changed and a cache check for each
  of the others. The guard #332 needed around each node
  (`FALLBACK[SEQ[changed?, f], SUCCEED]`) is no longer needed: the executor
  makes the same check for every node of every tree.
- **One executor.** The spike keeps the incremental executor behind a flag so
  both run in one process for the differential test. The proposal is to make
  it the only executor, with the naive one kept as the tests' reference.

---

## 1. What exists

- **Every node is an engine callable.** A composite ticks a child through the
  engine (`arora_call_indirect`, a map lookup, then the child's tick
  function). The cost of a tick is one dispatch per node on the active path.
- **Composites are textbook and reactive.** `SEQ` starts from its first child
  every tick, `FALLBACK` stops at the first child that does not fail, and
  `PARALLEL` ticks every child. `SEQ_STAR` and `RunStatus` keep their state in
  their argument cells.
- **There is no halt.** A child that its composite stops ticking is not ticked
  again. Nothing resets it, and nothing cancels a call in progress.
- **A long-running function returns `Running`** and is called again at the
  next step, because the tree ticks again from the root
  ([async functions](async-functions.md)).
- **Cells have no versions** on `main`. A `VariableCell` holds a value, local
  or backed by a store slot. Store keys are read live.
- **Detached nodes cache by argument value.** A detached node calls its
  function only when its arguments changed. The attached tree has no such
  cache.

## 2. Design

### 2.1 What a tick means

A tick means what it means today: each composite ticks its children by its
rules, from the root. The incremental executor is an implementation of that
meaning. For any tree, at every tick, it produces the same root status, the
same store writes and the same calls to every function that is not declared
to compute only from its arguments.

### 2.2 Versions and readers

- **A cell's version advances only when its value changes.** Writing the value
  a cell already holds is not a change.
- **A store-backed cell can change outside the tree.** At the start of a tick,
  the executor compares each store-backed cell the tree reads with the value it
  last saw, and treats a difference as a write. A node also compares its own
  store-backed reads before its clean check, so a write through another handle
  to the same key is seen. A store subscription would replace both (§5).
- **Readers are known when the tree is lowered.** Each node's reads are its
  arguments' expressions at any depth, as the detached layer already computes
  them, and its own argument cells, since a mutable parameter is read as well
  as written. A return binding counts as a read (§2.7).
- **Each path has its own cache.** A node shared by two parents is lowered once
  per path; the paths share cells and keep separate statuses.

### 2.3 Dirty, clean and skipped

A node is **dirty** when:
- a cell it reads changed version since its last tick;
- it has no cached status (its first tick, or the tree was lowered again);
- its last status was `Running`;
- or it cannot be cached (§2.4).

Marking a node dirty marks its ancestors, up to the first that already is.

A composite about to tick a child first looks at the child: a **clean** child
returns its cached status, with no engine dispatch. The composite's own logic
is unchanged: it still walks its children in order, and a clean child costs a
cache check where it used to cost a dispatch and a call.

A visit clears a node's dirty mark before it runs, so a mark made later in the
same tick waits for the next one. That keeps today's order within a tick: a
reader after a writer in tree order sees the new value this tick, and a reader
before it sees it at the next tick. A node's own write does not dirty it.

### 2.4 What can be cached

| Node | Cached | Why |
|---|---|---|
| `SEQ`, `FALLBACK`, `PARALLEL` | yes | Their status follows from their children's. |
| `SEQ_STAR`, `RunStatus` | yes | Their state is in their argument cells, which they read. |
| `Succeed`, `Fail`, `Equal`, `Unpack` | yes | They compute from their arguments. |
| `Run` | yes, but always polled | It is always `Running`. |
| `IsStringSet`, `WaitStringSet`, `RegexMatch` | yes | They compute from their arguments; a `Running` result is polled. |
| `SetString`, `UnsetString` | no | They exist to write. |
| `RunCall`, `Scatter` | no | The call or the keys are data. |
| Module leaf | only with `_on_change` | A module function may act on the world, and re-ticking a finished leaf starts it again today. |
| Module composite | no | It ticks its children from inside the module. Its children's own caches still apply once it calls them. |
| A node with a call nested in an argument | no | Its nested calls run, and keep call-site state, every tick. |

A module leaf states that it computes only from its arguments with a reserved
argument, `_on_change: true`, beside `_ret`. Such a leaf is called when one of
its reads changed and keeps its last status and outputs otherwise. The
argument stays in the tree and is never sent to the module. A function's
description could carry the same fact for every caller (§5).

### 2.5 Long-running calls

A leaf whose last status was `Running` is dirty at the next tick, so it is
polled every tick, as today, and so are its ancestors. A reactive `SEQ` above
it therefore re-checks its earlier conditions every tick, and stops ticking
the leaf as soon as one fails, as today. A module that can signal the end of a
call would let the executor poll it only then (§5).

### 2.6 Detached nodes

A node's detached reads are brought up to date before its clean check, so a
detached input that changed dirties it in the same visit. A node that reads
detached nodes keeps its ancestors dirty, so it is reached, and its pulls run,
whenever today's executor would reach it. A detached node's port writes go
through the versioned write, so re-writing an unchanged output does not dirty
its readers. Sinks run after the tree, as today.

### 2.7 Outputs that another writer overwrites

A cached node that writes a cell also reads it. When anything else writes that
cell (another node, the HAL, a bridge), the writer is dirty and writes its
value again the next time it is visited, as a tick from the root would. Its
own write does not dirty it. Without this rule a settled output that something
else overwrote would stay overwritten, which no tick from the root allows.

### 2.8 Node graphs in the tree

A node graph can be an attached subtree:

```text
PARALLEL                                   ◇ node graph
  {.amount} = clamp(x: {face/smile}, min: 0, max: 1, _on_change: true)
  {.corner} = remap(x: {.amount}, …, _on_change: true)
  {rig/mouth_corner} = damp(target: {.corner}, …, _on_change: true)
  RUNNING
```

With this executor, a tick of that subtree calls the leaves whose inputs
changed, checks the cache of the others, and dispatches the `PARALLEL` and
`RUNNING`. It needs no guard per node, and nothing a tree does not already
have apart from `RUNNING`, which #332 also proposed. The editor recognizes the
shape and opens it as a node graph. Detached nodes remain available. The two
differ in when a value is computed (where the subtree sits in the tree,
against at the first read) and so in what a reader sees within a tick (§3).

## 3. Within a tick

The executor keeps today's order, so what a reader sees within a tick follows
from where the computation sits. Take a tree that writes a key and reads it
back twice, with `v` starting at 0:

```text
SEQ
  v = v + 1
  check v × 10
  v = v + 1
  check v × 10
```

| Where the value read by `check` is computed | Tick 1 reads, then `v` | Shapes |
|---|---|---|
| At the read, in tree order | 10, 20; `v` = 2 | Arora's tree today; this executor on any tree. |
| At the first read in the tick, then reused | 10, 10; `v` = 1 | Detached nodes (#396). |
| In a subtree ticked before the behavior | 0, 0; `v` = 1 | A node graph placed before the tree, as #332 placed it, on either executor. |

None of these is a defect: each states when a value is computed. Placing a
node graph's leaves just before their readers gives tree order; placing the
graph first gives every reader the values from the start of the tick.

Across ticks every shape converges, provided an output that another writer
overwrote is written again (§2.7). A long-running leaf is polled and its guard
re-checked every tick (§2.5); cancelling it when the guard fails needs a halt,
which Arora does not have (§5).

## 4. Measurements

`cargo bench -p arora-behavior-tree --bench incremental` builds a node graph
of N module functions (add, mul, clamp, remap) through a native test bridge,
with K = max(2, N/10) store inputs, each function's result published to a
store key, in three forms:

- **attached-naive**: `PARALLEL[ leaf × N, RUN ]` on today's executor;
- **attached-incremental**: the same tree with `_on_change` on each leaf, on
  the incremental executor;
- **detached**: the same calls as detached nodes whose results are sinks,
  under a root `RUN`.

Scenarios: **settled** (no input changes), **one-input** (input 0 changes
every tick), **all-inputs** (every input changes every tick). Median µs per
tick over 7 samples of 500 ticks after 50 warm-up ticks; module calls and
engine dispatches per tick. Apple M4 Pro, release profile with thin LTO.

| N | Scenario | Form | µs/tick | Calls | Dispatches |
|--:|---|---|--:|--:|--:|
| 10 | settled | attached-naive | 10.5 | 10 | 12 |
| | | attached-incremental | 0.39 | 0 | 2 |
| | | detached | 2.3 | 0 | 1 |
| 100 | settled | attached-naive | 100 | 100 | 102 |
| | | attached-incremental | 2.7 | 0 | 2 |
| | | detached | 25 | 0 | 1 |
| 1000 | settled | attached-naive | 1017 | 1000 | 1002 |
| | | attached-incremental | 32 | 0 | 2 |
| | | detached | 247 | 0 | 1 |
| 1000 | one-input | attached-naive | 1019 | 1000 | 1002 |
| | | attached-incremental | 46 | 10 | 12 |
| | | detached | 252 | 10 | 1 |
| 1000 | all-inputs | attached-naive | 1131 | 1000 | 1002 |
| | | attached-incremental | 996 | 828 | 830 |
| | | detached | 454 | 828 | 1 |

What the numbers show:

- **The incremental form makes the calls the detached form makes**, in every
  scenario, so the two do the same useful work. They differ in bookkeeping.
- **A settled incremental tick is not free.** It is about 30 ns per node at
  N = 1000: the `PARALLEL` checks every child's cache, the executor compares
  every store-backed cell at the start of the tick, and it scans every node for
  one left `Running`. Each is linear in the tree, and each has a known
  replacement (§5). A settled detached tick is about 250 ns per node, because
  every sink is brought up to date each tick: its arguments are evaluated and
  compared with its last call's, and its outputs are published again.
- **When everything changes, detached nodes are cheaper.** Each attached leaf
  still costs an engine dispatch and its bookkeeping; a detached node calls its
  function directly. The incremental form then costs about what today's
  executor costs.
- **The bridge is native.** These timings do not include wasm calls or
  hardware I/O, which add the same cost per call to every form. Compare the
  counts as well as the times.

## 5. Open details

- **Declaring purity on functions.** `_on_change` is per node. A flag in a
  function's description would say it for every caller, and would let the
  importer mark Vizij's node vocabulary once.
- **A queue of dirty children** per composite, with counts of its children's
  statuses, so a `PARALLEL` visits only the children that changed instead of
  checking every child's cache.
- **A set of running nodes** in place of scanning every node at the start of a
  tick.
- **A store subscription** in place of comparing store-backed cells at each
  tick and before each clean check.
- **Completion signals** from long-running modules, so a waiting call is not
  polled.
- **Carrying caches across an edit.** A re-lowering starts from a dirty tree,
  which costs one full tick.
- **Module composites** that tick their children from inside a guest module
  cannot be skipped: the executor sees only the composite.
- **Halting.** Arora has no halt today, and this executor needs none. Halting
  a subtree a composite stops ticking, and dropping its state, is a separate
  decision (#332 §3.2).
- **API.** `VariableCell`'s variants now hold change metadata beside the
  value. Code that built a cell from a raw `Rc<RefCell<…>>` uses
  `VariableCell::local_with` and `set` instead.
