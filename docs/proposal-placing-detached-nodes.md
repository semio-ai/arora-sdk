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
call. A mutable parameter bound to a key starts from the key's value, as a
detached one does.

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
- **A sink's key read by the tree.** Detached, a tree node sees last tick's
  value; placed, this tick's.
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
| attached-naive | 993 µs · 1000 calls | 998 µs · 1000 calls | 1103 µs · 1000 calls |
| attached-incremental | 14 µs · 0 | 42 µs · 10 | 1048 µs · 841 |
| attached-incremental-with | 11 µs · 0 | 42 µs · 10 | 1071 µs · 841 |
| detached | 218 µs · 0 | 229 µs · 10 | 434 µs · 828 |
| placed | 12 µs · 0 | 42 µs · 10 | 1059 µs · 828 |

At N = 10 and 100, settled: placed 0.27 and 1.1 µs, detached 1.9 and 22 µs.

- **Placed and detached make the same module calls** in every scenario and
  size here.
- **A settled placed group is skipped whole.** Its `PARALLEL` has no `RUN`,
  so a settled tick dispatches only the `WITH` and its body. Written as
  `WITH[ PARALLEL[ leaf × N ], RUN ]`, attached leaves cost the same: the
  saving comes from the shape, which any attached node graph can take.
- **Publishing costs dispatches, not calls.** Each placed sink adds a `Copy`,
  so when every input changes a tick makes 1755 dispatches for 828 calls.
  When one input changes, the publication group checks each of its children's
  caches, which puts placed level with the other incremental forms.
- **When every input changes, detached nodes are cheapest.** A placed node is
  an attached leaf, ticked through an engine dispatch with its `Copy`; a
  detached node is called directly and publishes from its cache.

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
- **Errors in prerequisites.** Whether an erring group should stop the body,
  or be reported and skipped, as a failing detached sink is.
- **Placing on edit.** A re-placement after an edit starts the incremental
  executor from a dirty tree, which costs one full tick.
- **Load time.** Placement walks every path of the attached tree. It is not
  measured.
