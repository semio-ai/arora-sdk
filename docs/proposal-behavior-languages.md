# Proposal: one behavior graph, several execution languages

Status: draft, for review.
Date: 2026-10-09.

A device runs one behavior. Parts of it are best written as behavior trees:
actions and skills, which start, run and end with a status. Other parts are
best written as dataflow networks: rigs and mappings, which turn values into
values every tick and never end. This proposal makes them two languages of the
same behavior graph. A node's function decides what its children mean, so the
way a graph is organized tells how it runs. It also states the execution rules
both languages need, which no interpreter states today.

## 1. Where things stand

**The shared model already has the two relations.** An
[`arora_behavior::Graph`](../crates/arora-behavior/src/graph.rs) has:
- *links*, which carry values between node slots;
- *structure*: `root` and each node's ordered `children`.

The behavior tree uses the structure: a control node's function (`SEQ`,
`FALLBACK`, `PARALLEL`) decides how its children tick. Vizij's processing
graph uses only the links and topo-sorts them.

**A device hosts one interpreter**, set at build. Arora's step ticks it last,
so its writes are the frame's final ones.

**Neither interpreter has a rule for the nodes the links leave unordered.**
`Graph.nodes` is a map, so any order an author or a composer gave is lost on
the way through the shared model:
- **Vizij** composes a face's sources in a documented precedence: the rig's
  graphs, then the mappings, then the playing program, then the animations,
  the later winning a shared path. Lowering sorts nodes by id instead. On a
  device that evaluates `animations::` first and `studio::` last, so the
  source documented to override everything loses every shared path.
- **The behavior tree** lowers in map order with only the root placed first,
  though a port link needs its producer lowered before its consumer. It also
  keeps concurrent runs in a map, so which run's write wins is unspecified.

## 2. The rules

These hold for every language. Rules 1 to 4 are what Vizij's node graph does
once its order is kept. Rules 5 and 6 are what keeping it takes.

1. **A value moves within a tick only along a link.** A node runs after every
   node it reads from: within a tick, evaluation goes from inputs to outputs.
2. **A store path carries a value to the next tick.** A read sees the store as
   it was when the tick began. A write lands in the tick's one `StateChange`,
   and the next tick's reads see it. Paths are the contract between parts of a
   behavior that no link joins, so no link crosses from one composite to a
   sibling composite.
3. **Where no link orders two nodes, the listed order does:** a node's
   `children` order, the earlier first. On a path both write, the later write
   wins.
4. **Composites nest, and the rule applies at every level.** A composite runs
   as a unit in its parent's order. Everything under a later sibling runs after
   everything under an earlier one, and wins the paths they share.
5. **The order is part of the graph.** It lives in `root` and `children`. It
   survives encoding and decoding, and it travels through LOAD and EDIT like
   any other part of the graph. EDIT places a new node by replacing its parent
   with the parent's updated `children`. A node no composite holds runs after
   every placed node, in id order, so even a graph with no structure has one
   defined order.
6. **A run joins the graph at a declared place.** SPAWN grafts the run's
   fragment as a child of the composite that holds runs, after the runs
   already there. A later run therefore writes after an earlier one.

## 3. Composites are the languages

A composite's function says how its children run. Three cover what Arora and
Vizij do today:

| composite | its children are | each tick |
|---|---|---|
| **layers** | parts of a behavior, in precedence order | every child runs, in order; the later wins a shared path |
| **flow** | a dataflow network | every child is evaluated, topologically, ties in listed order (rules 1, 3) |
| **tree controls** (`SEQ`, `FALLBACK`, `PARALLEL`, …) | sub-behaviors with a status | children tick as the control decides, by status |

They nest:
- **A device's root is a `layers`:**

  ```
  layers (root)
  ├── flow  rig               ← the face's own graphs, in the bundle's order
  ├── flow  pose-driver
  ├── flow  standard::…       ← the mappings, ROS4HRI and Studio among them
  ├── layers runs             ← the program and every skill run, in spawn order
  └── flow  animations
  ```
- **A tree leaf can be a `flow`.** A rig-like controller runs while the leaf is
  `Running`.
- **A `flow` can start a tree run.** A spawn node (Vizij's `spawn`) asks the
  host to graft a run, and a status key reports it back.

Each language keeps its own executor: the tree engine for tree controls, the
node-graph plan for a `flow`. One interpreter dispatches each composite to its
executor and orders the composites by the rules above. Nothing about a
language leaks into another: the only things they share are the store and the
structure.

## 4. What changes

**In Vizij, to fix the precedence (VIZ-183):**
- The codec encodes the composed spec as the tree above: a `layers` root, one
  `flow` per composed source in composition order, and each source's nodes as
  its children in listed order.
- Decoding lists nodes in a pre-order walk of the root.
- A grafted run becomes a child of `runs`. The plan inserts its component at
  that place rather than at the end. Components are disjoint, so this is the
  same remapping as removal.
- The device's program, a run today, sits in `runs` with the skills.
- `compose_sources`' "last writer wins" warning then names the writer that
  actually wins.

**In arora-sdk:**
- `layers` and `flow` become well-known function ids next to the tree's
  controls. Every interpreter can then read the structure the same way, even
  one that runs a single language.
- The tree's lowering follows `root` and `children`, so a port link's producer
  is lowered first whatever the map order.
- Concurrent runs are kept in spawn order.

## 5. Toward one interpreter

Today a device picks a behavior tree *or* a node graph. With composites as the
languages, the target is one interpreter that runs both:
- a `layers` root;
- tree controls where the behavior decides;
- `flow`s where it computes.

**What stays as it is:**
- the `BehaviorInterpreter` trait;
- LOAD, EDIT, SPAWN and HALT;
- task runs;
- the store as the contract.

**Where it lives is open.** arora-sdk stays free of Vizij, and Vizij's
dataflow executor is written over Arora's `Value`. So either the combined
interpreter lives on the Vizij side and depends on the tree engine, or the
dataflow executor moves into Arora.

## 6. Open questions

- **Runs and animations.** The layout above places the runs below the
  animations, as Vizij's composition documents ("a playing animation overrides
  everything"). A skill run such as `play_viseme` then yields its lips to a
  playing animation. If a run should win instead, `runs` goes last.
- **Reads within a tick, in a tree.** Rule 2 is what Vizij does. Whether a
  tree leaf sees an earlier leaf's write in the same tick needs the same
  explicit answer.
- **When a `flow` under a tree is done.** A `flow` never ends on its own. As a
  tree leaf it needs a declared output that reports its status, or it stays
  `Running`.
- **What EDIT keeps.** A structural edit re-lowers. The tree resets its nodes'
  state, while the node graph keeps it. One rule should say what an edit keeps
  in both.
