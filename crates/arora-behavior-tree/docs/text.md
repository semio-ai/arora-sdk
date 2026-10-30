# The behavior text language

A behavior tree and the nodes beside it, as text. The tree is nested by
indentation; the nodes no node holds, which compute what the tree reads, are
listed after it. Nodes are named, and functions, parameters and variables are
written by name; an id is needed only to select a field out of a structure.

```text
# comment to end of line
fallback                                   # the first node is the root
  sequence
    equal(a: {person/visible}, b: true)    # {key}: a store key
    set_string(variable: {.mood}, value: "happy")   # {.x}: a tree-local variable
    below(x: @cm._ret, limit: 30)          # @node._ret: a node's return value
    smooth(x: @raw._ret, alpha: 0.2, state: @s.state)   # @node.parameter: a node's parameter
  succeed

raw: mul(a: {sensor/distance}, b: 100)     # name: … names a node; no node holds it: detached
cm: clamp(x: @raw._ret, lo: 0)
level: {face/level} = add(a: @cm._ret, b: 1)   # {target} = …: where its return value goes
s: smooth(x: @cm._ret, alpha: 0.5, state: 0)
sway: {rig/arm_wave} = mul(a: oscillator(t: {arora/time}, frequency: 1.6), b: 0.6)   # a call in an argument
```

It is the form the [display](../src/display.rs) writes: a tree shown with
`BehaviorTree::display` reads back to the graph it was built from, every node
with its id. The [`text`](../src/text/mod.rs) module reads and writes it:

| Function | Does |
|---|---|
| `text::parse(text) -> Parsed { document, diagnostics }` | The syntax tree of what is written, every part with its span. A line that does not parse is reported and left out; every other line is in the document. |
| `text::format(&parsed, &functions) -> Result<String, Vec<Diagnostic>>` | The parsed text in its canonical layout. A text with errors is refused, with its errors: what does not parse is not in its document, and would be lost. |
| `text::resolve(&document, &functions) -> Loaded { graph, nodes, diagnostics }` | The shared [`Graph`](../../arora-behavior/src/graph.rs) an interpreter runs, and where each of its nodes is written. |
| `text::natives()` | The native nodes as functions — name, parameters, types — for a palette. |
| `load_behavior_tree_text(text, &functions, &resolver)` | A runnable `BehaviorTree`, or every error with its `line:column`. |

`functions` is the device's function index: what `describeMethods` answers,
one `MethodSignature { module_id, id, name, function }` per function, is one
`ModuleFunction { module_id, function_id, function_name, function }` each.

## Lines and blocks

- A **comment** runs from `#` to the end of its line.
- A **node** is one line. The lines indented under it are its children. A
  block's lines are indented alike, with spaces: a tab in an indentation is an
  error, and so is a line indented as no enclosing block is.
- A line ending inside brackets goes on to the next lines, until the brackets
  close: a call can be broken over several lines, each indented past the line
  that opens it.
- A **blank line** separates groups; it means nothing else.
- A line ends at `\n`, `\r\n` or `\r`. A byte order mark opening the text
  is no part of it.
- **Nesting is bounded**: expressions nest at most 128 levels deep (a call in
  an argument, a value in `some(…)` or an array, a selection out of a value
  are each a level), and so do blocks of children (`text::MAX_DEPTH`). What
  is nested deeper is an error, and no reader — parser, resolver, formatter —
  goes deeper, so a text cannot run one out of stack.

The first node is the **root**: the tree is it and the nodes under it. The
nodes after it are held by no node. A call to a module function with no
children among them is **detached**: it runs when what it outputs is about to
be read ([detached nodes](../src/detached.rs)). Any other node there never
runs, and is warned about.

## Grammar

```text
document   = { line } ;
line       = [ name ":" ] [ target "=" ] function [ "(" [ arguments ] ")" ] [ comment ] NEWLINE
             [ INDENT { line } DEDENT ] ;
target     = variable | port ;
function   = word | word "::" word ;                (* "module::name": the module's id *)
arguments  = argument { "," argument } [ "," ] ;
argument   = parameter ":" expression ;
parameter  = word { "." identifier } ;              (* param.half_life *)
expression = primary { "[" path "]" } ;              (* a selection *)
primary    = literal | variable | port | call ;
call       = function "(" [ arguments ] ")" ;
variable   = "{" key "}" | "{" string "}" | "{" id "}" ;
port       = "@" word "." parameter ;                (* "@node._ret": its return value *)
literal    = number | string | "true" | "false" | "()" | "none" | "some" "(" expression ")"
           | [ element ] "[" [ expression { "," expression } [ "," ] ] "]"
           | "uuid" "(" id ")" | "json" "(" string ")" ;
element    = "bool" | "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64"
           | "f32" | "f64" | "str" ;
word       = identifier | id ;
```

