# Proposal: placing detached nodes before their readers

Status: proposal, with a spike in `arora-behavior-tree`.
Date: 2026-10-22.

A tree file states a node graph as detached nodes
([proposal-behavior-languages.md](proposal-behavior-languages.md)): named
values, such as `@wave` or `@chest`, that the tree reads where it needs them.
This document keeps that file and changes how a device runs it. When the tree
is loaded, a pass places each group of detached nodes in the tree, just before
the part of the tree that reads it, and the incremental executor
([proposal-incremental-behavior-tree.md](proposal-incremental-behavior-tree.md))
runs the result. What a person edits, and what an editor shows, does not
change; an editor can also show the tree as placed.

In short:

- **The file does not change.** Detached nodes stay listed beside the tree,
  read as `@node`. Placement is a step between loading and lowering; the spike
  exposes it as `place_detached`, which a caller applies to a node list.
- **`WITH` runs values before a body.** A new native control ticks its
  prerequisites in order, ignores their statuses, and returns its last child's
  status.
- **A group runs just before its readers.** Each connected group of used
  detached nodes becomes a `PARALLEL` of `_on_change` leaves, in data order,
  inside a `WITH` around the lowest common ancestor of the attached nodes that
  read it. A group with a sink goes before the root.
- **Placed nodes are ordinary attached leaves.** The incremental executor
  calls one only when its arguments changed. A node with a call in its
  arguments stays detached, and the detached layer runs it as today.
- **When a value is computed changes.** A placed group runs before its
  anchor's subtree, not at its first read. In the bench, where every reader's
  branch runs, placed and detached make the same calls; §3 lists where they
  differ.

---

## 1. What exists

- **Detached nodes** run when a node that reads them is about to tick: once
  per tick, at the first read, and only when their arguments changed. A sink
  (an output bound to a store key) runs after the tree at every tick, and
  publishes its key from its own port, which `@node` reads. A detached node
  has no lifecycle: a branch the tree stops ticking leaves its detached nodes
  as they are.
- **The incremental executor** ticks the attached tree with today's meaning
  and skips what cannot change: a clean node returns its cached status, and an
  `_on_change` leaf is called only when one of its arguments changed, its own
  state included.
- **There is no halt.** A subtree a composite stops ticking is not reset.

## 2. Design

### 2.1 `WITH`

`WITH[ p₁, …, pₙ, body ]` ticks `p₁ … pₙ` in order, ignoring their statuses,
then ticks `body` and returns its status. It needs at least two children. An
error in any child propagates. It is a native control, cacheable like the
others: a settled prerequisite is a clean child, skipped whole.

### 2.2 What is placed

- **A used node is placed.** A detached node runs today when the tree reads
  it, when it is a sink, or when it feeds one. Placement attaches those.
- **An unused node stays detached.** It stays in the file, so the ports other
  nodes name stay valid, and the detached layer never runs it, as today.
- **A node with a call in its arguments stays detached.** The incremental
  executor never caches such a node, since its nested calls keep call-site
  state, so placed it would be called at every tick. Detached, its main call
  is cached by argument value. Placed nodes around it read it, and are read
  by it, as attached and detached nodes read each other today.
- **A sink that shares a key with a sink staying detached stays detached.** A
  detached sink publishes after the tree and a placed one before the body, so
  every publisher of one key stays on the same side, and they keep publishing
  in file order. The rule repeats until no placed sink shares a key with a
  detached one; two variables of one name name one key.

### 2.3 Groups and anchors

The used, placeable nodes form a graph by what reads what, following reads
through any node that stays detached. Each connected component is a group. An
unused node joins no group.

A group's **anchor** is the lowest common ancestor, in the attached tree, of
every attached node that reads any node of the group, directly or through a
node that stays detached, over every path of a node shared by several parents.
Two scopes exist:

| Scope | Anchor | Use |
|---|---|---|
| `Readers` | the readers' common ancestor; the root for a group with a sink, one that feeds a detached sink, or one read at the root | a group runs only when the part of the tree that needs it runs |
| `Root` | the root, for every group | every group runs at every tick, like a node graph beside the tree |

### 2.4 Lowering

At each anchor, the anchor's parents point to a new `WITH` whose children are
one `PARALLEL` per group anchored there, in the order of each group's first
listed node, then the anchor itself. A group's `PARALLEL` holds its nodes in
data order, each with `_on_change: true` added. It has no `RUN` child: `WITH`
ignores its status, so a settled group is clean and skipped whole.

Readers keep their references: `@node` and `@node.parameter` read a placed
node's port as they read a detached one's. Every placed output gets a private
local cell, including a return value that was unbound. A sink keeps that
separation too: its node writes its own port, and a native `Copy` node
publishes the port to the key. The root's `WITH` holds one more `PARALLEL`,
after every group, with the publications in file order, so two sinks of one
key end with the value the later-listed one computed, as detached sinks do. A
`Copy` whose key another writer overwrote publishes again, with no module
call. One sink's outputs publish in detached publication's order: its return,
then its mutable parameters in signature order. A mutable parameter bound to a
key starts from the key's value, as a detached one does.

An input that another detached node's output feeds reads that output, not the
store key its argument names: loading gives the two a private cell. Once both
are placed, the input reads a fresh local variable instead, so the writer
still does not write the key.

New node and variable ids are derived from the anchor and the group, and
probe past ids the file already uses, so placing a file twice gives the same
tree.

Placement reuses the detached layer's own analysis (reads, pulls, outputs,
sinks, data order, unused nodes) and its validation, so it refuses what
detached lowering refuses, with the same errors. It also refuses attached
cycles and missing children.

### 2.5 State

