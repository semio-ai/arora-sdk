# arora-web

`wasm-bindgen` entry point that hosts the Arora engine inside a browser.
Compiles `arora` (with `--no-default-features`, so no wasmtime, no
libloading) to `wasm32-unknown-unknown` and exposes a small JS-facing
`Engine` class that loads guest modules through the browser's native
`WebAssembly` runtime.

JS surface (see [src/lib.rs](src/lib.rs)):

```ts
class Engine {
  constructor();
  loadModule(headerJson: string, executable: Uint8Array): string; // returns module id
  call(callJson: string): string;                                   // returns result JSON
}

class BehaviorTreeRunner {
  constructor();
  loadModule(headerJson: string, executable: Uint8Array): string;
  setVariable(varId: string, valueJson: string): void;
  tick(nodesJson: string): string;  // returns {status, trace, variables}
  run(nodesJson: string): string;   // runs until not Running
}
```

`callJson` matches `arora::call::Call`:
`{"id":"<function-uuid>","args":[...]}`. If the call doesn't carry a
`module_id`, arora-web looks one up from the function ID it was loaded
with.

Each node in `nodesJson` is:
```json
{
  "id": "<uuid>",
  "function": "<fn-uuid>",
  "children": ["<child-uuid>"],
  "arguments": { "<param-uuid>": {"value": {...}} | {"variable_id": "<uuid>"} },
  "return_binding": "<var-uuid>"
}
```
When `return_binding` is set the function's raw return value is stored in
that variable and the node always reports `success` to its parent.

## Build

```bash
wasm-pack build crates/arora-web --target web --dev
```

Output lands under `crates/arora-web/pkg/`. To consume from a bundler
swap `--target web` for `--target bundler`.

## Integration test (headless browser)

```bash
# First, force a wasm32-wasip1 build of the test guest module:
cargo test -p arora-integration-tests

# Then run the wasm-bindgen-test in a headless browser:
GECKODRIVER=$(which geckodriver) wasm-pack test --headless --firefox crates/arora-web
# (or --chrome — see notes below)
```

The test loads `test-rust-wasm.wasm` through `Engine.loadModule` and
calls `ping`, asserting the round-trip works.

> **Browser pick:** `wasm-pack` downloads a pinned `chromedriver`; if it
> doesn't match the locally installed Chrome it 404s. Firefox /
> geckodriver is more forgiving; CI uses `--firefox`.
>
> **Apple Silicon:** the `geckodriver` wasm-pack auto-downloads is
> x86_64 and SIGABRTs under Rosetta with a "rosetta error: Attachment
> of code signature supplement failed" message. Install a native arm64
> driver (`brew install geckodriver`) and point at it via the
> `GECKODRIVER` env var (as shown above). Same idea for `chromedriver`
> via the `CHROMEDRIVER` env var.

## Demo pages

```bash
crates/arora-web/www/serve.sh
# open http://localhost:8080          – Engine call demo (ping, succeed, cos)
# open http://localhost:8080/demo.html – BehaviorTreeRunner tick demo
```

`serve.sh` runs `wasm-pack build`, stages guest modules under
`www/modules/<name>/`, then starts `python3 -m http.server`.

`demo.html` shows a live behavior tree: each tick increments `x` by 0.1
via `add()`, then computes `cos(x)`. Variables persist across ticks.
The SVG tree panel highlights node status; the side panel displays
variable values and a tick log.

## Why a separate crate

`arora` is dual-target now (`wasmtime-host` and `native-host` features
default-on for native; gated out on wasm32, where the browser executor
takes over). `arora-web` exists purely to attach the wasm-bindgen
surface and keep host builds free of `wasm-bindgen` deps.
