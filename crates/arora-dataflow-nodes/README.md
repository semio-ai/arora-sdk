# arora-dataflow-nodes

Arora's dataflow nodes: a node vocabulary of arithmetic, logic, time,
smoothing, vectors, noise, blending, records, calls to other modules and
robotics, each node kind one function of an Arora module, computing on
Arora's values (`arora_types::value::Value`). A loader puts the calls on a
behavior tree ([proposal](../../docs/proposal-behavior-languages.md)), each a
detached node, and `functions::DataflowNodes` answers them in-process.

| Module | Functions |
|---|---|
| `@arora/dataflow` (`functions::DATAFLOW`) | the general node kinds, `select`, which applies a selector, and `external`, a call to another module |
| `@arora/geometry` (`functions::GEOMETRY`) | `vec3cross`, which builds a vec3 |
| `@arora/kinematics` (`functions::KINEMATICS`) | `inversekinematics` and the URDF nodes, behind the `urdf_ik` feature |

Vizij's node graph is one such loader: its graph format, the importer from a
graph to a tree and `TreeRuntime` are vizij-rs's
[`vizij-graph-core`](https://github.com/vizij-ai/vizij-rs/tree/main/crates/node-graph/vizij-graph-core).

## A node kind's function

A node kind (`kind::Kind`) is the function `functions::function_id(kind)`
of the module `functions::module_of(kind)`; `functions::function_index()`
describes every function. Its parameters:

| Parameter | Id | What it holds |
|---|---|---|
| an input port | `input_param(port)` | the port's value, an optional argument: a port with none reads its default |
| a node parameter | `param_field(field)` | a `params::Params` field; `functions::encode` gives the arguments a node's parameters set |
| the clock | `time_param()`, `dt_param()` | the time and the step's duration, in seconds, for the kinds `reads_time` and `reads_dt` name |
| the state | `state_param()` | a stateful node's state, a mutable parameter: the call writes it back, and the next starts from it |
| the node | `node_param()` | the node's id, which names it in an error and keys what the module keeps for it |
| the ports | `ports_param()` | the output ports of a node returning more than `out`, in order |

A node whose one port is `out` returns that port's value; any other returns
a structure of its ports, each under the field `output_field(port)`. A
node's ports are its kind's (`ports::ports`), and its output ports depend on
its parameters too: a split's sizes, a record's keys
(`eval::plan::output_layout`).

```rust
use arora_dataflow_nodes::functions::{function_id, input_param, node_param, DataflowNodes};
use arora_dataflow_nodes::Kind;
use arora_types::value::Value;

// `device_bridge` runs the calls a function makes to other modules.
let mut nodes = DataflowNodes::new();
let (a, b, node) = (Value::F32(1.0), Value::F32(2.5), Value::String("sum".into()));
let sum = nodes.call_borrowed(
    function_id(&Kind::Add),
    &[(input_param("operand_0"), &a), (input_param("operand_1"), &b), (node_param(), &node)],
    &mut device_bridge,
)?;
assert_eq!(sum.ret, Value::F32(3.5));
```

## On a behavior tree

- **Placement.** `functions::placement` says where a tree holds a call. A
  node of the vocabulary is a detached call, run when what it returns is read
  and only when its arguments changed. A task run and a spawn are leaves
  ticked at every tick, as they act at each evaluation. A call to another
  module (`externalfunction`) is left out when its function returns nothing
  and writes nothing back, and is otherwise a leaf ticked at every tick,
  since a module's function may keep state its arguments do not show.
- **The bridge.** `DataflowNodes::bridge(inner)` answers the three modules'
  calls and passes every other to `inner`, the device's. It registers the
  tree's callables itself, so a tree lowered over it ticks through it.
- **What it keeps.** `DataflowNodes` keeps, for each node it is called for,
  the decoded parameters, port layouts and buffers of its last call, so a
  call whose parameters did not change prepares nothing.
  `DataflowNodes::retain_sites` forgets the nodes a loader removed.
  `DataflowNodes::state` decodes a node's `state` argument.
- **Calls to other modules.** An `external` call goes to the module its
  function is routed to (`DataflowNodes::set_function_modules`), with its
  `args` ports under the parameter ids the node names.

`tests/calls.rs` calls the functions alone and on a tree, as a loader does.

## Values

- The numeric nodes read a value's form off the value itself, so they need no list of value types. A value whose leaves are numbers flattens: a structure field by field in declaration order, a record in key-name order, a sequence item by item (`eval::value_layout`). The result has the same form, with the same structure ids, field ids and keys. A structure flattens when its number fields are floats, and keeps their types (an `F64` field stays `F64`), though every number computes in `f32`. A structure with an integer field has no numeric form, as one with a text field: rebuilding it between a node's steps would round it. A number on its own, in a record or in a sequence, of any width, is an `F32`. Two operands combine when they have the same form, or when one is a number, which applies to every component of the other: a vec4, a quaternion and a colour each hold four numbers, and do not combine.
- The geometry types (vec2, vec3, vec4, quat, transform) are [`arora-geometry`](../arora-geometry/README.md)'s, and only the geometry and kinematics nodes and selectors name them.
- Records and enumerations are `KeyValue`s and `Enumeration`s under the ids `vocab` declares: a name-keyed record (`vocab::record`) and a name-identified enumeration (`vocab::enumeration`, `vocab::variant_id`).

## Selectors

- A selector (`select::Selector`) is a path of field names and indices, which `select::select` applies to a value, and the `select` function to its input.
- A field picks a record's field, a transform's part (`translation` or `position`, `rotation`, `scale`) or an enumeration's payload. An index picks an element of a numeric vector, an item of a sequence, or a structure's field in declaration order, whatever the structure's type: `["translation", 1]` on a transform is its translation's y.
- A selection reads the value alone. An invalid path fails with an error naming what it selected in (“selector index 5 out of bounds for vec3”).

## The clock

- A `dt` of zero is a step with no time in it, and moves nothing that time drives: `Time` and `Oscillator` read the unchanged `t`, `Spring` records its new target and keeps its position and velocity, and `Damp` and `Slew` keep their value (so does a negative or non-finite `dt`). A call at `dt = 0` therefore carries a changed input through every node that does not depend on time — a paused scene showing an edit — and a smoother it reaches takes it as its new target, which shows once time passes; the next step with time in it moves on as if the zero step had not been. An IK node answers a request equal to its last one with its last solution, so it holds too.

## Installation

```bash
cargo add arora-dataflow-nodes
```

| Feature   | Default | Description |
|-----------|---------|-------------|
| `urdf_ik` | ✔       | The URDF IK/FK nodes (depends on `k` and `urdf-rs`). |

## Development & Testing

```bash
cargo test -p arora-dataflow-nodes
cargo test -p arora-dataflow-nodes --no-default-features
```

Vizij's graphs, their fixtures and the benchmark of a graph on the tree run in vizij-rs, in `vizij-graph-core`.

## Related Packages

- [`arora-geometry`](../arora-geometry/README.md): the geometry types and the blending of values.
- [`arora-behavior-tree`](../arora-behavior-tree/readme.md): the tree the calls run on.
- [`vizij-graph-core`](https://github.com/vizij-ai/vizij-rs/tree/main/crates/node-graph/vizij-graph-core): Vizij's graph format, loaded onto the tree as calls of these functions.
