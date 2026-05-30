# `vizij-orchestrator-composed`

This module is the migration target beside the compatibility
`vizij-orchestrator` module. The compatibility module wraps
`vizij-orchestrator-core` as one all-in-one runtime. This module keeps the same
JSON facade style but composes the promoted `vizij-animation` and
`vizij-node-graph` module facades internally.

The first version intentionally uses in-process Rust facade calls so the browser
and Arora Web path can load one composed orchestrator module. That is the
stepping stone toward replacing the in-process calls with real Arora
module-to-module imports once that contract is ready.

## Export

```text
dispatch_json(request_json: str) -> str
```

Initial calls include:

- `runtime.create`
- `controllers.list`
- `graph.normalize`
- `graph.register`
- `graph.remove`
- `animation.register`
- `animation.remove`
- `input.set`
- `input.remove`
- `orchestrator.step`

## Current Scope

`SinglePass` order is animation modules first, then graph modules. `TwoPass`
adds a graph pass before the animation pass, then repeats graph evaluation after
animation writes. This mirrors the existing orchestrator's pass shape while the
module split matures.

Conflict diagnostics are currently minimal in the composed version: writes are
applied in pass order with later writes replacing the blackboard value.
