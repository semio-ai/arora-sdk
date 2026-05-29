# `vizij-orchestrator`

This module exposes the existing Vizij orchestrator runtime through Arora's
module contract. It intentionally wraps `vizij-orchestrator-core` instead of
copying graph, animation, scheduling, or blackboard semantics into the engine
repo.

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

```bash
cargo +nightly test
cargo +nightly build --target wasm32-wasip1
```
