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
  read as `@node`. Placement is a step of loading, like lowering.
- **`WITH` runs values before a body.** A new native control ticks its
  prerequisites in order, ignores their statuses, and returns its last child's
  status.
- **A group runs just before its readers.** Each connected group of detached
  nodes becomes a `PARALLEL` of `_on_change` leaves, in data order, inside a
  `WITH` around the lowest common ancestor of the attached nodes that read it.
  A group with a sink goes before the root.
- **One executor.** Placed nodes are ordinary attached leaves, so a device has
  one way to run a tree. The incremental executor calls a placed node only
  when its inputs changed: placed and detached make the same calls.
- **When a value is computed changes.** A placed group runs before its
  anchor's subtree, not at its first read. §3 lists where that shows.

---

## 1. What exists

- **Detached nodes** run when a node that reads them is about to tick: once
  per tick, at the first read, and only when their arguments changed. A sink
  (an output bound to a store key) runs after the tree at every tick. A
  detached node has no lifecycle: a branch the tree stops ticking leaves its
  detached nodes as they are.
- **The incremental executor** ticks the attached tree with today's meaning
  and skips what cannot change: a clean node returns its cached status, and an
  `_on_change` leaf is called only when one of its reads changed, its own state
  included.
- **There is no halt.** A subtree a composite stops ticking is not reset.

## 2. Design

### 2.1 `WITH`

`WITH[ p₁, …, pₙ, body ]` ticks `p₁ … pₙ` in order, ignoring their statuses,
then ticks `body` and returns its status. An error in any child propagates. It
is a native control, cacheable like the others: a settled prerequisite is a
clean child, skipped whole.

### 2.2 Groups

The detached nodes form a graph by what reads what. Each connected component
is a group. A node that nothing reads and that is not a sink, and that feeds
no sink, does not run today; placement leaves it out.

### 2.3 Anchors

A group's **anchor** is the lowest common ancestor, in the attached tree, of
every attached node that reads any node of the group, over every path of a
node shared by several parents. Two scopes exist:

| Scope | Anchor | Use |
|---|---|---|
| `Readers` | the readers' common ancestor; the root for a group with a sink or a root reader | a group runs only when the part of the tree that needs it runs |
| `Root` | the root, for every group | every group runs at every tick, like a node graph beside the tree |

### 2.4 Lowering

At each anchor, the anchor's parents point to a new `WITH` whose children are
one `PARALLEL` per group anchored there, in the order of each group's first
listed node, then the anchor itself. A group's `PARALLEL` holds its nodes in
data order, each with `_on_change: true` added. It has no `RUN` child: `WITH`
ignores its status, so a settled group is clean and costs nothing.

Readers keep their references: `@node` and `@node.parameter` read an attached
node's port as they read a detached one's. A detached node whose return value
is unbound gets an explicit local binding. New node ids are derived from the
anchor and the group, so placing a file twice gives the same tree.

Placement reuses the detached layer's own analysis (reads, pulls, sinks, data
order, unused nodes) and its validation, so it refuses what detached lowering
refuses, with the same errors.

### 2.5 State

A placed node is a computation: its return value is bound. A mutable
parameter it writes is an input to its next call, so it is called until its
state settles, as a detached node is.

### 2.6 Two views of one file

An editor shows the file as authored: the tree, and the detached nodes as a
node graph beside it. It can also show the tree as placed, with each `WITH`
and its groups where the device runs them. Neither view needs anything stored
beyond the file.

## 3. Where placed and detached differ

Placement moves when a value is computed, from the first read to the start of
the anchor's subtree. Tests pin the first three; the rest follow from the
same move and are not asserted.

- **A write before the read.** An attached node that writes a detached node's
  input inside the anchor, before the first reader: detached, the reader sees
  the new value this tick; placed, at the next tick.
- **State under a branch not taken.** A stateful node read only under a
  branch the tree does not take: detached, it is not called. Placed at the
  root, it advances every tick. Placed at its readers, it stays dormant while
  its anchor is not reached.
- **A sink's key.** A tree node reading a key a sink publishes: detached, it
  sees last tick's value; placed, this tick's, since the group runs before the
  root.
- **Readers in separate branches.** Reaching the anchor runs the group, even
  when the branch taken reaches none of its readers.
- **Sinks of one key.** Detached sinks publish after all are up to date, in
  file order. Placed sinks write as they run, in group and data order.
- **Errors.** A detached sink that fails stops no other. A placed node that
  fails stops its `WITH`, and the body with it.
- **Long-running calls.** A placed node that returns `Running` is polled
  every tick; a detached one is called again only when its arguments change.

## 4. Measurements

`cargo bench -p arora-behavior-tree --bench incremental` adds two forms to the
incremental proposal's bench: **placed** (the detached form's file, placed with
`Readers`, on the incremental executor) and **attached-incremental-with**
(`WITH[ PARALLEL[ leaf × N ], RUN ]`, each leaf `_on_change`). Median µs per
tick, Apple M4 Pro, release profile, native test bridge.

| N = 1000 | settled | one input changes | every input changes |
|---|--:|--:|--:|
| attached-naive | 1069 µs · 1000 calls | 1052 µs · 1000 calls | 1101 µs · 1000 calls |
| attached-incremental | 14 µs · 0 | 42 µs · 10 | 1053 µs · 841 |
| attached-incremental-with | 11 µs · 0 | 43 µs · 10 | 1182 µs · 841 |
| detached | 220 µs · 0 | 226 µs · 10 | 430 µs · 828 |
| placed | 12 µs · 0 | 36 µs · 10 | 874 µs · 828 |

At N = 10 and 100, settled: placed 0.26 and 1.2 µs, detached 2.0 and 23 µs.

- **Placed and detached make the same calls** in every scenario and size: the
  same nodes run, at different times.
- **A settled placed group costs nothing but the `WITH`.** Its `PARALLEL` has
  no `RUN`, so it is clean and skipped whole. `attached-incremental` keeps
  `RUN` inside its `PARALLEL`, which keeps that `PARALLEL` dirty and checking
  every child; written as `WITH[ PARALLEL[ leaf × N ], RUN ]`, the same leaves
  cost what placed ones do. The difference is the shape, which any attached
  node graph can take, not placement.
- **When every input changes, detached nodes are cheaper.** A placed node is
  an attached leaf, ticked through an engine dispatch; a detached node is
  called directly. Placed groups also add a `PARALLEL` per group and a `WITH`
  per anchor: 930 dispatches against 828 calls here, from 100 groups.
- **When one input changes, placed is cheapest.** Each connected group has its
  own `PARALLEL`, so only the group whose input changed is visited, and the 99
  others are skipped as one node each. A single `PARALLEL` checks all 1000
  children; a queue of dirty children per composite
  ([incremental proposal](proposal-incremental-behavior-tree.md) §5) would
  give it the same cost.

## 5. Open details

- **Lifetime.** With a halt, a `WITH` that its parent stops ticking could drop
  its groups' state, so a branch's smoothing restarts when the branch starts
  again. Arora has no halt (§1); this placement needs none.
- **Finer groups.** A group per value read, instead of per connected
  component, places each value closer to its readers and runs less of the
  graph when one branch is taken. At the limit, it is #396's evaluation.
- **Errors in prerequisites.** Whether a failing group should stop the body,
  or be reported and skipped, as a failing detached sink is.
- **Placing on edit.** A re-placement after an edit starts the incremental
  executor from a dirty tree, which costs one full tick.
- **Load time.** Placement walks every path of the attached tree. It is not
  measured.
