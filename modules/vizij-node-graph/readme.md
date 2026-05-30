# `vizij-node-graph`

This module exposes the Vizij node graph runtime through Arora's module contract.
It is the graph peer to `vizij-animation`: graph parsing, normalization, staged
inputs, persistent node state, and write-batch generation stay in
`vizij-graph-core`.

## Exports

All functions use Arora `str` values as their request/response shape.

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
| `reset_graph` | no request | `{ "reset": true }` |
| `load_graph` | `{ "spec": GraphSpec }` or direct `GraphSpec` | `{ "loaded": true }` |
| `stage_input` | `{ "path": string, "value": ValueJSON, "shape"?: ShapeJSON }` | `{ "path": string }` |
| `evaluate` | `{ "dt": number }` | `{ "nodes": {}, "writes": WriteBatchJSON }` |
| `normalize_graph` | `{ "spec": GraphSpec }` or direct `GraphSpec` | normalized `GraphSpec` |

## Local Build

This module currently depends on the sibling Vizij Rust experiment worktree:

```text
../../../vizij-rs-vizij-engine-backend-experiment
```

```bash
cargo +nightly test -p vizij-node-graph
cargo +nightly build -p vizij-node-graph --target wasm32-wasip1
```
