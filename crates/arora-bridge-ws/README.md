# arora-bridge-ws

The open local bridge for Arora: a WebSocket server that bridges the Arora API,
implementing [`arora_bridge::Bridge`], for editors and apps on trusted local
links.

An Arora device is one blackboard with four seams around it (store, HAL,
bridge, behavior). This crate is a **bridge** implementation whose remote is a
local app — a rig editor, a control panel, a debugging tool — rather than
Semio Studio over the network. Messages speak the data-layer vocabulary:
clients **write** and **read** values at **keys** (hierarchical paths into the
store, e.g. `face/mouth`), list the available keys, and call the device's
methods by name.

## Wire format

JSON messages with a `type` field discriminator, over a WebSocket:

| Client → Server | Reply | Meaning |
| --- | --- | --- |
| `{"type": "write_values", "values": {"face/mouth": {"f64": 0.5}}}` | `write_values_resp` | Write values to keys |
| `{"type": "read_values", "keys": ["face/mouth"]}` | `read_values_resp` | Read current values |
| `{"type": "list_keys", "path": "face"}` | `list_keys_resp` | List the device's keys (optionally under a prefix) |
| `{"type": "list_methods"}` | `list_methods_resp` | List the callable methods |
| `{"type": "invoke", "method": "say", "args": {"text": {"str": "hi"}}}` | `invoke_resp` | Call a method by name |
| `{"type": "halt", "run": "<run id>"}` | `halt_resp` | Stop a run |
| `{"type": "subscribe", "keys": ["face/mouth"]}` | `subscribe_resp` | Choose which keys are pushed |

The server also pushes `{"type": "values_changed", "values": {...}}`
unsolicited whenever the runtime writes new state — the live feed a connected
editor renders from. A client is pushed every key until it subscribes, and only
the keys it named afterwards: nothing about a key makes it special, so a client
that wants the device's clock subscribes to it like any other.

## Keys

`list_keys` answers from the device, asked as the client asks: every key it holds
right now, each with the `KeyMeta` its store keeps —

```json
{"path": "face/mouth", "__meta": {"ty": "f64", "min": 0.0, "max": 1.0,
                                  "default": {"f64": 0.0}, "editable": true,
                                  "description": "how open the mouth is"}}
```

— so a slider knows its range without anyone restating it here, and a module
loaded while the device runs is listed at once. A key nobody has described
carries the default meta: the shape of the value it holds, and closed to writes.

A write reaches the device, which accepts it only for the keys it opened
(`editable: true`, by key or by subtree in its store) and refuses the rest,
naming the path. A device that opens nothing accepts no writes: a client on an
unauthenticated link does not get to set a key the device never offered.

## Methods and runs

`list_methods` answers with the methods the server itself registered *and* the
device's own module functions, each under its declared name with the parameters
and value shapes of its described signature — nothing mirrors them here.
`invoke` binds its `args` to those parameters by name; an argument the signature
does not name fails the call rather than being dropped.

A method that reports a behavior status is a **run**: long-running and
cancellable (`"task": true` in its description). Invoking it answers at once with
the run, by name —

```json
{"type": "invoke_resp", "success": true, "value": {"keyvalue": {"fields": {
  "run":    {"name": "run",    "value": {"str": "0195e1f2-…"}},
  "status": {"name": "status", "value": {"str": "arora/tasks/…/status"}},
  "feedback": {"name": "feedback", "value": {"strs": []}},
  "result":   {"name": "result",   "value": {"strs": []}},
  "update":   {"name": "update",   "value": {"strs": []}}
}}}}
```

— so a client watches `status` for the outcome (subscribe to it) and stops the
run with `{"type": "halt", "run": "0195e1f2-…"}`.

## Pieces

- `AroraWSServer` — the ready-to-use server. Binds loopback by default: the
  link is unauthenticated, so exposing other interfaces is an explicit opt-in.
  One active client at a time; a new connection replaces the old one.
- `Device` — the device behind the server, and the only thing a client
  discovers: its keys with their meta, its functions with their signatures, and
  the calls it answers. The server keeps nothing of its own.
- `bridge::WsBridge` — drives the server as an Arora `Bridge`: incoming
  writes/reads become `BridgeCommand`s for the runtime, the runtime's state flows
  out as `values_changed`, and the device's methods are described and called
  through the same channel.
- A built-in control panel served on plain HTTP from the same port (opt-in via
  `ServerConfig::serve_control_panel`): sliders over the advertised input keys,
  and a row per method with its arguments, a call and a stop; the device's other
  keys are listed read-only beneath, with the value they held when listed.

## Example

```rust,no_run
use arora_bridge_ws::{AroraWSServer, CancellationToken};

#[tokio::main]
async fn main() {
    let server = AroraWSServer::with_port(9000);
    server.set_write_values_handler(|values| {
        println!("{} values written", values.len());
        Ok(())
    }).await;
    server.run(CancellationToken::new()).await.unwrap();
}
```

To serve a runtime instead of raw handlers, wrap the server in
`WsBridge::new(server)` and hand it to the runtime as its bridge.