- An **identifier** is `[A-Za-z_][A-Za-z0-9_]*`; an **id** is a UUID,
  `5f726574-0000-4000-8000-000000000000`.
- A **number** is `-?digits[.digits][e±digits]`, or `NaN` or `inf`, with an
  optional type suffix: `1.5`, `100u8`, `-3i64`, `NaNf64`. `NaN` and `inf`
  are numbers, not names.
- A **string** is double-quoted, with the escapes Rust's `Debug` writes:
  `\n`, `\r`, `\t`, `\\`, `\"`, `\'`, `\0`, `\u{…}`.
- `true`, `false`, `none`, `some`, `uuid` and `json` are literals, not
  functions.
- A **key** in `{key}` holds no space, brace, quote or line break and does not
  start with `=`; any other key is written quoted, `{"a key"}`, with a
  string's escapes. `{<uuid>}` is the variable of that id, which names none;
  `{"<uuid>"}` is the key that reads as that id.
- A **path**, in a selection, runs from `[` to the first `]` on its line and
  may hold any other character: the path of `{pose}[.a#b(c]` is `.a#b(c`. It is a
  [`Key`](../../arora-types/src/data/state.rs) whose attributes, the
  `.`-separated parts after its first `.`, are read in turn
  ([`Key::select`](../../arora-types/src/data/state.rs)): a structure's field
  by the field's id, a key-value record's field by its key, an array's element
  by its index. `[.0]` is the first element and `[.name]` a record's field
  `name`; a path with no `.`, such as `[0]`, selects nothing out of its
  value and reads it whole.
- A **parameter's name** is words joined by dots, as a module may declare
  them (`param.half_life`). In `@node.parameter`, the parameter is everything
  after the first dot: a node's name is one word.
- A node's parentheses may be left out when it has no argument: `fallback`
  is `fallback()`. A call in an argument has them: `distance()`.

## Expressions

| Expression | Text | In the graph |
|---|---|---|
| a literal | `1.5`, `1.5f64`, `"text"`, `true`, `()`, `f64[1.0, 2.0]`, `[1.0, 2.0]`, `some(x)`, `none` | `Source::Value` |
| an id | `uuid(…)` | `Value(Value::Uuid)` |
| a structure, an enumeration | `json("…")`: the value's serde JSON | `Value` |
| a variable | `{person/visible}` (a store key), `{.mood}` (tree-local), `{"a key"}` (quoted), `{<uuid>}` | `Source::Key` |
| a node's return value | `@cm._ret` | `Source::Port` on `_ret` |
| a node's parameter | `@s.state` | `Source::Port` on the parameter |
| a selection | `@record._ret[.name]`, `{pose}[.0]`, `@step._ret[.<field id>]` | `Source::Select` |
| a call | `oscillator(t: {arora/time}, frequency: 1.6)` | a node of its own, read by its return value |

An argument binds its parameter, on the node it is written on: each graph
node holds its bindings (`Node::bind`).

The display writes two more forms, which a text does not: `{= value}`, a cell
the tree holds with no variable (a call's literal argument once the tree is
loaded), shown with the value it holds; and a module, function or parameter
computed rather than named, shown as its expression in parentheses. A text
using either is told so.

## Resolution

`text::resolve` turns a document into the shared `Graph`:

- **Functions.** A name is a native's (below), else a function of the index.
  Two modules' functions of one name are told apart by the module's id:
  `<module>::name`. A native's name is always the native's: a module function
  of the same name takes its module. An id names a function by its id; a
  function the index does not have is warned about, and its parameters are
  named by their ids.
