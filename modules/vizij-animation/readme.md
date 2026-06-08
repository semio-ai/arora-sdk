# `vizij-animation`

This module exposes the existing Vizij animation core through Arora's module
contract. It is intentionally a facade: animation parsing, migration, sampling,
player state, and write-batch generation stay in `vizij-animation-core`.

## Exports

All exported functions use Arora `str` values as their request/response shape so
the first integration slice can carry current Studio/Vizij animation JSON without
adding new Arora schema records.

Each response is JSON:

```json
{ "ok": true, "value": {} }
```

or:

```json
{ "ok": false, "error": "message" }
```

| Function | Request | Response value |
| --- | --- | --- |
| `reset_engine` | no request | `{ "reset": true }` |
| `load_stored_animation` | `{ "animation": StoredAnimation }` or direct `StoredAnimation` | `{ "animationId": number }` |
| `create_player` | `{ "name": string }` | `{ "playerId": number }` |
| `add_instance` | `{ "playerId": number, "animationId": number, "config"?: InstanceCfg }` | `{ "instanceId": number }` |
| `update_nodes_writes` | `{ "dt": number, "inputs"?: Inputs }` | `{ "nodes": {}, "writes": WriteBatchJSON }` |
| `list_animations` | no request | animation metadata array |

`add_instance.config` accepts the core `InstanceCfg` field names and Studio-shaped
instance settings. Studio `timescale` maps to core `time_scale`, Studio `offset`
is interpreted as milliseconds and converted to core `start_offset` seconds, and
Studio `active` maps to core `enabled`.

## Local Build

This module currently depends on the sibling Vizij Rust experiment worktree:

```text
../../../vizij-rs-vizij-engine-backend-experiment
```

That keeps this branch as a local integration experiment without copying Vizij
runtime logic into Arora.

See [`../../docs/vizij-engine-backend-experiment.md`](../../docs/vizij-engine-backend-experiment.md)
for the full three-repo checkout layout and instructions for replacing this
relative path if your checkout names differ.

```bash
cargo +nightly test
cargo +nightly build
```

For Wasmtime/WASI execution, install the nightly WASI target and build:

```bash
rustup +nightly target add wasm32-wasip1
cargo +nightly build --target wasm32-wasip1
```
