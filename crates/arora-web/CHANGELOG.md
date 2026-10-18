# Changelog

All notable changes to `arora-web`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [10.2.0] - 2026-10-19

### Changed

- `AroraRuntime`'s operations that change the device (`call`, `invoke`,
  `spawn`, `halt`) settle when no `run()` is ongoing, whether or not the page
  steps the device. Each one asks for a step at a zero dt in the next
  microtask, and the operations issued in one turn share that step, applied in
  issue order. That step publishes `arora/dt` as zero, ticks the behavior once
  (a node that counts ticks advances one) and writes the HAL and the bridges.
  A page that steps in the same turn applies them with its own step instead.
  On a device the page steps itself, an operation made outside the turn of a
  `step()` takes a step at a zero dt before the page's next one. Under
  `run()`, the run's next step applies them, and the cadence is the run's.
- `setValue` and `writeValues` reach the HAL and the bridges by the same steps
  (a step at a zero dt they share with the turn's operations, or the run's
  next step). A write from inside a step is carried by that step.
- `listKeys` and `describeMethods` answer at once from the device's current
  state, with no step, whether or not `run()` owns the device. They do not see
  an operation still queued. `invoke` reads its method's signature that way,
  and its call takes one step.
- A call from a callback the step itself invokes is queued like any other, and
  a step at a zero dt taken as a task after the current step applies it, or
  the run's next step does.
- A step that fails, the page's own or one at a zero dt, rejects each
  operation it left unanswered with its error. A later step still applies an
  operation the failed step did not reach.
- The `running` getter answers from inside a step, and `run()` from there
  rejects as mid-step. Both used to panic.
- Requires arora 13.2, whose `list_keys` and `Arora::describe_methods` the
  read-only operations answer from.

### Added

- `AroraWeb::complete(reply)`: settles the reply of a `LocalCaller` operation
  the way the JS surface's operations settle, for a Rust crate that wraps the
  device.

## [10.1.0] - 2026-10-18

### Added

- `AroraRuntimeBuilder.withRecords(records)`: the records guest modules are
  described by, as a module's package ships its `records/` directory — each
  record file's path under `records/` mapped to its YAML. A module whose
  record the device holds has every export described, those taking or
  returning a structure included (arora 13.1, `AroraBuilder::with_registry`).

## [10.0.0] - 2026-10-16

### Changed

- **Breaking:** depends on arora 13, whose `Arora` and `AroraBuilder` this
  crate's surface carries.

## [9.0.1] - 2026-10-16

### Fixed

- `AroraRuntime` holds the store changes a page has not drained as the latest
  value per key, folded on every step (`step()` or `run()`) and on every
  `setValue`/`writeValues`. What a page that never calls `drainChanges` holds
  is one entry per key changed since its last drain, rather than a copy of
  every change of every step. A drain
  answers as before: each changed key's latest value, `null` for a cleared
  one, the store's whole state on the first drain.

## [9.0.0] - 2026-10-06

### Changed

- **Breaking:** depends on arora 12, whose `Arora` and `AroraBuilder` this
  crate's surface carries. A device's bridges refuse a write whose value is
  not of the type its key's meta states.

## [8.2.0] - 2026-10-05

### Added

- `CompiledModule` (`new CompiledModule(headerJson, executable)`): a guest
  module compiled once, and `AroraRuntimeBuilder.withCompiledModule(module)`,
  which loads it into a device as `withModule` loads one from its bytes. Each
  device built from it instantiates the compiled `WebAssembly.Module` instead
  of compiling the bytes.

### Changed

- Depends on arora 11.8.

## [8.1.0] - 2026-10-05

### Added