- **Node ids** are UUID v5, under a fixed namespace, of the node's name, or
  of its path when it has none: `root`, `root.0.2` (the root's first child's
  third child), `detached.3` (the fourth top-level node after the root), and
  `<holder's path>.<parameter>` for a call in an argument (`detached.4.a`).
  A node named by an id has that id. An edit that keeps a node's name, or
  its place when it has none, keeps its id, so the interpreter's edit keeps
  its state.
- **Variables.** `{name}` binds the key `name`. A key starting with `.` is
  tree-local; any other is a store key under the Direct convention. `{<uuid>}`
  is a variable known by its id alone, which names no key: it binds the
  tree-local key `.<uuid>`.
- **Targets.** `{key} = f(…)` binds `_ret` (`5f726574-0000-4000-8000-000000000000`)
  to the key. `@n.p = f(…)` binds it to another node's input, which then
  reads this node.
- **Ports.** `@n.p` is node `n`'s parameter `p`, by name or id, and
  `@n._ret` its return value. A bare `@n` is an error: it names no port. `n`
  is a node of the text, by its name or its id. A node no node holds
  outputs its return value and the parameters it writes, and those alone are
  read from it; a tree node's argument is a cell any node reads.
- **Cycles.** Nodes no node holds that read one another in a cycle, or a
  node reading an output of its own, are an error: a value fed back goes
  through a store key.
- **Calls in arguments.** A binding holds no call: a call in an argument is a
  node no node holds, with the holder's path and the parameter for its path,
  read by its return value. A native is no such call.
- **Literals** take the type of the parameter they are given to: `100` given
  to an `f64` is `100f64`, given to a `u8` is `100u8`. An optional parameter
  takes its content's type. A literal stating its type keeps it, and it must
  match. A parameter of any type, such as `equal`'s, takes a literal that
  states its type: `b: true`, `b: 3f64`, not `b: 3`. A finite number past
  its type's range (`1e999f64`) is refused, not made infinite.
- **Natives' own state** is seeded when a node leaves it out:
  `sequence_star`'s `current_index` at `0u16`, `run_status`'s `latch` at `()`.
- **Inputs and outputs.** Which of a node's ports are inputs and which
  outputs, and their types, is its function's signature's to say: its
  parameters are inputs, its return value (`_ret`) and the parameters it
  writes are outputs. The graph holds the bindings alone.
- **Unstated parameters.** A native's parameters a node must bind are known,
  and leaving one out is warned about. A module function declares no
  default: a parameter a node leaves out is not in the call, which the
  function may mean (a module declared with `arora-module` reads it as `None`
  when it is optional), so it is not warned about.
- **One key, several writers.** A node no node holds publishes its keys
  after the tree ticks, so it wins over a tree node writing the same key;
  of several such nodes, the one with the greatest id publishes last, and
  wins, as lowering lists them by id, whatever their order in the text. Each
  writer of a store key that another node writes too is warned about, with
  the one that wins.

What does not resolve is reported where it is written, and left out of the
graph; the rest of the graph is there. An **error** says the text does not
say what to run: a function, parameter or node not found, a name given
twice (at both nodes), a literal of the wrong type or range, children under
a node that holds none, a read lowering refuses. A **warning** says the text
runs, though likely not as meant: a native's argument left out, a node that
never runs, a function the index does not have, a key several nodes write.

`Loaded::nodes` gives, for each graph node, its `id`, `name`, `span`, whether
the tree holds it (`attached`), its `function`, its `parent` (the node
holding it, or the node in whose argument its call is written) and its
`path`.

## Natives

The nodes the tree runs itself, in no module (their `module_id` is the nil
id):

| Name | Parameters | Children |
|---|---|---|
| `sequence`, `fallback`, `parallel` | — | yes |
| `sequence_star` | `current_index: u16` (its state) | yes |
| `succeed`, `fail`, `running` | — | no |
| `equal` | `a`, `b`: any value | no |
| `unpack` | `values`; `out_0` … `out_63`, written | no |
| `scatter` | `values`, `keys: str[]`, `within: str?` | no |
| `set_string` | `variable: str?` (written), `value: str?` | no |
| `unset_string` | `variable: str?` (written) | no |
| `is_string_set`, `wait_string_set` | `value: str?` | no |
| `regex_match` | `value: str?`, `matcher: str?`, `first_match: str?` (written) | no |
| `run_call` | `call` | no |
| `run_status` | `status` (written), `latch` (its state) | yes |

