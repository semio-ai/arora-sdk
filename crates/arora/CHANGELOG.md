# Changelog

All notable changes to `arora`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [7.0.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

### Added

- Predetermined slots bind to the store unless linked (API-consistency PR 6)

### Fixed

- A dead server ends the inbound stream; run_with_hal fails fast on bind

## [6.0.0] - 2026-07-10

### Breaking

- 6.0.0 — the runtime's call dispatch is part of its face
- The golden behavior edit — a Call reaches interpreter.apply through the engine (PR 5b)
- Dispatch BridgeOp::Call through the engine (API-consistency PR 5a)

## [5.0.0] - 2026-07-09

### Breaking

- 5.0.0 — the graph-model interpreter is part of arora's face
- 4.0.0 — the graph-lowered interpreter is its own major
- Call_bridge, and edition that defers lowering to the tick
- Shared graph model + GraphDiff + BehaviorInterpreter::apply

## [4.1.0] - 2026-07-09

### Breaking

- 1.0.0 — the engine on crates.io predates the workspace by months
- 3.0.0 — the empty-ready interpreter is its own major
- Echo-free frame — change-only store feed, HAL-origin subtraction, non-blocking HAL sends
- Stage the Studio connection out for the release ordering

### Added

- The Studio connection returns over the published client

## [4.0.0] - 2026-07-09

### Breaking

- Design B — run drains the seams; the device owns them
- Inject the interpreter once at build; async fixed-interval run
- One Arora builder, fold Runtime, functional step
- Delete the io pump; step drives the sync bridge/HAL seams

### Fixed

- Gate anyhow macro import so wasm build is warning-clean
- SilentBridge in namespaced-store test; rustfmt

### Changed

- Build the feature against arora-bridge 2's sync Bridge

## [3.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

## [2.0.0] - 2026-07-08

### Breaking

- Rename Behavior trait to BehaviorInterpreter

## [1.0.0] - 2026-07-08

### Breaking

- Golden clock keys (time/dt) in the store; drop ctx.dt
- Rename arora-websocket to arora-bridge-ws

### Added

- A terminal operator UI — logs, indicators, and the prompt line (ARORA-51)

### Fixed

- Accept arora-websocket 1.x

## [0.2.0] - 2026-07-06

### Breaking

- One run family at the crate root — arora::run()
- Drop the private semio-record dependency — type records live in arora-types

### Added

- Headless runner accepts an injected HAL (launch_with_hal)
- The Studio connection is opt-in — new `studio-bridge` feature
- Headless registers device info from the env
- Headless device runner as arora's binary
- Bundle the Zenoh bridge deps (validated, headless bin WIP)
- Runtime over Arc<dyn DataStore> + a NamespacedStore (#108)
- Bridge introspection — ListKeys / ListMethods (ARORA-42)
- A Behavior the runtime ticks, in an arora-behavior crate (VIZ-33)
- Bind behavior-tree variables to the data store
- Launch and launch_with take an injectable data store
- Launch_with — build the bridge inside arora's runtime
- Worked example of a device-specific arora
- Boot the binary on the portable runtime
- Run the opinionated arora runtime on wasm
- Single-thread, step-dispatched runtime loop (Phase 4b)

### Fixed

- Crates.io forbids wildcard version constraints
- Clippy ptr_arg in headless app-data (&PathBuf -> &Path)
- Update the device example for the data-store launch API

### Changed

- Depend on published arora-studio-bridge-client (crates.io)
- Point studio-bridge deps at main (reqwest-0.13 migration merged)
- A behavior writes + switches a namespaced store key (ARORA-39) (#109)
- Native control nodes; free arora of the BT-nodes module
- De-flake the runtime-loop tests (sleep, not yield)
- Extract the launcher into an injectable library API
- Portable single-thread runtime (sync step + async io pump)
- Group module-authoring crates under crates/arora-module-authoring/

## [0.1.0] - 2026-06-26

### Added

- Add the opinionated `arora` runtime wrapper
- Surface guest TYPE_ERROR as DispatchError::Guest / CallError::Guest
- Tick() with arguments, return_binding, and variables

### Fixed

- Resolve hand-written lints; allow on generated subtrees
- Track getrandom 0.4 for the wasm32-unknown-unknown browser build
- Remove cdylib from arora crate-type to fix duplicate arora-types compilation
- Use normalized_value() instead of deprecated unescape_value()

### Changed

- Rename the engine crate `arora` -> `arora-engine`
- Bring arora-types into the workspace as a path crate
- Repoint behavior-tree / web / nao / polly links to their new repos
- Exercise the browser executor in CI at the engine level
- Source the CallBridge interface from arora-types
- Make arora schema re-export private to avoid type namespace confusion
- Remove cos wrapper, fix build warnings

