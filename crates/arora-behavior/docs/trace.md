# Traces

A **trace** reports what a running behavior does, tick by tick: which nodes
the tick reached and what each returned, which detached nodes it called, and
the values the nodes a client watches output. A behavior editor uses one to
show a behavior running: it highlights the nodes as they run and shows the
values on their ports.

A trace is a [task run](../readme.md#task-runs). It starts with a call, answers
a `TaskHandle`, reports on the handle's feedback key, and ends with the halt
that ends any run. An interpreter that cannot trace refuses the call.

## Starting and stopping a trace

`BehaviorInterpreter::trace(TraceOptions)` starts one. A client reaches it
through the interpreter module's `TRACE` function, with the options as its one
argument (`interpreter_module::encode_trace`); `decode_spawn_result` reads the
handle it answers. An `arora` device's `Caller::trace` does both.

| option | default | meaning |
|---|---|---|
| `values` | none | the nodes whose outputs the reports carry |
| `min_interval_ns` | none | the least time between two reports, in nanoseconds of `arora/time` |

With the defaults, a trace reports statuses and detached activity at every
tick that changes one. Several traces may run at once, each with its own
options: one editor following every tick, another watching a few values at
30 Hz.

The handle's keys follow a spawned run's layout, under
`arora/tasks/<interpreter module id>/<TRACE function id>/<run id>/`:

| key | holds |
|---|---|
| `status` | `Running` from the trace's first report; `Failure` once it is halted |
| `feedback` | the latest report |
| `result`, `update` | nothing: a trace has no result and takes no steering |

`HALT` with the trace's id, the handle's `stop` call, ends it. Its status key
goes `Failure` at the next tick, as a halted run's does, and its keys change
no more. A trace survives `load` and `apply`: it follows the interpreter, not
one behavior.

## The report

At the end of each tick, a trace whose report is due writes one to its
feedback key if something it follows changed since its previous report. Its
first report is written at the first tick after it starts, whatever it holds.
A report is a `TraceReport`, written as a structured value
(`trace::encode_report`, read back with `trace::decode_report`):

| field | holds |
|---|---|
| `tick` | the interpreter's tick the report was written at, counted from its first tick |
| `since` | the tick of the trace's previous report; absent in its first |
| `status` | node id → its new status, for each node whose status changed |
| `activity` | node id → its new activity, for each detached node whose activity changed |
| `values` | node id → port id → its new value, for each output of a watched node that changed |
| `restarted` | the nodes an edit restarted since the previous report |

Every field but `tick` is left out when it is empty.

- **A status** is `success`, `failure` or `running`, what the node returned
  when the tick reached it, or `not_reached`: the tick did not tick it. A node
  whose tick fails with an error reports `failure`; the error is the tick's.
- **An activity** is that of a detached node, a node no other node holds,
  evaluated when what it outputs is read: `called` (its arguments changed and
  it called its function), `cached` (it was read and answered from its last
  call) or `idle` (nothing read it).
- **A value** is an output of a watched node: its return value, under the id
  of `_ret` (`5f726574-0000-4000-8000-000000000000`), whether or not a binding
  keeps it, and each parameter it writes, under the parameter's id. A status
  is no value: the node's status reports it. An output that became unset reads
  as the unit value.
- **A restarted node** is one an edit set or rebound: it starts afresh, and
  the report states its status, activity and values anew, even when they equal
  what was reported before.

A change is a difference from what the trace last reported. The first report
states every node the tick reached, every detached node it read, and every
output of a watched node that is set. A node the first report does not name is
not reached, and a detached node it does not name is idle. Between two reports
of a paced trace, the next report states each node as it stands then: a node
that changed and changed back is not reported.

### Reading it

The feedback key holds the report as a structured value, converted with
`arora_types::value_serde`: a key-value whose fields carry the names above,
`status`, `activity` and `values` each a key-value keyed by node id (and
`values` by port id within it), a status or an activity as its name in a
string, and a watched value as itself. A bridge relays it in the value
vocabulary like any other key; a client holding arora-behavior (a Rust
program, a wasm page) reads it with `trace::decode_report`.

A decoded `TraceReport` serializes to JSON as a page uses it:

```json
{
  "tick": 42,
  "status": {
    "0195e1f2-0000-7000-8000-000000000001": "running",
    "0195e1f2-0000-7000-8000-000000000002": "success",
    "0195e1f2-0000-7000-8000-000000000003": "not_reached"
  },
  "activity": {
    "0195e1f2-0000-7000-8000-000000000004": "cached"
  },
  "values": {
    "0195e1f2-0000-7000-8000-000000000004": {
      "5f726574-0000-4000-8000-000000000000": {"f64": 0.25}
    }
  }
}
```

## How an editor uses it

1. When the user starts playback, the editor calls `TRACE` with the nodes it
   shows values for and the refresh it wants, and keeps the handle.
2. It follows the handle's feedback key. Each report is one tick of its view:
   it sets each listed node to its new status, activity or value, keeps every
   other node as it was, and clears what it shows for each `restarted` node
   before applying the report. A report whose `tick` is more than one past the
   previous one means the ticks between changed nothing, or that the trace is
   paced. A report whose `since` is not the `tick` of the last report the
   editor read follows one it missed: the editor halts the trace and starts a
   new one, whose first report gives the whole picture.
3. When the user stops, the editor calls the handle's `stop`. The status key
   going `Failure` confirms the trace ended.

A client that reconnects starts a new trace: the first report gives it the
whole picture, with no history to replay. A report that the interpreter
fails to write leaves the trace to start over: its next report is a first
one, with no `since`, stating everything.

## Cost

With no trace running, a tick tests one flag per node it ticks. While traces
run, each node the tick reaches records its status, and each detached node it
reads records its activity; at the end of the tick the interpreter compares
that with the previous tick, and hands each trace what changed. The work
follows the nodes the tick reaches and what changed, not the size of the
graph; a watched node's outputs are read only when a report is written.