See [nodes.md](nodes.md) for what each does.

## Formatting

`text::format` writes a document in one layout:

- one line per node, its children two spaces under it, a node without
  arguments without parentheses;
- the root's tree, a blank line, then the other nodes; a blank line written
  between two nodes is kept, one however many were written;
- the arguments in the order of their function's parameters, when the
  function is known;
- literals as written: `1.5` stays `1.5`, `1.5f64` stays `1.5f64`; a key
  quoted when it needs it;
- comments where they were, above a node or at the end of its line; a node
  whose call holds comments is written one argument per line, each followed
  by the comments written after it.

Formatting a formatted text changes nothing. A text with errors is not
formatted.

## The display

`BehaviorTree::display` writes what the text reads back. A node, function or
parameter name that is not written as a name as it is — a space, a dash, a
letter past ASCII, `inf` — shows as its id, which reads back to the same
node, function or parameter; a module function whose name alone names a
native or another module's function shows as `<module>::name`. A key shows
quoted when it needs it. A float shows in its shortest form that reads back:
`1e300f64`, not three hundred digits.

## Which form to use

A behavior is written in three forms, one per kind of reader:

| Form | Use it for | Because |
|---|---|---|
| Text (`.bt`) | writing and reading behaviors | The tree shows as indentation and a binding as an argument, everything by name. |
| [Readable graph or diff](#readable-graphs-and-diffs), JSON or YAML | files people write, read or review in git — fixtures, examples, hand-made diffs — and tools working on a graph's structure | It is the shared model field by field, with a name wherever a person reads a reference. |
| Graph or diff by ids, JSON or YAML | the machine world: `LOAD`, `EDIT` and `SPAWN_GRAPH` payloads, `READ` answers, graphs programs exchange or store, and wherever a name may be ambiguous or renamed | An id means one thing on every device and after every rename. |

A payload to the interpreter module is by ids: whoever holds the function
index resolves names before sending. A name means what the functions the
device has make it mean when it reads it, so a payload by name could name one
function when it is written and another, or none, once a module is loaded or
dropped.

## Readable graphs and diffs

[`readable`](../src/readable.rs) mirrors the shared `Graph` and `GraphDiff`
field by field, every reference a string: a node's `id`, its `function`, the
ports its `bind` maps, its `children`, the `root`, the `node` and `port` of a
`port` source or of a diff's binding, the nodes a diff `remove`s. A string
that parses as a UUID is an id; any other is a name, and one file may mix
both. This is the graph of a text:

```text
watch: fallback
  calm: sequence
    near: below(x: @cm._ret, limit: 30)
    settle: smooth(x: @cm._ret, alpha: 0.5, state: {.s})
  done: succeed

cm: clamp(x: {sensor/distance}, lo: 0)
level: {face/level} = add(a: @cm._ret, b: 1)
```

```yaml
root: watch
nodes:
- id: watch
  function: fallback
  children: [calm, done]
- id: calm
  function: sequence
  children: [near, settle]
- id: near
  function: below
  bind:
    x: {port: {node: cm, port: _ret}}
    limit: {value: {f64: 30.0}}
- id: settle
  function: smooth
  bind:
    x: {port: {node: cm, port: _ret}}
    alpha: {value: {f64: 0.5}}
    state: {key: .s}
- id: done
  function: succeed
- id: cm
  function: clamp
  bind:
    x: {key: sensor/distance}
    lo: {value: {f64: 0.0}}
- id: level
  function: add
  bind:
    _ret: {key: face/level}
    a: {port: {node: cm, port: _ret}}
    b: {value: {f64: 1.0}}
```

and a diff to it, applied to that graph:

```yaml
remove: [level]
set:
- id: pulse
  function: mul
  bind:
    _ret: {key: face/pulse}
    a: {port: {node: cm, port: _ret}}
    b: {value: {f64: 2.0}}
unbind:
- {node: settle, port: state}
bind:
- {node: near, port: limit, source: {value: {f64: 20.0}}}
- {node: cm, port: hi, source: {value: {f64: 50.0}}}
```

- **Nodes.** A name is the id a text gives the node of that name, or the
  node at that path for a node with none (`root.0.2`): `cm` here is `cm:` in
  the text. A name in a reference names a node of the file. In a diff,
  `remove` names nodes of the graph the diff applies to, and every other
  reference a node of that graph it does not remove, or one it sets. An id is
  taken as it is.
- **Functions** resolve as in a text: a native's name, else a function of
  the index, `<module>::name` naming one of several modules' functions of
  that name.
- **Ports.** A name is the parameter of that name of the node's function,
  and `_ret` its return value. The ports of a node whose function the index
  does not have are named by their ids.

A reference that does not resolve is an error naming where the file writes
it — ``nodes[5].bind.low: `clamp` has no parameter `low` `` — and so is a name
naming several functions or ports, or a port bound twice. A file by ids
resolves with no function index.

| Function | Does |
|---|---|
| `readable::Graph::resolve(&self, &functions) -> Result<Graph, Vec<ReferenceError>>` | The graph a file describes, or every reference that does not resolve. |
| `readable::GraphDiff::resolve(&self, &base, &functions) -> Result<GraphDiff, Vec<ReferenceError>>` | The diff a file describes, `base` the graph it applies to. |
| `readable::Graph::from_graph(&graph, &names)` | A graph written with a name for each reference that has one. |
| `readable::GraphDiff::from_diff(&diff, &base, &names)` | A diff written so, `base` the graph it applies to. |

A file is read by deserializing it, in any serde format, into a
`readable::Graph` or `readable::GraphDiff`, then resolving it; a file by ids
reads the same way. `names` is a [`display::Names`](../src/display.rs): its
`functions` name the functions and ports, its `nodes` the nodes, such as a
text's `Loaded::nodes`. A reference is written as a name when that name
resolves back to the same id, and as its id otherwise, so a file written
reads back to the graph or diff exactly.

## Examples

The examples of [The NGBT spectrum](../../../docs/ngbt-spectrum.html), with
the operations it names (`clamp`, `remap`, `damp`, `oscillator`, `mul`,
`add`, `not`, `truthy`, `wait`, `identity`) as functions of the device's
modules, their parameters named for reading. The functions of
`@arora/dataflow` ([arora-dataflow-nodes](../../arora-dataflow-nodes/README.md))
name theirs after a node graph's ports and parameters, and take the clock and
their state as arguments:

```text
running

d: {face/out} = damp(in: {face/x}, param.half_life: 0.12f32, dt: {arora/dt}, state: ())
```

A pure mapping: sinks beside a tree that does nothing.

```text
running

amount: clamp(x: {face/smile}, min: 0, max: 1)
corner: remap(x: @amount._ret, in_min: 0, in_max: 1, out_min: -0.2, out_max: 1)
mouth: {rig/mouth_corner} = damp(target: @corner._ret, half_life: 0.12)
cheeks: {rig/cheek_raise} = mul(a: @amount._ret, b: 0.6)
```

Breathing, driven by time: the wave runs every tick, the amplitude and the
flush only when `face/energy` changes.

```text
running

wave: oscillator(t: {arora/time}, frequency: {face/breath_rate})
amplitude: remap(x: {face/energy}, in_min: 0, in_max: 1, out_min: 0.03, out_max: 0.2)
chest: {rig/chest} = add(a: mul(a: @wave._ret, b: @amplitude._ret), b: 0.5)
flush: {face/flush} = remap(x: {face/energy}, in_min: 0, in_max: 1, out_min: 0.15, out_max: 0.85)
```

Greet, then rest: control around dataflow. A leaf whose return value goes to
a key writes it each time it ticks, and succeeds.

```text
fallback                                   # greet or rest
  sequence                                 # greet
    truthy(x: {person/visible})
    truthy(x: @fresh._ret)                 # not greeted yet
    parallel                               # wave until the timer ends
      {rig/arm_raise} = damp(target: 1, half_life: 0.12, initial: {rig/arm_raise})
      {rig/arm_wave} = mul(a: oscillator(t: {arora/time}, frequency: 1.6), b: 0.6)
      wait(seconds: 2)
    {social/greeted} = identity(x: true)
  parallel                                 # rest
    {rig/arm_raise} = damp(target: 0, half_life: 0.2, initial: {rig/arm_raise})
    {rig/arm_wave} = damp(target: 0, half_life: 0.2, initial: {rig/arm_wave})

fresh: not(x: {social/greeted})
```
