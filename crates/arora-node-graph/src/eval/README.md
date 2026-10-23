# The per-node code

This directory holds what each node kind computes: the code the module's functions ([`functions`](../functions.rs)) run for a call, over the node's inputs, parameters, clock and state.

## Files

| File | Responsibility |
|------|----------------|
| `eval_node.rs` | Dispatches a `Node` (its id, kind and `Params`) to its kind's code, over its input and output slots of `Value`s. |
| `context.rs` | One call's `Context`: the clock (`t`, `dt`), the node's state (springs, dampers, slews, URDF chains, task runs, spawns) and the runs it requests. |
| `plan.rs` | A node's input and output port layouts, from its kind's signature in the editor registry ([`schema`](../schema.rs)). |
| `value_layout.rs` | Flattens a value whose leaves are numbers into a buffer, by its own form, and rebuilds it. |
| `numeric.rs` / `variadic.rs` | Unary, binary and variadic arithmetic over flattened values, and number readings. |
| `noise.rs` | The noise nodes' generators. |
| `shape_helpers.rs` | Applies an edge's selector, and Vizij's declared shapes: inference, matching and numeric coercion. |
| `vizij.rs` | What a call does for Vizij's graph format: an `input` node fitting its value to its declared shape, the check of declared output shapes, and a keyed batch's writes. |
| `urdfik.rs` | The URDF IK/FK helpers, behind the `urdf_ik` feature. |

## A call

1. The module decodes the call's static arguments into the node's `Params`, and lays out its inputs.
2. It sets the `Context`'s clock and the node's state, then `eval_node_inner` runs the node's code, which writes its outputs.
3. The module checks the node's declared output shapes, returns its outputs, and writes its state back.

Staging inputs, ordering the nodes and writing the graph's outputs are the tree's: `TreeRuntime` stages inputs in its store, and the behavior tree runs each node when what it outputs is read.

## Adding a node kind

1. Extend `NodeType` in `types.rs`, and give it its signature in the editor registry (`schema.rs`).
2. Write its code in `eval_node.rs`, with helpers to keep the dispatch short.
3. A node that keeps state between calls keeps it in the `Context` (see `Spring`, `Damp`, `Slew`).
4. Test it on the tree (`tree_tests`).

URDF code stays behind the `urdf_ik` feature, to keep the default dependencies light.