A placed node is a computation: its return value is bound, so it returns
`Success` whatever its function returns. A mutable parameter it writes is an
argument to its next call, so it is called until its state settles, as a
detached node is.

### 2.6 Two views of one file

An editor shows the file as authored: the tree, and the detached nodes as a
node graph beside it. It can also show the tree as placed, with each `WITH`
and its groups where the device runs them. Neither view needs anything stored
beyond the file.

## 3. Where placed and detached differ

Placement moves when a value is computed, from its first read in a tick to the
start of its anchor's subtree, and moves sink publication from after the root
to before its body. Tests pin each of these, with the values:

- **A write before the read.** An attached node that writes a detached node's
  input inside the anchor, before the first reader: detached, the reader sees
  the new value this tick; placed, at the next tick.
- **State under a branch not taken.** A stateful node read only under a
  branch the tree does not take: detached, it is not called. Placed at the
  root, it advances every tick. Placed at its readers, it stays dormant while
  its anchor is not reached.
- **A placed sink's key read in the same tick.** Detached, a tree node, or a
  node that stays detached, sees last tick's value; placed, this tick's.
- **A sink's key written by the tree.** A sink computing 3 and a tree node
  writing 100 to one key: detached, the sink publishes last and the key ends at
  3; placed, the tree writes last and it ends at 100. Neither calls the sink's
  function again while its inputs stay the same.

These follow from the same move and are not asserted:

- **Readers in separate branches.** Reaching the anchor runs the group, even
  when the branch taken reaches none of its readers.
- **Errors.** A detached sink that fails stops no other. A placed node whose
  call errs stops its `WITH`, and the body with it. A `Failure` status does
  not, since `WITH` ignores its prerequisites' statuses.

## 4. Measurements

`cargo bench -p arora-behavior-tree --bench incremental` adds two forms to the
incremental proposal's bench: **placed** (the detached form's file, placed with
`Readers`, on the incremental executor) and **attached-incremental-with**
(`WITH[ PARALLEL[ leaf × N ], RUN ]`, each leaf `_on_change`). Every detached
node in the bench is a sink and the tree is a bare `RUN`, so every group
anchors at the root: the bench measures placed groups and publication, not
anchoring at readers. Median µs per tick and module calls per tick, Apple M4
Pro, release profile, native test bridge.

| N = 1000 | settled | one input changes | every input changes |
|---|--:|--:|--:|
| attached-naive | 975 µs · 1000 calls | 983 µs · 1000 calls | 1072 µs · 1000 calls |
| attached-incremental | 14 µs · 0 | 41 µs · 10 | 1014 µs · 841 |
| attached-incremental-with | 11 µs · 0 | 41 µs · 10 | 1029 µs · 841 |
| detached | 40 µs · 0 | 44 µs · 10 | 232 µs · 828 |
| placed | 11 µs · 0 | 41 µs · 10 | 1014 µs · 828 |

At N = 10 and 100, settled: placed 0.28 and 1.2 µs, detached 0.43 and 3.8 µs.
These are measured on main with arora-behavior-tree 9.5.0's detached
evaluator, which compares a node's arguments in place and lends them to its
call.

- **Placed and detached make the same module calls** in every scenario and
  size here.
- **Settled, a placed group costs about a quarter of detached nodes.** Its
  `PARALLEL` has no `RUN`, so a settled tick dispatches only the `WITH` and
  its body, where every detached sink is brought up to date and published.
  Written as `WITH[ PARALLEL[ leaf × N ], RUN ]`, attached leaves cost the
  same: the saving comes from the shape, which any attached node graph can
  take.
- **When one input changes, the forms cost the same**: the calls dominate.
- **When every input changes, detached nodes are about 4.4× cheaper.** A
  placed node is an attached leaf, ticked through an engine dispatch, with a
  `Copy` for each sink: 1755 dispatches for 828 calls here. A detached node is
  called directly, with its arguments lent in place.

## 5. Open details

- **Lifetime.** With a halt, a `WITH` that its parent stops ticking could drop
  its groups' state, so a branch's smoothing restarts when the branch starts
  again. Arora has no halt (§1); this placement needs none.
- **Finer groups.** A group per value read, instead of per connected
  component, places each value closer to its readers and runs less of the
  graph when one branch is taken. At the limit, it is #396's evaluation.
- **Publishing after the body.** A `WITH` form that ticks some children after
  its body would publish sinks where detached ones publish, removing the
  difference in §3 for a key the tree also writes.
- **Calls in arguments.** Hoisting a nested call into a placed leaf of its own
  would let its owner be placed and cached too.
- **An attached output feeding a detached input.** An attached node whose
  output is bound to a detached node's input that names a store key writes a
  private cell while the detached node reads the key; placed, the write
  reaches the key. Whether the detached layer should feed that input, as it
  does from a detached writer, is a question for detached lowering first.
- **Errors in prerequisites.** Whether an erring group should stop the body,
  or be reported and skipped, as a failing detached sink is.
- **Placing on edit.** A re-placement after an edit starts the incremental
  executor from a dirty tree, which costs one full tick.
- **Load time.** Placement walks every path of the attached tree. It is not
  measured.
- **Hosts that observe detached nodes.** `observe_detached` and
  `detached_outputs` serve an editor's view of every node of a graph. A node
  placement attached is no longer detached: observing it does nothing, and
  its outputs read as `None`, as a node not yet run does, while
  `argument_value` still reads its bound cells. A host that both places and
  observes needs placement to leave observed nodes detached, or those calls
  to read a placed node's cells. No host does both today: the node-graph
  importer does not place.
- **The name.** The node-graph importer already uses "placement" for where a
  call to another module goes. This pass may want another name, such as
  hoisting.
