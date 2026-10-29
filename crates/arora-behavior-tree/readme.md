# arora-behavior-tree

The Arora behavior tree: a tree of nodes that a device ticks once per step,
whose leaves call the functions of the device's modules and whose variables
are the device's store keys. It is the
[behavior interpreter](../arora-behavior/readme.md) an Arora device runs by
default, and it reads trees written as text, as Groot XML, or as Arora's
shared behavior graph.

## A tree of nodes

A **node** is a function, the arguments it is called with and, for a control
node, its children. Ticking a node runs its function and gives a
[`Status`](../arora-behavior/src/status.rs): `Success`, `Failure`, or
`Running` (not finished; tick it again). A **tree** is a root node and the
nodes under it; ticking the tree ticks the root, which ticks its children as
its function says.

```text
fallback                                    # the first child that does not fail
  sequence                                  # each child in turn, while they succeed
    is_string_set(value: {command})         # a condition on the store key `command`
    set_string(variable: {last_command}, value: {command})
    unset_string(variable: {command})
  running                                   # nothing to do this tick
```

This tree reacts once to each write of `command` from outside it: it copies
the command to `last_command` and clears it.

A node's function is one of three kinds:

- a **native** node, which the tree runs itself: the control nodes
  (`sequence`, `fallback`, `parallel`, `sequence_star`), the status leaves, the
  data and string nodes, and the task-run nodes. Every device has them, with
  no module loaded ([`docs/nodes.md`](docs/nodes.md) lists them);
- a **module function**: the tree finds it in the device's function index
  ([`FunctionIndex`](../arora-behavior/src/lib.rs)) and calls it through a
  [`CallBridge`](../arora-types/src/call.rs), which the
  [engine](../arora-engine/readme.md) implements. A function returning a
  `Status` reports it; any other function reports `Success`, its return
  value written to a variable when the node binds one;
- a **detached** node: a call to a module function that no node of the tree
  holds, listed beside the tree. It runs when what it outputs is about to be
  read, and only when its arguments changed: dataflow beside the control flow.

An **argument** is a literal, a variable, another node's output, or a call. A
**variable** `{name}` is the store key `name` (the Direct convention): a node
reads the key, and what the node writes lands in it. A variable whose name
starts with `.`, `{.name}`, is **tree-local**: a cell of the tree, which no
other tree and no bridge sees.

## Running a tree

[`BehaviorTreeInterpreter`](src/behavior.rs) runs a tree. Load a behavior
into it, then tick it once per step with the device's store, call bridge and
function index:

```rust
use arora_behavior::{BehaviorContext, BehaviorInterpreter, FunctionIndex};
use arora_behavior_tree::behavior::BehaviorTreeInterpreter;
use arora_behavior_tree::text;
use arora_types::{call::CallBridge, data::DataStore};

const TREE: &str = "\
fallback
  sequence
    is_string_set(value: {command})
    set_string(variable: {last_command}, value: {command})
    unset_string(variable: {command})
  running
";

fn run(
    store: &dyn DataStore,
    engine: &mut dyn CallBridge,
    functions: &FunctionIndex,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read the text into the shared graph, against the device's functions.
    let parsed = text::parse(TREE);
    let loaded = text::resolve(&parsed.document, functions);
    let errors: Vec<String> = parsed
        .diagnostics
        .iter()
        .chain(&loaded.diagnostics)
        .filter(|diagnostic| diagnostic.severity == text::Severity::Error)
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("; ").into());
    }

    // Load it, then tick once per step.
    let mut interpreter = BehaviorTreeInterpreter::new();
    interpreter.load(loaded.graph)?;
    let mut context = BehaviorContext {
        store,
        call_bridge: engine,
        functions,
    };
    interpreter.tick(&mut context)?;
    Ok(())
}
```

One step is one tick of the tree. A tree that finishes, `Success` or
`Failure`, is ticked again from its root at the next step: a behavior tree is
a policy the device re-evaluates, so the interpreter itself always reports
[`BehaviorStatus::Running`](../arora-behavior/src/lib.rs).

On a device, the [`arora`](../arora/readme.md) runtime holds the interpreter
and ticks it each step. A behavior reaches it through the interpreter
module's `load` function
([`interpreter_module`](../arora-behavior/src/interpreter_module.rs)): a
remote client calls it over a bridge, and `Arora::load_groot` calls it for a
Groot file.

## Writing a tree

