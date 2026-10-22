# Arora Behavior Trees

Implementation of a behavior tree executor based on Arora module nodes.
A [`BehaviorTree`](src/behavior_tree.rs) is a set of
[`Node`s](src/schema.rs) arranged in a tree,
*i.e.* with all nodes having a single parent contained
in the behavior tree, but the root node.
The [`BehaviorTree`](src/behavior_tree.rs) holds a list `variables`, *a.k.a.* the *blackboard*.
This is property is checked when the behavior tree is loaded from a set of nodes,
using [`load_behavior_tree_nodes` or `load_behavior_tree_yaml`](src/behavior_tree.rs).

> **How the tree fits the Arora interpreter model** — what a node is, and how
> nodes touch the data store and the engine's modules — is diagrammed in
> [`docs/nodes.md`](docs/nodes.md), which builds on
> [arora-behavior's interpreter workflow](../arora-behavior/docs/interpreter-workflow.md).

## Schema

The YAML format to the schema defined in [`src/schema.rs`](src/schema.rs).
It consist of a list of nodes,
which arguments can be expressed dynamically with a flexible `Expression`.
It can be:
- a serialized [`Value`, like defined in `arora-types`](https://github.com/semio-ai/arora-sdk),
  or more specifically an [UUID](https://docs.rs/uuid/latest/uuid/),
- a reference to a variable, in-memory or identified with the
  [UUID](https://docs.rs/uuid/latest/uuid/) of blackboard variable.
- a reference to the argument of another node, so that to skip the general blackboard.
- the description of a call to perform every time the node is ticked.

## Runtime

To run a behavior tree, use [`BehaviorTreeRuntime::setup`](src/behavior_tree.rs)
to create a `BehaviorTreeRuntime` instance, that proposes the general `tick` function:
`BehaviorTreeRuntime` implements `Tickable`.
`Tickable::tick` takes no argument and returns a `behavior_tree::Status`.

> Note: we call "parameter" the declaration of
> what function may accept as inputs (or outputs, if `mutable`).
> We call "argument" the actual value passed to the function.

`behavior_tree::Status` is defined in [`arora-behavior-tree-types`](../arora-behavior-tree-types/readme.md),
and can be `Success`, `Failure` or `Running`.
**Any function of a module that returns a `behavior_tree::Status`
can be used by a node, and nodes can refer only to such functions.**

Alternatively, a node may set `return_binding` to an `Expression`
pointing at a blackboard variable. In that case the function's raw
return value is written to the variable after each tick, and the node
always reports `Status::Success` to its parent. This lets non-Status
functions (e.g. `add`, `cos`) participate in a behavior tree.

Setting up the runtime requires a [`CallBridge`](https://github.com/semio-ai/arora-sdk),
which the [Arora engine](https://github.com/semio-ai/arora-sdk) implements.
The setup mechanism will compute every binding of node arguments,
so that every node can be `tick`ed with no argument.
Thus the tick functions can be registered to the engine in exchange of a `CallableId`.
This identifier is wrapped in a structure called `TickId`,
also defined in [`arora-behavior-tree-types`](../arora-behavior-tree-types/readme.md).

Nodes that have children are required to expose them as their first parameter,
that should be named `children`,
identified with the UUID `5b6e9515-dbcc-411d-bee9-3d8cba5fedda`,
and accept a `Vec<TickId>`.
At runtime, the children are passed as `TickId`s
that can be converted into `CallableId`s
before calling [`arora_dispatch_indirect`](https://github.com/semio-ai/arora-sdk).
The return value is always expected to be a `behavior_tree::Status`.

## Basic Nodes and Helpers

Some [helpers are also available to create behavior trees in code](src/nodes.rs).
The basic control nodes (sequence, sequence-star, fallback, parallel), the
status leaves (succeed, fail, run) and the data nodes `Equal(a, b)`,
`Unpack(values, out_0 … out_63)` and `Scatter(values, keys, within)` are built in to this crate and dispatched natively; additional leaf and utility nodes for tests and demos live in the
[`test-behavior-tree-nodes`](../../modules/test-behavior-tree-nodes/readme.md) module.
[`TreeNode`](src/tree_node.rs) offers an alternative way to build behavior trees for Rust applications,
where the nodes are directly created as trees (instead of being juxtaposed flatly),
providing a build-time guarantee of the validity of the structure.

## Groot Support

[Groot](https://github.com/BehaviorTree/Groot) is a behavior tree editor in C++ Qt,
that is used along the mainstream [BehaviorTreeCPP](https://github.com/BehaviorTree/BehaviorTree.CPP)
C++ library for behavior trees.
It is used in [Nav2](https://docs.nav2.org/),
a well-known navigation for ROS.
[`TreeNode`s](src/tree_node.rs) can be converted
from and to [`groot::Node`s](src/schema_groot.rs).
Though most of basic Groot nodes are not implemented in this library,
it is possible to edit behavior trees with Groot,
and convert them for this library, using [`BehaviorTree::from_groot_xml`](src/schema_groot.rs).
Some example files are available in [`groot/`](groot/),
and are used in tests.
We can use and complete the [Groot Arora palette](groot/groot_arora_palette.xml) with time.

## Dealing with States in a Stateless Design

Some effort was made to keep the nodes stateless,
by avoiding the need for a clean-up function for each node function.
Instead, stateful information is meant to be passed as mutable parameters
to the node functions. Thus, it is the behavior tree that holds this state
in variables (anonymously or in the blackboard),
and it is capable of resetting them to their default values,
if they are mentioned in the [module function description](https://github.com/semio-ai/arora-sdk).

Statelessness is important in a client / server architecture - behavior tree / module, here -
to ensure that the server is designed to not maintain resources busy
if the communication with the client is absolutely dropped.
It also favors reproducibility, parallelization, reusability and scalability.
In this particular case, it also favors introspectability,
since states are meant to be accessible in the behavior tree.

## Incremental ticks

`LoweredTree::set_incremental(true)` (or the same method on
`BehaviorTreeRuntime`) enables an opt-in executor that keeps terminal statuses
and skips clean child ticks before engine dispatch. The default is off. A clean
root makes no indirect dispatch; a skipped node emits no trace line. A new
lowering starts dirty.

Native controls and value computations can cache. Module leaves run on every
visit unless their `arguments` contain
`schema::_ON_CHANGE_PARAM_ID: Expression::Value(Value::Boolean(true))`.
This reserved `_on_change` argument stays in the tree, never in a module call.
It promises that a terminal status and its outputs remain valid until an argument
changes. Nested calls, including under a selection, always disable caching.
`SetString`, `UnsetString`, `RunCall` and `Scatter` are never cached.

Every argument except `_ret` contributes its read cells, including mutable
parameters and in-out state. A visit records their versions before executing.
If any version advances during the visit, even through the node's own write,
the node and its ancestors remain dirty for the next tick. Stateful computations
therefore run until their state settles, and an action that resets its state on
completion starts again when reached. A write only to a `_ret` destination
does not self-invalidate; a foreign overwrite still invalidates its producer.

Only nodes visited in the previous tick keep polling alive: Running nodes and
non-cacheable nodes dirty themselves and their ancestors; readers with detached
pulls dirty their parent so the pulls stay reachable. A bypassed Running child
drops out of the polling list. Its cached Running status cannot answer a later
visit, so it executes when reached again.

Subscribed cells advance their version only when their observed value changes.
Store values are read live. Each tick first observes all store-backed read cells
for outside changes. An effectful visit or a stored-cell write flags possible
store movement. Before skipping a clean subtree, the executor observes its
deduplicated store reads, then checks its cache again; an executing node also
synchronizes its own store reads before recording argument versions. There is
no whole-store comparison after each visit. Tick-boundary observation costs one
pass over store-backed read cells; lazy checks cover disjoint skipped subtrees,
and polling follows only the previous tick's visits.

Detached inputs are pulled before a reader's cache check, and detached sinks
still publish after the root. Later writes invalidate earlier readers for the
next tick. Control order and the absence of halt stay the same.

With the flag off there are no incremental subscriptions, value comparisons or
value copies for change tracking on writes. Enabling subscribes the lowered read
sets and initializes their last-seen values; disabling drops those subscriptions
and invalidates cached statuses. Reusing a lowering preserves its caches.