- `AroraRuntime` has the client operations a remote client has over a bridge,
  beside `call`: `invoke(method, argsJson, moduleId?)` (resolves to the return
  value, or for a task-shaped method to the run's handle), `spawn(callJson)`
  (resolves to the run's handle, a plain object `{run, status, feedback,
  result, update}`: the run id and the keys' paths), `halt(runId)`, `listKeys(prefix?)` (an
  array of `{path, __meta}`) and `describeMethods(prefix?)` (each method as a
  bridge lists it, plus its `module` id).
- `AroraWeb::caller`: the device's `LocalCaller`, for a Rust crate wrapping
  the device that wants the typed operations.

### Changed

- Every client operation, `call` included, is queued on the device before the
  method returns, so a page that calls and then `step()`s in the same turn has
  it applied by that step.
- Depends on arora 11.6, arora-bridge 6.1 and arora-behavior 9.

## [8.0.0] - 2026-09-29

### Changed

- **Breaking:** depends on arora 11, whose `Arora` and `AroraBuilder` this crate's
  surface carries.

## [7.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3 and arora-engine 5.

## [6.1.0] - 2026-07-21

### Added

- `stop()`: reclaims the device from `run()` at the next step boundary — the
  run promise resolves and `step()` (or another `run()`) works again. `run`
  is no longer a one-way door.
- `running` getter: whether `run()` currently owns the device.
- `behaviorError` getter: the behavior's standing error, `undefined` while
  healthy — readable while the device runs.
- `behaviorErrorChanged()`: resolves on the next standing-error change with
  the new reading. Sequential awaits share one cursor, so a notify loop
  misses no change.

### Fixed

- A failed `run()` parks the device back in its slot instead of consuming
  it: the embedder can still `step()`, read the standing error, or `run()`
  again.

## [6.0.0] - 2026-07-20

### Breaking

- `BrowserRuntime` and `BrowserRuntimeBuilder` are gone: `arora::Arora` is the
  inner object. `AroraWeb` (JavaScript name: `AroraRuntime`) wraps it —
  constructed by its JS constructor (the demo device), by `AroraRuntimeBuilder`
  (adding guest modules), or in Rust via `From<Arora>` for composed devices.
- `AroraRuntime.start()` is `new AroraRuntime()`: it builds, it does not start.

### Added

- `run(periodMs?)` — hands the device to `Arora::run` for good (`step()` is
  unavailable from then on); the rest of the surface keeps working because it
  never touches the stepping device.
- `call(callJson)` — a promise dispatched through the device's in-process
  `Caller`, resolved after the step that applies it.
- `setValue`/`writeValues`/`readValues`/`snapshot` work on a sibling handle of
  the store (`DataStore::clone_box`), `drainChanges` on its subscription —
  usable while the device runs.
- The `store_json` module: the Value↔JSON store accessors over any
  `DataStore`/`Subscription`, for downstream wrappers.

## [5.2.2] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [5.2.1] - 2026-07-10

### Fixed

- Store accessors return plain JS objects, as documented

## [5.2.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

## [5.1.0] - 2026-07-10

### Breaking

- 6.0.0 — the runtime's call dispatch is part of its face

## [5.0.0] - 2026-07-09

### Breaking

- 5.0.0 — the graph-model interpreter is part of arora's face
- Shared graph model + GraphDiff + BehaviorInterpreter::apply
- 1.0.0 — the engine on crates.io predates the workspace by months
- Echo-free frame — change-only store feed, HAL-origin subtraction, non-blocking HAL sends

## [4.0.0] - 2026-07-09

### Breaking

- Design B — run drains the seams; the device owns them
- Inject the behavior interpreter at BrowserRuntime::start
- One Arora builder, fold Runtime, functional step
- Delete the io pump; step drives the sync bridge/HAL seams

## [3.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

## [2.0.0] - 2026-07-08

### Breaking

- Rename Behavior trait to BehaviorInterpreter

## [1.0.0] - 2026-07-08

### Breaking

- Golden clock keys (time/dt) in the store; drop ctx.dt

## [0.1.1] - 2026-07-07

### Added

- Injectable BrowserRuntime primitive + publish (ARORA-52)
- Run the opinionated arora runtime on wasm

### Fixed

- Don't claim the wasm-bindgen module start (0.1.1)

### Changed

- Native control nodes; free arora of the BT-nodes module
- Rename the engine crate `arora` -> `arora-engine`

## [0.1.0] - 2026-06-26

### Added

- Emit record files per module and expose wasm Registry
- Module-explorer demo with local file loading and introspection
- Add listModules() to Engine and BehaviorTreeRunner
- Multi-tree demo with variable table and dashed ref lines
- Add+cos behavior tree demo
- Tick() with arguments, return_binding, and variables

### Fixed

- IndexMap-based semio-record and improved WASM building
- Write back mutated params to variable store

### Changed

- Take arora-web + polly + nao modules back into the workspace
- Make the engine build and test without behavior-tree
- Describe the component executor, wasip2 target, and async browser load path
- Bindep test-rust-wasm guest instead of explicit wasm build
- Make arora schema re-export private to avoid type namespace confusion
- Replace return_binding with _ret out-parameter
- Update READMEs for return_binding, BehaviorTreeRunner, and demo
- Group wasm guest modules under www/modules/

