# `vizij-orchestrator`

This module exposes the existing Vizij orchestrator runtime through Arora's
module contract. It intentionally wraps `vizij-orchestrator-core` instead of
copying graph, animation, scheduling, or blackboard semantics into the engine
repo.

This is the all-in-one compatibility orchestrator. The split-module migration
target lives beside it as `vizij-orchestrator-composed`, which composes the
promoted `vizij-animation` and `vizij-node-graph` module facades.

## Export

The module exports one function:

```text
dispatch_json(request_json: str) -> str
```

Requests and responses use the shared Vizij module-facade JSON contract:

```json
{
  "call": "runtime.create",
  "requestId": "r1",
  "args": { "schedule": "SinglePass" }
}
```

Responses are JSON:

```json
{ "ok": true, "result": {}, "version": 1, "requestId": "r1" }
```

or:

```json
{ "ok": false, "error": "message", "version": 1, "requestId": "r1" }
```

Initial calls include:

- `runtime.create`
- `runtime.dispose`
- `controllers.list`
- `graph.register`
- `graph.replace`
- `graph.merge`
- `graph.remove`
- `animation.register`
- `animation.remove`
- `input.set`
- `input.remove`
- `orchestrator.step`
- `orchestrator.stepDelta`

## Local Build

This module currently depends on the sibling Vizij Rust experiment worktree:

```text
../../../vizij-rs-vizij-engine-backend-experiment
```

That keeps this branch as a local integration experiment without changing Arora
engine core or copying Vizij runtime logic.

See [`../../docs/vizij-engine-backend-experiment.md`](../../docs/vizij-engine-backend-experiment.md)
for the full three-repo checkout layout and instructions for replacing this
relative path if your checkout names differ.

```bash
cargo +nightly test
cargo +nightly build --target wasm32-wasip1
```
