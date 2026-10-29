# The per-node code

What each node kind computes: the code the module's functions
([`functions`](../functions.rs)) run for a call, over the node's inputs,
parameters, clock and state.

## A call

1. The module decodes the call's static arguments into the node's `Params`,
   and lays out its inputs.
2. It sets the `Context`'s clock and the node's state, then
   `eval_node::eval_node_inner` runs the node's code, which writes its
   outputs.
3. The module returns its outputs, and writes its state back to the call's
   `state` argument.

Reading inputs, ordering the nodes and writing a graph's outputs are the
tree's and the loader's: the behavior tree runs each call when what it
returns is read, and a loader such as Vizij's (`vizij-graph-core`) binds a
graph's inputs and outputs to store keys.

## Files

| File | Holds |
|------|-------|
| `eval_node.rs` | The dispatch of a `Node` (its id, kind and `Params`) to its kind's code, over its inputs and outputs (`InputSlots`, `OutputSlots`), each a `Value`. |
| `context.rs` | One call's `Context`: the clock (`t`, `dt`), the node's state (`NodeRuntimeState`: springs, dampers, slews, URDF chains, task runs, spawns) and the runs it requests. |
| `plan.rs` | A node's input and output port layouts, from its kind's ports ([`ports`](../ports.rs)). |
| `value_layout.rs` | The flattening of a value whose leaves are numbers into a buffer, by its own form, and its rebuilding. |
| `numeric.rs`, `variadic.rs` | Unary, binary and variadic arithmetic over flattened values, and number readings. |
| `noise.rs` | The noise nodes' generators. |
| `node_function.rs` | `NodeFunctions`, what an `externalfunction` or `taskrun` node calls another function through; the module answers it with the device's bridge. |
| `urdfik.rs` | The URDF IK/FK helpers, behind the `urdf_ik` feature. |

## Adding a node kind

1. Extend `Kind` in `kind.rs`, list it in `functions::KINDS` (and in
   `functions::module_of` when it is not `@arora/dataflow`'s), and give it its
   ports in `ports.rs`.
2. Write its code in `eval_node.rs`, with helpers to keep the dispatch short.
3. A node that keeps state between calls keeps it in the `Context`, as
   `Spring`, `Damp` and `Slew` do; `functions::is_stateful` lists it, and
   `functions.rs` encodes its state into the `state` argument and decodes it
   back. A node reading the clock is listed by `functions::reads_time` or
   `functions::reads_dt`.
4. Test it as a call (`tests/calls.rs`). A graph format that names it, such
   as Vizij's, adds it to its own kinds and registry.

The URDF code stays behind the `urdf_ik` feature, which keeps the default
dependencies light when it is off.
