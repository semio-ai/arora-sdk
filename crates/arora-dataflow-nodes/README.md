# arora-dataflow-nodes

Arora's dataflow nodes: the vocabulary of a node graph (arithmetic, logic,
time, smoothing, vectors, noise, blending, records, kinematics, calls to other
modules) as the functions of three Arora modules, computing on Arora's values
([`Value`](../arora-types/readme.md)). A graph loader places one call per
graph node beside a [behavior tree](../arora-behavior-tree/readme.md), where
it runs when what it returns is read; `DataflowNodes` answers the calls
in-process.

## A node kind is a function

Each node kind ([`Kind`](src/kind.rs)) is one function, named by the kind's
graph name (`add`, `damp`, `urdfikposition`), and each node of a graph is one
call of it:

| Parameter | Id | Holds |
|---|---|---|
| `node` | `node_param()` | the node's id in the graph, which names it in an error and keys what the module keeps for it |
| an input port (`in`, `operand_0`, …) | `input_param(port)` | the port's value; a port with no argument reads its default |
| a node parameter, `param.<field>` (`param.half_life`) | `param_field(field)` | a [`Params`](src/params.rs) field; `functions::encode` gives the arguments a node's parameters set |
| `t`, `dt` | `time_param()`, `dt_param()` | the time and the step's duration: a float in seconds, or an unsigned integer in nanoseconds (`arora/time`, `arora/dt`), for the kinds `reads_time` and `reads_dt` name |
| `state` | `state_param()` | a stateful node's state, a mutable parameter: the call writes it back, and the next starts from it |
| `ports` | `ports_param()` | the output ports of a node returning more than `out`, in order |

A node whose one port is `out` returns that port's value; any other returns
a structure of its ports, each under the field `output_field(port)`. A
node's ports are its kind's ([`ports::ports`](src/ports.rs)), and its output
ports depend on its parameters too: a split's sizes, a record's keys
(`eval::plan::output_layout`). Parameter ids derive from the role alone, so a
port has one id whatever function it is a parameter of.

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

The functions belong to three modules ([`functions`](src/functions.rs)):

| Module | Functions |
|---|---|
| `@arora/dataflow` (`functions::DATAFLOW`) | the general node kinds; `select`, which applies a selector; `external`, a call to another module's function |
| `@arora/geometry` (`functions::GEOMETRY`) | `vec3cross`, the cross product of two values of three numbers, in the form of the first |
| `@arora/kinematics` (`functions::KINEMATICS`) | `inversekinematics`, and the URDF nodes behind the `urdf_ik` feature |

`functions::function_index()` describes every function, each parameter and
return of any type.

## On a behavior tree

