# Arora Behavior Trees

Implementation of a behavior tree executor based on Arora module nodes.
A [`BehaviorTree`](src/behavior_tree.rs) is a set of
[`Node`s](src/schema.rs) arranged in a tree,
*i.e.* with all nodes having a single parent contained
in the behavior tree, but the root node.
The [`BehaviorTree`](src/behavior_tree.rs) holds a list `variables`, *a.k.a.* the *blackboard*.
This is property is checked when the behavior tree is loaded from a set of nodes,
using [`load_behavior_tree_nodes` or `load_behavior_tree_yaml`](src/behavior_tree.rs).

## Schema

The YAML format to the schema defined in [`src/schema.rs`](src/schema.rs).
It consist of a list of nodes,
which arguments can be expressed dynamically with a flexible `Expression`.
It can be:
- a serialized [`Value`, like defined in `arora-types`](https://github.com/semio-ai/arora-types),
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

Setting up the runtime requires a [`CallBridge`](https://github.com/semio-ai/arora-engine),
which the [Arora engine](https://github.com/semio-ai/arora-engine) implements.
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
before calling [`arora_dispatch_indirect`](https://github.com/semio-ai/arora-engine).
The return value is always expected to be a `behavior_tree::Status`.

## Basic Nodes and Helpers

Some [helpers are also available to create behavior trees in code](src/nodes.rs),
using basic nodes provided by the module
[`behavior_tree_nodes`](../modules/behavior-tree-nodes/readme.md).
[`TreeNode`](src/tree_node.rs) offers an alternative way to build behavior trees for Rust applications,
where the nodes are directly created as trees (instead of being juxtaposed flatly),
providing a build-time guarantee of the validity of the structure.

## Groot Support

[Groot](https://github.com/BehaviorTree/Groot) is a behavior tree editor in C++ Qt,
that is used along the mainstream [BehaviorTreeCPP](https://github.com/BehaviorTree/BehaviorTree.CPP)
C++ library for behavior trees.
It is used in [Nav2](https://docs.nav2.org/tutorials/docs/using_groot.html),
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
if they are mentioned in the [module function description](https://github.com/semio-ai/arora-types).

Statelessness is important in a client / server architecture - behavior tree / module, here -
to ensure that the server is designed to not maintain resources busy
if the communication with the client is absolutely dropped.
It also favors reproducibility, parallelization, reusability and scalability.
In this particular case, it also favors introspectability,
since states are meant to be accessible in the behavior tree.
