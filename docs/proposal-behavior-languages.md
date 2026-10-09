# Proposal: one behavior graph, several execution languages

Status: draft, for review.
Date: 2026-10-09.

A device runs one behavior. Parts of it are best written as behavior trees:
actions and skills, which start, run and end with a status. Other parts are
best written as dataflow networks: rigs and mappings, which turn values into
values every tick and never end. This proposal makes them two languages of the
same behavior graph. A node's function decides what its children mean, so the
way a graph is organized tells how it runs. A behavior tree already works this
way. The dataflow graph gets the same rule.

## 1. Where things stand

**The shared model already has the two relations.** An
[`arora_behavior::Graph`](../crates/arora-behavior/src/graph.rs) has:
- *links*, which carry values between node slots;
- *structure*: `root` and each node's ordered `children`.

**A behavior tree's order is its structure.** A tick goes from the root to the
leaves. Each control node (`SEQ`, `FALLBACK`, `PARALLEL`) ticks its children
in their listed order, as its function decides. A node reads and writes the
store when it is ticked, so a leaf sees what an earlier leaf wrote in the same
tick.

**Vizij's processing graph uses only the links.** It topo-sorts them and has
no structure to break the ties, while `Graph.nodes` is a map. So the order a
composer gives is lost on the way through the shared model. A face's sources
are composed in a documented precedence: the rig's graphs, then the mappings,
then the playing program, then the animations, the later winning a shared
path. Lowering sorts nodes by id instead. On a device that evaluates
`animations::` first and `studio::` last, so the source documented to override
everything loses every shared path.

**A device hosts one interpreter**, set at build. Arora's step ticks it last,
so its writes are the frame's final ones.

## 2. The rule: the structure is the order

This is the behavior tree's rule, made the rule of every language:

1. **A tick walks the structure from the root to the leaves.** At each
   composite, the composite's function decides how its children run. Unless it
   says otherwise, they run in their listed order.
2. **A node reads and writes the store when it runs.** On a path two nodes
   write in the same tick, the later one wins.
3. **The order is part of the graph.** It lives in `root` and `children`, so
   it survives encoding and decoding and travels through LOAD and EDIT like
   the rest of the graph. EDIT places a new node by replacing its parent with
   the parent's updated `children`. A node no composite holds runs after every
   placed node, in id order, so even a graph with no structure has one defined
   order.
4. **A run joins the graph at a declared place.** SPAWN grafts the run as a
   child of the composite that holds runs, after the runs already there. A
   later run therefore writes after an earlier one.

## 3. Composites are the languages

A composite's function says how its children run. Three cover what Arora and
Vizij do today:

| composite | its children are | each tick |
|---|---|---|
| **tree controls** (`SEQ`, `FALLBACK`, `PARALLEL`, …) | sub-behaviors with a status | children tick as the control decides, by status |
| **layers** | parts of a behavior, in precedence order | every child runs, in order; the later wins a shared path |
| **flow** | a dataflow network | every child is evaluated in link order: a node after every node it reads from, ties in listed order |

A `flow` is where links carry values within a tick. A link stays inside its
`flow`. Parts of a behavior that no link joins meet only through store paths,
read and written in the order the structure gives.

They nest:
- **A device's root is a `layers`:**

  ```
  layers (root)
  ├── flow  rig               ← the face's own graphs, in the bundle's order
  ├── flow  pose-driver
  ├── flow  standard::…       ← the mappings, ROS4HRI and Studio among them
  ├── flow  animations
  └── layers runs             ← the program and every skill run, in spawn order
  ```

  The runs come last, so a skill overrides a playing animation on the paths
  both write. The device's program is a run too, so it does the same.
- **A tree leaf can be a `flow`.** A rig-like controller runs while the leaf is
  `Running`.
- **A `flow` can start a tree run.** A spawn node (Vizij's `spawn`) asks the
  host to graft a run, and a status key reports it back.

Each language keeps its own executor: the tree engine for tree controls, the
node-graph plan for a `flow`. One interpreter walks the structure and hands
each composite to its executor. Nothing about a language leaks into another:
the only things they share are the store and the structure.

## 4. What changes

**In Vizij, to fix the precedence (VIZ-183):**
- The codec encodes the composed spec as the tree above: a `layers` root, one
  `flow` per composed source in composition order, and each source's nodes as
  its children in listed order.
- Decoding lists nodes in a pre-order walk of the root, which is the order the
  plan's topological sort breaks ties by.
- A grafted run becomes the last child of `runs`, so the plan still appends
  its component at the end.
- The device's program, a run today, sits in `runs` with the skills.
- `compose_sources`' "last writer wins" warning then names the writer that
  actually wins.

**In arora-sdk:**
- `layers` and `flow` become well-known function ids next to the tree's
  controls. Every interpreter can then read the structure the same way, even
  one that runs a single language.
- The tree's runner lists its runs in spawn order. It builds the `PARALLEL`
  that holds them from a map today, so the order among concurrent runs is not
  defined.

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

- **When a `flow` reads the store.** Rule 2 has a node read when it runs.
  Vizij's processing graph reads every input path before it evaluates, so a
  `flow` sees an earlier sibling's write only on the next tick. Reading when
  the `flow` runs makes it follow the rule and removes a tick of latency.
- **When a `flow` under a tree is done.** A `flow` never ends on its own. As a
  tree leaf it needs a declared output that reports its status, or it stays
  `Running`.
- **What EDIT keeps.** A structural edit re-lowers. The tree resets its nodes'
  state, while the node graph keeps it. One rule should say what an edit keeps
  in both.
