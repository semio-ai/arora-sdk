# `vizij-orchestrator-composed`

This module is the migration target beside the compatibility
`vizij-orchestrator` module. The compatibility module wraps
`vizij-orchestrator-core` as one all-in-one runtime. This module keeps the same
JSON facade style but composes the promoted `vizij-animation` and
`vizij-node-graph` modules through Arora imports.

The composed module calls the promoted domain modules through the generated
Arora module import wrappers declared in `module.yaml`. In the browser those
imports are wired by the Wasm executor. On desktop and service hosts, the native
executor installs an equivalent host dispatcher into the loaded cdylib so the
same import wrappers call back through the owning Arora engine.

## Desktop-Native Execution

The generated module header still declares the `wasm` executor because the
browser path consumes it directly. Desktop hosts can load the native cdylib with
Arora's native executor by overriding the executor at load time.

```bash
cargo build -p vizij-animation -p vizij-node-graph -p vizij-orchestrator-composed
cargo test -p arora-integration-tests call_vizij_composed_native_module_from_desktop_engine -- --nocapture
```

For manual CLI checks, pass the native library and override the executor:

```bash
cargo build -p vizij-animation -p vizij-node-graph -p vizij-orchestrator-composed

target/debug/arora-cli \
  --header modules/vizij-animation/src/arora_generated/module.yaml \
  --exe target/debug/libvizij_animation.so \
  --header modules/vizij-node-graph/src/arora_generated/module.yaml \
  --exe target/debug/libvizij_node_graph.so \
  --header modules/vizij-orchestrator-composed/src/arora_generated/module.yaml \
  --exe target/debug/libarora_vizij_orchestrator_composed.so \
  --executor-override native \
  --call "id: 90725b7e-a4d9-4a3f-99af-8e227612bed7
args:
- id: 323d47be-3b30-46ff-882f-bc7f7ffacd57
  value:
    str: '{\"call\":\"runtime.create\",\"requestId\":\"desktop-native\",\"args\":{\"schedule\":\"SinglePass\"}}'"
```

The domain module headers are included first so the CLI's local registry can
resolve imports before the composed module is loaded. The composed native
runtime then calls `vizij-animation` and `vizij-node-graph` through Arora's
native module boundary. Use `.dylib` on macOS and `.dll` on Windows.

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

## Local Vizij Rust Dependency

This module currently depends on the sibling Vizij Rust experiment worktree:

```text
../../../vizij-rs-vizij-engine-backend-experiment
```

See [`../../docs/vizij-engine-backend-experiment.md`](../../docs/vizij-engine-backend-experiment.md)
for the full three-repo checkout layout and instructions for replacing this
relative path if your checkout names differ.
