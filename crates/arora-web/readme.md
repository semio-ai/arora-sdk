# arora-web

Run an Arora device inside a browser. Compiles `arora` (with
`--no-default-features`, so no wasmtime, no libloading) to
`wasm32-unknown-unknown` and exposes two things:

- **`AroraRuntime`** (Rust name: `AroraWeb`) — the wasm-bindgen device over an
  `arora::Arora`: a synchronous `step()`, a self-pacing `run()`, in-process
  `call()` dispatch, and Value↔JSON store accessors.
- **`Engine`** / **`BehaviorTreeRunner`** — a lower-level JS surface for loading
  guest modules and running behavior trees directly on the engine.

## `AroraRuntime` (the device)

```ts
class AroraRuntime {
  constructor();                          // the demo device (fake HAL, no bridge)
  step(dtMs: number): void;               // one step, e.g. from requestAnimationFrame
  run(periodMs?: number): Promise<void>; // hands the device to arora's own loop; resolves on stop(), rejects when stepping fails
  stop(): void;                           // reclaims the device at the next step boundary; step() works again
  readonly running: boolean;              // whether run() currently owns the device
  readonly behaviorError?: string;        // the behavior's standing error; undefined while healthy
  behaviorErrorChanged(): Promise<string | undefined>; // resolves on the next standing-error change
  call(callJson: string): Promise<string>; // in-process Call, applied by the next step; resolves to result JSON
  setValue(path: string, valueJson: string): void;
  writeValues(valuesJson: string): void;
  readValues(paths: string[]): Record<string, unknown>;
  snapshot(): Record<string, unknown>;
  drainChanges(): Record<string, unknown>; // first drain = the store's whole state
}

class AroraRuntimeBuilder {
  constructor();
  withModule(headerJson: string, executable: Uint8Array): void; // repeatable
  build(): AroraRuntime;
}
```

Values cross the JS boundary as JSON in the Arora `Value` vocabulary, e.g.
`{"f32": 0.75}`. `drainChanges` is the poll-based counterpart to a store
subscription (JavaScript can't await the std channel `DataStore::subscribe`
delivers on); a subscription opens on the store's whole current state, so the
first drain returns the full picture.

`run()` hands the device to `arora::Arora::run` until `stop()` reclaims it —
the loop ends at its next step boundary, the run promise resolves, and
`step()` (or another `run()`) works again; while it runs, `step()` is
unavailable. The rest of the surface keeps working while the
device runs, because none of it touches the stepping device: `setValue`/
`readValues`/`snapshot` work on a sibling handle of the store, `drainChanges`
on its subscription, and `call` goes through the device's in-process
`arora::Caller` — enqueued at once, applied at the next step's event phase
exactly like a remote's call, resolved on that step's reply. The device must
be stepping (`run()` or your own `step` calls) for a `call` to land.

A downstream device (e.g. Vizij) composes its own `arora::Arora` with
`arora::AroraBuilder` — the HAL, bridge, and store seams are trait objects that
cannot cross the JS boundary — and wraps it with `AroraWeb::from`, reusing the
same JS surface. The `store_json` module exposes the Value↔JSON accessors over
any store for wrappers that add their own methods.

## Engine / BehaviorTreeRunner JS surface (see [src/lib.rs](src/lib.rs)):

```ts
class Engine {
  constructor();
  loadModule(headerJson: string, executable: Uint8Array): string; // returns module id; sync compile (< 8 MB in Chrome)
  prepareModule(headerJson: string, executable: Uint8Array): Promise<void>; // async compile + instantiate, any size
  loadPreparedModule(headerJson: string): string;                   // completes a prepareModule load; returns module id
  call(callJson: string): string;                                   // returns result JSON
  listModules(): string;                                            // returns JSON array of loaded module headers
}

class BehaviorTreeRunner {
  constructor();
  loadModule(headerJson: string, executable: Uint8Array): string;
  prepareModule(headerJson: string, executable: Uint8Array): Promise<void>;
  loadPreparedModule(headerJson: string): string;
  listModules(): string;                                            // returns JSON array of loaded module headers
  setVariable(varId: string, valueJson: string): void;
  tick(nodesJson: string): string;  // returns {status, trace, variables}
  run(nodesJson: string): string;   // runs until not Running
}
```

`loadModule` compiles and instantiates synchronously, which Chrome rejects
above 8 MB on the main thread. The `prepareModule` + `loadPreparedModule`
pair routes through the async `WebAssembly.instantiate` and works for any
size in every browser.

`callJson` matches `arora_engine::call::Call`:
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