| Form | Read with |
|---|---|
| Text ([`docs/text.md`](docs/text.md)) | `text::parse`, then `text::resolve` into a `Graph`; or `load_behavior_tree_text` into a `BehaviorTree` |
| [Groot](https://github.com/BehaviorTree/Groot) XML | `BehaviorTreeInterpreter::load_groot`; or `schema_groot::BehaviorTree::try_from_groot_xml`, then `into_graph` |
| Arora's shared behavior [`Graph`](../arora-behavior/src/graph.rs) | `BehaviorInterpreter::load`, and `apply` with a `GraphDiff` for an edit: what an editor sends |
| YAML, a list of [`schema::Node`](src/schema.rs) | `load_behavior_tree_yaml` |
| Rust | [`TreeNode`](src/tree_node.rs) and the builders of [`nodes`](src/nodes.rs) (`seq`, `fallback`, `set_str`, …) |

The text and the shared graph say everything a tree can: the tree, the
detached nodes beside it, every binding. `BehaviorTree::display` writes a loaded
tree back as text.

Groot is the visual editor of BehaviorTree.CPP, the behavior tree library
[Nav2](https://docs.nav2.org/) runs on. A Groot tag names a native node
(`Sequence`, `SetString`, `Scatter`, …) or a module function, by its name or
its name in PascalCase. A port of a data or task-run node holds `{name}` for
a variable, `json:` and a value's serde form for a typed literal, or plain
text for a string; a port of a string node or a module function holds
`{name}` or a string.
[`groot/groot_arora_palette.xml`](groot/groot_arora_palette.xml) declares the
nodes Groot does not know: `Equal`, `Unpack`, `Scatter`, `RunCall`,
`RunStatus`, and the [`test-behavior-tree-nodes`](../../modules/test-behavior-tree-nodes/readme.md)
module's `Cos` and `Increase`. `schema_groot::BehaviorTree::to_groot_xml`
writes a tree back.

## Calling module functions

A node calling a module function passes its arguments by parameter id. After
the call, each parameter the function reports mutated is written back to the
variable its argument reads, and the return value to the variable bound to
the node's `_ret` parameter ([`_RET_PARAM_ID`](src/schema.rs)).

A function holding children is a control node a module provides. Its first
parameter is `children`, an array of `arora.TickId`
([`arora-behavior-tree-types`](../arora-behavior-tree-types/readme.md)): one
callable per child, which the function ticks through the engine's indirect
dispatch (`arora_dispatch_indirect` in a guest), receiving the child's
`Status`.

A node keeps no state hidden from the tree. A function that needs state
across ticks declares it as a mutable parameter; the tree holds the value in
the variable, or the call site's own cell, that the argument names, so the
state is visible, survives an edit that leaves the node in place, and resets
with the node. The module function itself stays a function of its arguments,
which keeps it reusable, reproducible, and free of resources a lost client
would leave held.

## The interpreter

`BehaviorTreeInterpreter` hosts one graph, rooted at a parallel **runner**.
The behavior `load` gives is the runner's first child; `apply` edits it;
`spawn` and `spawn_graph` start **task runs** beside it, each under a
decorator that publishes its status to the run's status key; `halt` removes
a run. Everything it runs is ordinary graph data, visible through `graph()`.
An edit re-lowers the graph at the next tick, and every node the edit left in
place keeps its state.

The interpreter reads the graph type `behavior-tree`
([`GRAPH_TYPE_NAME`](src/behavior.rs)) up to version 1.3: 1.0 has the control
nodes, the status leaves, the task-run nodes and module function leaves; 1.1
adds `Equal` and `Unpack`; 1.2 adds `Scatter`; 1.3 adds detached nodes.
[`docs/nodes.md`](docs/nodes.md) describes the scaffold, each native node,
how a node reaches the store and the modules, and detached nodes.

## Without the interpreter

A [`BehaviorTree`](src/behavior_tree.rs) also runs on its own, against any
`CallBridge`: `lower_behavior_tree` registers each node's tick on the bridge
and gives a `LoweredTree`, ticked once per step and unregistered when it is
replaced. `run_behavior_tree` ticks a tree back to back until it finishes,
for a tree that does (a test, a script). A tree's variables bind to store
keys through the resolver `load_behavior_tree_nodes_with` or
`graph::build_behavior_tree` is given; a variable the resolver declines is
tree-local.

## Reference

- The crate has no Cargo features, and depends on no engine: any
  `CallBridge` runs its module calls.
- `Status` and `TickId` are records of the `arora` scope:
  [`arora-behavior-tree-types`](../arora-behavior-tree-types/readme.md), and
  as record files [`arora-behavior-tree-types-yaml`](../arora-behavior-tree-types-yaml/readme.md).
- [`examples/policy-modules`](../../examples/policy-modules/README.md) drives
  a simulated robot with Groot trees.
- The benches (`benches/`: `edit`, `scatter`, `text`) state their arguments
  in their headers and count instructions, not time.

```sh
cargo test -p arora-behavior-tree
```
