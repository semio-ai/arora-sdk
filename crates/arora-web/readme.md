# arora-web

Run an Arora device in a browser page. The crate compiles the
[`arora`](../arora/readme.md) runtime to `wasm32-unknown-unknown` and exports
it to JavaScript as `AroraRuntime`; its modules run on the browser's own
`WebAssembly`, so a page hosts the same device, modules and behavior a robot
runs.

## The device in the page

An `AroraRuntime` is one device: a store of keys and values, the modules it
loaded, and its behavior. The page drives it and talks to it two ways:

- **as its owner**: it steps the device — `step(dt_ms)` from its own clock, or
  `run()` to let the device pace itself — and reads and writes the store
  directly (`setValue`, `readValues`, `snapshot`, `drainChanges`);
- **as a client**: it calls the device's functions (`call`, `invoke`), starts
  and halts task runs (`spawn`, `halt`), and lists what the device has
  (`listKeys`, `describeMethods`) — the operations a remote has over a bridge.

Values cross the boundary as JSON in the Arora `Value` form: `{"f64": 0.5}`,
`{"str": "hi"}`, `{"f32s": [0, 1, 0]}`.

## Smallest use

```sh
wasm-pack build crates/arora-web --target web --dev   # writes crates/arora-web/pkg/
```

```js
import init, { AroraRuntime } from "./pkg/arora_web.js";

await init();
const device = new AroraRuntime();            // fake hardware, no bridge, no behavior
device.setValue("face/mouth", JSON.stringify({ f64: 0.5 }));
device.step(16);                              // one step, 16 ms after the last
console.log(device.readValues(["face/mouth"]));
```

For a bundler, build with `--target bundler`.

## Loading modules

`AroraRuntimeBuilder` builds a device with modules:

```js
const builder = new AroraRuntimeBuilder();
builder.withModule(headerJson, wasmBytes);   // compiled for this device alone
builder.withRecords(records);                // the module package's records/
const device = builder.build();
```

- `withModule(headerJson, executable)` compiles the module when the device is
  built. To run one module on several devices, compile it once as a
  `CompiledModule(headerJson, executable)` and give it to each with
  `withCompiledModule`: each device still gets its own instance and memory.
  Compiling is synchronous, which Chrome refuses above 8 MB on the main thread.
- `withRecords(records)` takes the records a module package ships in its
  `records/` directory, as an object mapping each file's path under `records/`
  (`module/<id>@<version>.yaml`, `structure/…`, `enumeration/…`,
  `folder/<id>.yaml`) to its YAML. With its module's record, every export is
  described and callable, those taking or returning a structure included, and
  the build fails when the record does not match the module's header. Without
  it, only the exports whose types are primitives are described.

## Stepping

`step(dt_ms)` takes one step, `dt_ms` milliseconds after the previous one — what a
`requestAnimationFrame` delta gives. `run(period_ms?)` instead hands the device
to the runtime's own loop (about 100 Hz by default) until `stop()`; its promise
resolves on `stop()` and rejects when a step fails. While `run()` owns the
device, `step()` is refused and the rest of the surface keeps working;
`running` says which mode the device is in.

`behaviorError` is the behavior's standing error (undefined while healthy),
and `behaviorErrorChanged()` resolves on its next change. A failing tick does
not stop the device.

`drainChanges()` returns each key that changed since the previous drain, with
its latest value (`null` for a cleared key); the first drain returns the
store's whole state. What a page has not drained stays one entry per key,
however many steps pass.

## Talking to the device

```ts
call(callJson: string): Promise<string>;                 // an arora_types::call::Call as JSON
invoke(method: string, argsJson: string, moduleId?: string): Promise<unknown | Run>;
spawn(callJson: string): Promise<Run>;
halt(runId: string): Promise<void>;
listKeys(prefix?: string): Promise<{ path: string; __meta: unknown }[]>;
describeMethods(prefix?: string): Promise<unknown[]>;

type Run = { run: string; status: string; feedback: string[]; result: string[]; update: string[] };
```

`invoke` calls a method by the name its module exports, with arguments by
parameter name (`{"x": {"f64": 2.5}}`); when two modules export the name, pass
the module's id (each method `describeMethods` lists carries its `module`).
A method returning the behavior `Status` starts a task run instead and
resolves to its `Run`: `run` is the id `halt` takes, `status` the key that
reports how the run goes and ends, and the rest the keys carrying its
progress, its result and what steers it. `listKeys` gives each key with its
`KeyMeta` under `__meta`, as every bridge lists keys.

