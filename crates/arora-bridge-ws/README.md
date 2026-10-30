# arora-bridge-ws

The open local bridge of [Arora](https://github.com/semio-ai/arora-sdk): a
WebSocket server through which an app on the same machine — an editor, a
control panel, a script — reads and writes a running device's keys and calls
its functions. It implements the bridge interface,
[`arora_bridge::Bridge`](https://docs.rs/arora-bridge), with JSON messages.

## The open local bridge

A device built with [`arora`](https://docs.rs/arora) serves this bridge by
default, on `ws://127.0.0.1:9000`. The link is unauthenticated, so the server
binds the loopback interface unless its embedder opens another, and the device
accepts a write only to the keys it opened.

The server relays the device and keeps nothing of its own: the keys and their
meta come from the device's store, the methods from its modules, each asked
when the client asks, so a module loaded while the device runs is listed at
once. The server serves one client at a time; a new connection replaces the
previous one.

## Smallest use

Run a device, then talk to it from any WebSocket client. From a browser
console:

```js
const ws = new WebSocket("ws://127.0.0.1:9000");
ws.onmessage = (event) => console.log(JSON.parse(event.data));
ws.onopen = () => ws.send(JSON.stringify({ type: "list_keys" }));
```

The answer lists the device's keys, and from then on the server pushes
`values_changed` with every key the device changes.

## Messages

Each message is a JSON object whose `type` names it. Values are Arora `Value`s
in their JSON form: `{"f64": 0.5}`, `{"str": "hi"}`, `{"bool": true}`,
`{"f32s": [0, 1, 0]}`.

| Client sends | Server answers | What it does |
|---|---|---|
| `{"type": "list_keys", "path": "face"}` | `list_keys_resp` | The device's keys (all, or those whose path starts with `path`), each with its meta. |
| `{"type": "read_values", "keys": ["face/mouth"]}` | `read_values_resp` | The current values of the keys. |
| `{"type": "write_values", "values": {"face/mouth": {"f64": 0.5}}}` | `write_values_resp` | Writes the keys, all or none. |
| `{"type": "subscribe", "keys": ["face/mouth"]}` | `subscribe_resp` | Chooses the keys this connection is pushed; `null` or absent means every key. |
| `{"type": "list_methods"}` | `list_methods_resp` | The device's callable methods, with their parameters. |
| `{"type": "describe_methods", "path": "dou"}` | `describe_methods_resp` | The same methods (all, or those whose name starts with `path`), each with its full signature and the ids a call names. |
| `{"type": "invoke", "method": "say", "args": {"text": {"str": "hi"}}, "request_id": "r1"}` | `invoke_resp` | Calls a method by name. |
| `{"type": "call", "module_id": "…", "id": "…", "args": […], "request_id": "r3"}` | `call_resp` | Calls a function by ids. |
| `{"type": "halt", "run": "<run id>", "request_id": "r2"}` | `halt_resp` | Stops a run. |

Unsolicited, the server pushes `{"type": "values_changed", "values": {…}}`
with what changed at each step of the device. A connection is pushed every key
until it subscribes, then only the keys it named; no key is special, so a
client that wants the device's clock subscribes to it like any other. A
message the server cannot parse is answered with `{"type": "error",
"message": …}`.

## Keys and their meta

`list_keys_resp` carries each key with the `KeyMeta` the device's store keeps
for it, under `__meta`:

```json
{"path": "face/mouth", "__meta": {"ty": "f64", "min": 0.0, "max": 1.0,
                                  "unit": "fraction", "default": {"f64": 0.0},
                                  "editable": true,
                                  "description": "how open the mouth is"}}
```

so a slider knows its range and its unit without anyone restating them. A key
nothing described carries only the type of the value it holds, and is closed.

A write is accepted only for the keys the device opened (`editable`, by key or
by subtree in its store), and only with values of the type their meta states:
`{"f32": 0.5}` sent to an `f64` key is refused, naming the key and both types.
A refused write writes none of its keys, and `write_values_resp` carries
`"success": false` with the reason. A device that opens nothing accepts no
writes.

## Methods and runs

`list_methods_resp` lists each of the device's functions under the name its
module declares, with its parameters (`name`, `param_type`, `required`) in
declaration order, its `return_type`, and `task`. `invoke` binds its `args` to
those parameters by name; an argument the method does not declare fails the
call. When two modules export one name, `invoke` fails naming them.

A method with `"task": true` returns the behavior `Status`: it is a **run**,
long-running and cancellable. Invoking it starts the run and answers at once
with its handle, by name:

```json
{"type": "invoke_resp", "success": true, "value": {"keyvalue": {"id": "…", "fields": {
  "run":    {"name": "run",    "value": {"str": "0195e1f2-…"}},
  "status": {"name": "status", "value": {"str": "arora/tasks/…/status"}},
  "feedback": {"name": "feedback", "value": {"strs": []}},
  "result":   {"name": "result",   "value": {"strs": []}},
  "update":   {"name": "update",   "value": {"strs": []}}
}}}}
```

A client follows the run on its `status` key (subscribe to it) and stops it
with `{"type": "halt", "run": "0195e1f2-…"}`.

## Signatures and calls by ids

`describe_methods` and `call` let a client that holds no Rust code drive the
device as an in-process caller (`arora::LocalCaller`) does: resolve a behavior
against the device's functions, then load it.

`describe_methods_resp` lists each method as `arora_bridge::MethodSignature`
serializes it: the exporting module's id, the function's id, its name, and its
frozen function record — parameters by id, their order, the return type:

```json
{"type": "describe_methods_resp", "methods": [
  {"module_id": "…", "id": "…", "name": "double",
   "function": {
     "parameters": {"<x id>": {"name": "x", "mutable": false,
                               "type": {"type": "primitive", "value": {"kind": "f64"}}}},
     "parameterOrdering": ["<x id>"],
     "returnType": {"type": "primitive", "value": {"kind": "f64"}}}}
]}
```

It lists what the device's method index describes, as `list_methods` does:
its modules' described functions and its behavior interpreter's task-run
functions (`spawn`, `spawn_graph`, `halt`). The interpreter's `load` and `edit`
are not described; a client calls them by their fixed ids
(`arora_behavior::interpreter_module::{ID, LOAD, EDIT}`).

`call` carries an `arora_types::call::Call` — `module_id`, `id`, and `args`,
each argument a parameter id and a value — beside the message's own fields:

```json
{"type": "call", "module_id": "…", "id": "…",
 "args": [{"id": "<x id>", "value": {"f64": 3.0}}], "request_id": "r3"}
```

The answer carries the `CallResult` — the return value and the mutated
parameters — or the device's reason for refusing it:

```json
{"type": "call_resp", "success": true, "request_id": "r3", "ret": {"f64": 6.0}, "mutated": []}
{"type": "call_resp", "success": false, "request_id": "r3", "message": "call failed: …"}
```

A call is made as it is sent: nothing binds names, and a task-shaped function
is called once, not started as a run. To start a run, call the interpreter's
`spawn` with the call to run; its `ret` is the run's handle, and `halt` stops it.

## Who may do what

The bridge is unauthenticated, so the server binds the loopback interface
unless its embedder opens another: whoever reaches the port is trusted as a
local app.

- **A write** reaches only the keys the device opened (`editable` in their
  meta) with values of their declared type, and is refused whole otherwise.
  The device checks it as it checks every bridge's write.
- **A call**, by name (`invoke`) or by ids (`call`), reaches any function the
  device dispatches — its modules' and its behavior interpreter's, described or
  not — exactly as a call from the in-process caller or from any other bridge
  does. The bridge adds no rule of its own. A call is not a write: a behavior
  it loads or a run it starts writes keys as the device's own behavior, not as
  a remote writer.

## Serving a device

The `arora` crate attaches this bridge when a device is given no other, and
offers it composed: `arora::local_ws_bridge()`, `local_ws_bridge_with(config)`
for a `ServerConfig` of your own, and `serve_local_ws_bridge(server)` for a
server you keep. Without `arora`, wrap a server in `bridge::WsBridge` — which
turns each message into a `BridgeCommand` on its inbound stream and each
outbound change into a `values_changed` push — and run the server yourself:

```rust,no_run
use std::sync::Arc;
use arora_bridge_ws::{bridge::WsBridge, AroraWSServer, CancellationToken, ServerConfig};

# async fn serve() {
let server = Arc::new(AroraWSServer::new(ServerConfig::with_port(9000)));
let bridge = WsBridge::new(server.clone()).await;
tokio::spawn(async move { server.run(CancellationToken::new()).await });
// Hand `bridge` to the device: arora::Arora::builder().with_bridge(Box::new(bridge))
# }
```

The inbound stream ends when the server stops, which the device sees as the
endpoint disconnecting.

Underneath, the server dispatches to handlers an embedder can set directly —
`set_write_values_handler`, `set_read_values_handler`, and `set_device` with a
`Device` (keys, methods, invoke, halt, signatures, calls by ids) — which is
what `WsBridge` sets.

## Reference

`ServerConfig`:

| Field | Default | Meaning |
|---|---|---|
| `port` | `9000` | The port served. |
| `bind_address` | `127.0.0.1` | The interface bound. Binding another one exposes an unauthenticated link to that network. |
| `serve_control_panel` | `false` | Serve a control panel on plain HTTP on the same port: a slider per input key, a row per method with its arguments, a call and a stop, and the other keys read-only. |

Limits: a message is at most 1 MiB, a frame 256 KiB. The server answers
WebSocket pings.

A device built with `arora` takes the port from `--bridge-port`
(`ARORA_BRIDGE_PORT`), and `arora --open` opens every key that has no meta of
its own but the runtime's `arora/` keys, for a sandbox or a bench.
