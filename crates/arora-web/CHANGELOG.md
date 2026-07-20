# Changelog

All notable changes to `arora-web`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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