**When an operation settles.** `call`, `invoke`, `spawn` and `halt` change the
device, so they settle on the step that applies them:

- with no `run()` ongoing, the operations of one turn share one step at a zero
  dt, taken in the next microtask — unless the page steps in that turn, and its
  own step applies them. A device nobody steps still settles every operation;
- with `run()` ongoing, the run's next step applies them, and the page adds no
  step.

A step at a zero dt is an ordinary step with no time elapsed: it publishes
`arora/dt` as zero and ticks the behavior once, so what runs on time holds still
while a node that counts ticks advances. A step that fails rejects the
operations it left unanswered; an operation it had not applied yet is applied
by a later step.

`setValue` and `writeValues` hold in the store when they return, and the same
steps carry them to the HAL and the bridges. `listKeys` and `describeMethods`
never step: they answer at once from the store and from the method index read
when the device was built, so await an operation before reading what it did.

A call into this surface from inside a step (a module calling a page function
during a tick) is queued for a step at a zero dt after the current one, or the
run's next step; `step()` from there is refused. What a step needs done
belongs in a behavior.

## A device of your own

The JS constructor builds a demo device: fake hardware, no bridge, a private
store, no behavior. A downstream crate composes its own device in Rust with
`arora::AroraBuilder` — the HAL, bridge and store are trait objects, which
cannot cross the JS boundary — and wraps it with `AroraWeb::from` (`AroraWeb`
is `AroraRuntime`'s Rust name) to reuse this whole surface. For a wrapper
adding methods of its own, `store_json` holds the `Value`-as-JSON store
accessors, `AroraWeb::caller` the typed client operations (an operation through
it is a remote's, applied at the next step), `AroraWeb::complete` settles one
as this surface's operations settle, and `AroraWeb::request_step` asks for the
step at a zero dt for a change the wrapper makes another way.

## The engine alone

Below the device, `Engine` loads modules and calls their functions with no
store, behavior or step:

```ts
class Engine {
  loadModule(headerJson: string, executable: Uint8Array): string;     // the module's id; compiles synchronously
  prepareModule(headerJson: string, executable: Uint8Array): Promise<void>; // compiles asynchronously, any size
  loadPreparedModule(headerJson: string): string;                     // completes a prepareModule
  call(callJson: string): string;                                     // the CallResult as JSON
  listModules(): string;                                              // the loaded headers, as a JSON array
}
```

A `Call` without a `module_id` goes to the module the function was loaded
with. `prepareModule` + `loadPreparedModule` compile through the browser's
asynchronous `WebAssembly.instantiate`, with no size limit.

`BehaviorTreeRunner` has the same loading methods and ticks a behavior tree
given as JSON nodes over them: `setVariable(varId, valueJson)`, `tick(nodesJson)`
(one tick; returns `{status, trace, variables}`) and `run(nodesJson)` (ticks
until the tree stops running; returns `{status, trace}`). Each node is:

```json
{
  "id": "<uuid>",
  "function": "<function uuid>",
  "children": ["<child uuid>"],
  "arguments": { "<parameter uuid>": {"value": {"f64": 1.0}} },
  "return_binding": "<variable uuid>"
}
```

An argument is a `value` or a `variable_id`. With `return_binding`, the
function's return value is stored in that variable and the node reports
success. `Registry` resolves type ids to dotted names from a module's
`records.json` (`loadRecordsJson`, `resolveId`).

## Build and test

The crate builds only for `wasm32`; on the host it is empty, so it still takes
part in a native workspace build.

```sh
cargo build -p arora-web --target wasm32-unknown-unknown
GECKODRIVER=$(which geckodriver) wasm-pack test --headless --firefox crates/arora-web --lib --test browser
```

The browser test loads the `test-rust-wasm` guest, which its build compiles
for `wasm32-wasip2` through an artifact dependency, and drives `AroraRuntime`
on a device nobody steps, one the page steps, and one `run()` owns. CI runs it
on Firefox. `wasm-pack` downloads a `chromedriver` pinned to one Chrome
version, and on Apple Silicon a `geckodriver` built for x86_64: install a
native driver (`brew install geckodriver`) and name it with `GECKODRIVER` (or
`CHROMEDRIVER`).

`www/` holds two demo pages over `Engine` and `BehaviorTreeRunner`, staged and
served by `www/serve.sh` (wasm-pack and Python 3). The script copies a
generated `module.yaml` for `test-rust-wasm` that the module's build does not
write, so it stops at that step.