A node of the vocabulary is a detached node of the tree: it runs when what it
returns is read, once per tick, and only when its arguments changed. Written
in the [tree's text](../arora-behavior-tree/docs/text.md), with the clock and
the state as arguments:

```text
running

d: {face/out} = damp(in: {face/x}, param.half_life: 0.12f32, t: {arora/time}, state: ())
```

Here `d` writes the store key `face/out`, so it is a sink, brought up to date
at every tick, and its state lives in the call site's `state` cell.

- **Placement.** `functions::placement` says where a tree holds each call.
  A task run (`taskrun`) and a spawn are leaves of the tree, ticked at every
  tick, as they act at each evaluation. A call to another module
  (`externalfunction`) is left out when its function returns nothing and
  writes nothing back, and is otherwise a leaf ticked at every tick: a
  module's function may keep state its arguments do not show, such as the
  animation module's `step_values`, which advances its players by the time it
  is given.
- **The bridge.** The functions are not a module a device loads: a host runs
  them by wrapping the device's [`CallBridge`](../arora-types/src/call.rs)
  with `DataflowNodes::bridge(inner)`, which answers the three modules' calls
  and passes every other to `inner`. It registers the tree's callables
  itself, so a tree lowered over it ticks through it. Vizij's
  [`vizij-graph-core`](https://github.com/vizij-ai/vizij-rs/tree/main/crates/node-graph/vizij-graph-core)
  (`TreeRuntime`) is such a host, and its loader places Vizij's node graphs
  on a tree.
- **What it keeps.** `DataflowNodes` keeps, for each node it is called for,
  the decoded parameters, port layouts and buffers of its last call, so a
  call whose parameters did not change prepares nothing.
  `DataflowNodes::retain_sites` forgets the nodes a loader removed, and
  `DataflowNodes::state` decodes a node's `state` argument.
- **Calls to other modules.** An `external` call goes to the module its
  function is routed to (`DataflowNodes::set_function_modules`,
  `set_default_module`), with its `args` ports under the parameter ids the
  node names. The runs `spawn` nodes request wait in
  `DataflowNodes::take_spawns`.

## Values

- **Numbers and their forms.** The numeric nodes read a value's form off the
  value itself, so they need no list of value types. A value whose leaves are
  numbers flattens: a structure field by field in declaration order, a record
  in key-name order, a sequence item by item (`eval::value_layout`). The
  result has the same form, with the same structure ids, field ids and keys.
  A structure flattens when its number fields are floats, and keeps their
  types (an `F64` field stays `F64`), though every number computes in `f32`.
  A structure with an integer field has no numeric form, as one with a text
  field: rebuilding it between a node's steps would round it. A number on
  its own, in a record or in a sequence, of any width, is an `F32`. Two
  operands combine when they have the same form, or when one is a number,
  which applies to every component of the other: a vec4, a quaternion and a
  colour each hold four numbers, and do not combine.
- **Geometry.** The geometry types (vec2, vec3, vec4, quat, transform) are
  [`arora-geometry`](../arora-geometry/README.md)'s, and only the geometry
  and kinematics nodes and selectors name them.
- **Records and enumerations** are `KeyValue`s and `Enumeration`s under the
  ids [`vocab`](src/vocab.rs) declares: a name-keyed record (`vocab::record`)
  and a name-identified enumeration (`vocab::enumeration`,
  `vocab::variant_id`).
- **Task runs** speak the behavior `Status` enumeration of
  [`arora-behavior`](../arora-behavior/src/status.rs) ([`task`](src/task.rs)).

## Selectors

- A selector ([`select::Selector`](src/select.rs)) is a path of field names
  and indices, which `select::select` applies to a value, and the `select`
  function to its input.
- A field picks a record's field, a transform's part (`translation` or
  `position`, `rotation`, `scale`) or an enumeration's payload. An index
  picks an element of a numeric vector, an item of a sequence, or a
  structure's field in declaration order, whatever the structure's type:
  `["translation", 1]` on a transform is its translation's y.
- A selection reads the value alone. An invalid path fails with an error
  naming what it selected in (“selector index 5 out of bounds for vec3”).

## The clock

A `dt` of zero is a step with no time in it, and moves nothing that time
drives: `time` and `oscillator` read the unchanged `t`, `spring` records its
new target and keeps its position and velocity, and `damp` and `slew` keep
their value (so does a negative or non-finite `dt`). A call at `dt = 0`
therefore carries a changed input through every node that does not depend on
time (a paused scene showing an edit), and a smoother it reaches takes it as
its new target, which shows once time passes; the next step with time in it
moves on as if the zero step had not been. An IK node answers a request equal
to its last one with its last solution, so it holds too.

## Reference

| Feature | Default | Description |
|---|---|---|
| `urdf_ik` | yes | The URDF IK/FK nodes (depends on `k` and `urdf-rs`). |

```bash
cargo test -p arora-dataflow-nodes
cargo test -p arora-dataflow-nodes --no-default-features
```

`tests/calls.rs` calls the functions alone and on a tree, as a loader does.
The per-node code, and how to add a node kind, are in
[`src/eval/`](src/eval/README.md). Vizij's graphs, their fixtures and the
benchmark of a graph on the tree run in vizij-rs, in `vizij-graph-core`.

## Related packages

- [`arora-behavior-tree`](../arora-behavior-tree/readme.md): the tree the
  calls run beside.
- [`arora-geometry`](../arora-geometry/README.md): the geometry types and the
  blending of values.
- [`vizij-graph-core`](https://github.com/vizij-ai/vizij-rs/tree/main/crates/node-graph/vizij-graph-core):
  Vizij's graph format, loaded onto the tree as calls of these functions.
