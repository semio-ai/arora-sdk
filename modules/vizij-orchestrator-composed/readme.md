# `vizij-orchestrator-composed`

This module is the migration target beside the compatibility
`vizij-orchestrator` module. The compatibility module wraps
`vizij-orchestrator-core` as one all-in-one runtime. This module keeps the same
JSON facade style but composes the promoted `vizij-animation` and
`vizij-node-graph` module facades internally.

On `wasm32`, the composed module calls the promoted domain modules through the
generated Arora module import wrappers declared in `module.yaml`. On native
targets, it uses the same Rust-backed facades in process so the core
implementations remain usable outside the browser.

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

The compatibility module remains available for the all-in-one fallback path,
while this module is the preferred target for proving animation and graph
execution as independent first-class Arora modules.
