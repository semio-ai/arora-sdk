# arora-websocket

The open local bridge for Arora: a WebSocket server that bridges the Arora API,
implementing [`arora_bridge::Bridge`], for editors and apps on trusted local
links.

An Arora device is one blackboard with four seams around it (store, HAL,
bridge, behavior). This crate is a **bridge** implementation whose remote is a
local app — a rig editor, a control panel, a debugging tool — rather than
Semio Studio over the network. Messages speak the data-layer vocabulary:
clients **write** and **read** values at **keys** (hierarchical paths into the
store, e.g. `face/mouth`), list the available keys, and invoke registered RPC
methods.

## Wire format

JSON messages with a `type` field discriminator, over a WebSocket:

| Client → Server | Reply | Meaning |
| --- | --- | --- |
| `{"type": "write_values", "values": {"face/mouth": {"f64": 0.5}}}` | `write_values_resp` | Write values to keys |
| `{"type": "read_values", "keys": ["face/mouth"]}` | `read_values_resp` | Read current values |
| `{"type": "list_keys", "path": "face"}` | `list_keys_resp` | List available keys (optionally under a prefix) |
| `{"type": "list_methods"}` | `list_methods_resp` | List registered RPC methods |
| `{"type": "invoke", "method": "reset", "request_id": "req-1"}` | `invoke_resp` | Invoke a method |

The server also pushes `{"type": "values_changed", "values": {...}}`
unsolicited whenever the runtime writes new state — the live feed a connected
editor renders from.

## Pieces

- `AroraWSServer` — the ready-to-use server. Binds loopback by default: the
  link is unauthenticated, so exposing other interfaces is an explicit opt-in.
  One active client at a time; a new connection replaces the old one.
- `Registry` — advertises the keys (`KeyInfo`) and methods (`MethodInfo`)
  clients can discover with `list_keys` / `list_methods`.
- `bridge::WsBridge` — drives the server as an Arora `Bridge`: incoming
  writes/reads become `BridgeCommand`s for the runtime, and the runtime's
  `send_data` flows out as `values_changed`.
- A built-in control panel (sliders over the advertised input keys) served on
  plain HTTP from the same port, opt-in via `ServerConfig::serve_control_panel`.

## Example

```rust,no_run
use arora_websocket::{AroraWSServer, CancellationToken};

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
