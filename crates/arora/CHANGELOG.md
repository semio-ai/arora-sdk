# Changelog

All notable changes to `arora`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [11.8.0] - 2026-10-06

### Added

- `AroraBuilder::with_declared_module::<M>` loads a module declared in Rust
  as a guest executable with the declaration's frozen signatures, so every
  export — a `Status`-returning behavior leaf, a structure-typed parameter —
  joins the method index and is reachable from a behavior tree. `with_module`
  can index only primitive-typed exports, because a bare header carries no
  type versions; `build` keeps a declaration's signature over the primitive
  freeze it would otherwise derive from the header.
- `Arora::load_groot` installs a Groot tree into a built device: its tags
  resolve against the device's own method index and the lowered graph loads
  through the interpreter module. The `arora` binary's Groot option covers a
  device with no modules; a device that loads modules needs the index the
  builder assembled.

## [11.7.0] - 2026-10-06

### Added

- `DescribeMethods` lists a guest (wasm) module's functions that take or
  return an optional over a scalar primitive (`Option<u64>`, `Option<String>`),
  with the parameter or return typed as a `FrozenOption` over the primitive —
  the signature the module's record declares for it. `LocalCaller::invoke` and
  remote clients call them by name; an optional argument may be left out,
  which the guest reads as `None`. A function naming a record type, a map or a fixed-length array still
  dispatches without being listed: a record needs a type registry to pin its
  version.

## [11.6.1] - 2026-10-06

### Changed

- A step lends its outbound change to the HAL instead of copying it when no
  key has to be left out — none of the HAL's own readings is still a
  frame-final value, which is every frame of a HAL that reports nothing. Only
  a frame that has readings to leave out builds the HAL a change of its own.
  What the HAL receives is unchanged.

## [11.6.0] - 2026-10-05

### Added

- `LocalCaller` has every operation a remote client has over a bridge, beside
  `call`: `list_keys` and `describe_methods` (introspection), `invoke` (call a
  method by name with arguments by parameter name; a task-shaped method is
  spawned and answers with its run's handle, `Invoked::Started`; an optional
  module id chooses among modules exporting one name, which otherwise fails
  naming them), `spawn` (start a call as a concurrent task run) and `halt`.
  Each sends the `BridgeOp` a remote sends, queued before the method returns
  and applied at the next step, in order with every other inbound op; `invoke`
  reads the signature on one step and applies its call on the next.
- Re-exports `TaskHandle`, `TaskId` and `MethodSignature`, which those
  operations answer with and take.

### Changed

- Depends on arora-bridge 6.1.

## [11.5.0] - 2026-10-05

### Changed

- **A device reaches a bridge that authenticates it.** The `studio-bridge`
  feature depends on `arora-studio-bridge-client` 9.1 and connects with the
  device's Studio credentials: over a TLS endpoint (`STUDIO_BRIDGE_ENDPOINT=tls/…`)
  the client obtains from Studio a certificate whose Common Name is the
  device's principal, keeps it with its key in the device directory's `studio/`
  subdirectory, and renews it while it runs. Over `tcp/` nothing changes.

## [11.4.0] - 2026-10-03

### Changed

- A device answers `ListKeys` and `DescribeMethods` over the Studio bridge: the
  `studio-bridge` feature depends on `arora-studio-bridge-client` 9 (studio-bridge
  msgs 6, whose `AroraOp` carries both), so a Studio's `listKeys` and
  `describeMethods` reach the device's `BridgeOp::ListKeys` and
  `BridgeOp::DescribeMethods` as every other bridge's do.

## [11.3.0] - 2026-10-02

### Added

- **The device loads the modules in its directory.** Every module directory
  under `<device directory>/modules/` is loaded at start, in name order, and
  `--module <DIR>` (repeatable) adds one from elsewhere. A module directory is
  `header.json` — the module's low-level `Header`, as JSON — beside the one
  file with the extension the header's executor names: `.wasm` for `wasm`,
  the platform's dynamic library (`.so`, `.dylib`, `.dll`) for `native`. The
  artifact's name is free, so a published artifact directory (the
  `@vizij/animation-module` package's `artifact/`) is a module directory as
  is. Entries whose name starts with `.` (OS metadata such as `.DS_Store` or
  an AppleDouble `._x.wasm`) are neither module directories nor artifacts. A
  module the device cannot load — a missing or malformed header, no artifact
  or several, an executor it does not run, one module in two directories, an
  artifact the engine rejects — fails the start, naming the module. A loaded
  module's functions are reachable by any call and `DescribeMethods` lists
  their primitive-only signatures. `module_dir::read` and
  `module_dir::in_device_dir` read module directories for an embedder;
  `DeviceCli::modules` reads what the command line names, for `with_module`.
- `device_dir::from_env()`: the device directory of a run configured from the
  environment — `DEVICE_DIR`, else `<IDENTITY_FILE>_dir` while that deprecated
  variable is set (with a deprecation warning), else the per-user directory of
  the device `DEVICE_LOCAL_ID` names. `device_dir` is part of every native
  build, no longer of the `studio-bridge` feature alone.
- `standard_frontend()`: the front end `run` picks when none is injected, for
  a binary that logs before `run` — the `arora` binary installs it before it
  reads its module directories, so what the loading says is captured.

### Changed

- Two guest modules with one id fail the build, naming both; the engine would
  load the first and the method index describe the last.
- A guest module the engine cannot load fails the build naming the module (its
  header's name and id) along with the engine's reason, and each guest module
  loaded is logged with its name and id.

## [11.2.0] - 2026-10-02

### Added

- A `ListKeys` answer carries a key's unit (`KeyMeta::unit`, arora-types 3.2)
  with the rest of its meta, so every bridge relays it as it relays the range.

### Changed

- Depends on arora-types 3.2.

## [11.1.0] - 2026-10-02

### Added

- **The device directory**: what a device keeps of its own from one run to the
  next, each use in a subdirectory of its own. It is
  `<data_local_dir>/semio/arora/devices/<local id>` (`~/Library/Application
  Support` on macOS, `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows),
  the local id coming from `DEVICE_LOCAL_ID`, `default` when unset; two devices
  on one host set different ids. `DEVICE_DIR` replaces the whole path, and so
  does `studio::connect_with_device_dir(&Path)` for an embedder whose platform
  gives it a data directory (Android, a Tauri app). `device_dir::of(local_id)`
  gives a local id's per-user directory. The README shows the tree.

### Changed

- **The Studio credentials live in the device directory's `studio/`**, kept by
  arora-studio-bridge-client 8.1's `DeviceCredentials`, which alone writes
  inside it. Rebuilding, reinstalling or moving the binary no longer registers
  the device anew. Credentials from arora 11.0 and earlier (`.semio/arora`
  under the executable's directory, the home directory or the current
  directory) move into `devices/default/studio/` at the first start, so the
  device keeps its Studio identity.
- No file is written to probe a directory's writability; the directories and
  files are readable by the current user alone on Unix.
- A saved refresh token that cannot be read says why when the device signs in
  anew.
- Depends on arora-studio-bridge-client 8.1, and no longer on
  `crypto_secretbox`.

### Deprecated

- `IDENTITY_FILE`. The device directory of a run that sets it is
  `<IDENTITY_FILE>_dir`: the file is copied there with the key it decrypts
  under, and is neither updated nor removed afterwards. Each run warns of the
  deprecation and that the file can be removed; a run with the file absent
  falls back to the directory. With neither the file nor the directory
  present, the run fails.

## [11.0.1] - 2026-09-29

### Fixed

- The `studio-bridge` feature builds: it depends on arora-studio-bridge-client 8,
  which implements the arora-bridge 6 `Bridge` the rest of arora speaks.

## [11.0.0] - 2026-09-29

### Added

- `arora --open`: every key is an input, for a sandbox or a bench. Without it the
  binary's device accepts no remote writes until something opens a key.
- `serve_local_ws_bridge(Arc<AroraWSServer>)`: serve a server you built — bound,
  spawned, and cancelled when the returned bridge is dropped, exactly as
  `local_ws_bridge` does it. The caller keeps the server, so it can reach it at
  any point in the run. `local_ws_bridge_with` is one line of it.

### Changed

- **Breaking:** `BridgeOp::ListKeys` answers with each key's
  `KeyMeta` — the store's meta, with the shape of the value it holds filled in
  where the store says nothing, and every key the store describes even if nothing
  has written it yet. A bridge relays that instead of keeping its own account of
  the device's keys.
- **Breaking:** a `BridgeOp::Update` reaches only the keys the device opened
  (`KeyMeta::editable`); any other is refused, naming the key. This is every
  bridge's inbound write — ws, ROS 2, Studio — checked once on the way in, so a
  device that opens nothing accepts no remote writes. The device's own writers
  (HAL, modules, behavior) are never checked.
- **Breaking:** depends on arora-types 3.1, arora-bridge 6 and arora-bridge-ws 7,
  the last of which this crate re-exports as `arora::bridge_ws`.

## [10.3.0] - 2026-09-28

### Added

- The methods the behavior interpreter describes (arora-behavior 9.1's
  `described_methods`) join the method index under the interpreter module,
  so a remote discovers and spawns them like any task run. A direct call to
  one fails, saying to spawn it. A function id described both by a module
  and by the interpreter fails the build.

## [10.2.0] - 2026-09-28

### Added

- The re-exported `HostModule` has `from_exports`: depends on arora-engine
  5.1.

## [10.1.0] - 2026-09-28

### Added

- `local_ws_bridge_with(ServerConfig)`: the open local bridge on another port, on
  a LAN-facing address, or serving the control panel — everything else is
  `local_ws_bridge`, so an app that needs one of those no longer rebuilds the
  bind, the serving task and its cancellation.
- `arora::bridge_ws`: the open local bridge's crate re-exported, so an embedder
  names `ServerConfig` through the arora it serves.

### Changed

- Depends on arora-bridge-ws 6, where a client reaches the device's own methods
  and subscribes to the keys it wants.

## [10.0.1] - 2026-09-26

### Fixed

- The `studio-bridge` feature builds: it depends on arora-studio-bridge-client
  7, which implements the arora-bridge 5 `Bridge` trait.

## [10.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3, arora-engine 5, arora-behavior 9,
  arora-behavior-tree 8, arora-bridge 5, arora-bridge-ws 5, arora-hal 4 and
  arora-simple-data-store 3.
- The `studio-bridge` feature does not build until arora-studio-bridge-client
  is released against arora-bridge 5: the client implements
  `arora_bridge::Bridge`.

## [9.11.0] - 2026-07-30

### Added

- Loaded guest (wasm) modules become discoverable: each export whose parameters
  and return are all primitives joins the device's method index with a frozen
  signature, so `DescribeMethods` lists it — the guest counterpart of 9.10's
  host-module discoverability. A guest header carries no type versions, so an
  export with a non-primitive type dispatches but stays undiscoverable (it
  needs a registry to pin versions); it is logged at build.

## [9.10.0] - 2026-07-29

### Added

- Host modules become discoverable: the described functions of every
  `with_host_module` module join the device's method index, so introspection
  (`DescribeMethods`) lists them with their signatures — including the
  action-shaped ones the ROS 2 bridge turns into actions. Declare them with
  `ModuleBuilder::described_function`; an undescribed function still
  dispatches, it is just not listed.

### Fixed

- The method index was never populated: `DescribeMethods` answered with an
  empty list on every built device, whatever modules it loaded.

## [9.9.0] - 2026-07-29

### Changed

- Re-pinned to `arora-behavior-tree` 7 — the runner-scaffold interpreter with
  reactive tick semantics (one runtime tick is one behavior-tree tick). Task
  runs and the loaded behavior are now tree structure under a parallel runner,
  introspectable through the interpreter's graph; the interpreter is a standing
  policy (always `Running`, never dropped). The binary's Groot flow loads with
  `load_groot(xml)` — the tree binds to the device's store at its first tick.

## [9.6.0] - 2026-07-27

### Added

- `arora::local_ws_bridge()`: the open local bridge constructor, now public.
  `AroraBuilder::run` still attaches it for you when you inject no bridge; a host
  that composes its own bridge set (the open local bridge *and* a ROS 2 or Studio
  bridge) attaches it explicitly with `with_bridge(local_ws_bridge().await?)`,
  the same way `studio::connect()` is composed.

## [9.5.0] - 2026-07-27

### Added

- `AroraBuilder::with_frontend(frontend)`: inject the operator front end
  instead of the standard terminal-detection pick.
- Application commands in the terminal UI: `tui::commands_frontend(commands)`
  builds the standard front end with key-driven commands (footer label,
  optional text prompt) delivered on a channel — `tui::TuiCommand`,
  `tui::TuiCommandEvent`.

### Changed

- Stopping a run is dropping its future, now as a documented, complete
  teardown: the operator flow polls the access-request serving in its own
  scope (no detached task), so dropping `AroraBuilder::run`'s future
  synchronously releases the front end (the terminal), the device, and its
  bridges; `Arora::run` documents drop-between-steps cancellation.

## [9.4.0] - 2026-07-27

### Added

- `DeviceCli`: the standard device binary's command line as a clap helper
  (native-only); the Groot argument moves there.

### Fixed

- The library no longer reads argv: `run`/`run_with`/`run_with_hal` and the
  run funnel took the process' first argument as a Groot file, which broke
  any embedding binary with its own CLI. The `arora` binary parses
  `DeviceCli` and composes the behavior-tree load itself.

## [9.3.0] - 2026-07-24

### Changed

- Re-pinned to `arora-behavior` 7 (the `LinkSource::Select` change, ARORA-72).
  No engine behavior change.

## [9.2.0] - 2026-07-23

### Changed

- Adopt `arora-behavior` 6's "golden"→"built-in" rename (ARORA-59): the runtime
  publishes the built-in clock keys via `arora_behavior::built_in`. Public API
  and the `arora/time`/`arora/dt` wire keys are unchanged.

## [9.1.0] - 2026-07-21

### Changed

- A failing behavior no longer stops the device: the step (and so `run`)
  goes on — the HAL keeps being driven, the behavior is ticked again next
  step. The failure is logged once per distinct message and stands on a
  watch — `Arora::behavior_error` hands a receiver that outlives `run`
  owning the device, so a self-paced device's embedder still observes it;
  nothing reaches the store.
- `LocalCaller` enqueues a call before `call` returns rather than when the
  returned future is first polled, so firing and stepping the device in the
  same breath lands the call on that step — a JS Promise, in particular, is
  first polled a microtask later.

### Fixed

- `run` yields its thread even when every step overruns the period: the
  metronome's overrun arm now suspends through the timer, so a saturated
  device shares its thread — on the web, an overrunning device used to
  freeze the whole page.
- `arora-studio-bridge-client` re-pinned to 6: claims inform the embedding
  application and no longer filter commands.

## [9.0.2] - 2026-07-21

### Changed

- `arora-studio-bridge-client` re-pinned to 5: the client now watches the
  device's per-studio claim set (`DeviceClient::claims`) and refuses a
  command whose named caller has not claimed the device. Nothing changes
  in this crate's surface — claims stay outside the runtime.

## [9.0.1] - 2026-07-21

### Fixed

- The optional `studio-bridge` feature compiles again:
  `arora-studio-bridge-client` re-pinned to 4, the release built on this
  crate's own arora-bridge 4 / arora-types 2.

## [9.0.0] - 2026-07-20

### Breaking

- The step-phase functions are private: the step pipeline is internal, `step`
  and `run` are the surface.
- `StepOutcome` is gone — `step` returns `Result<(), RuntimeError>`, and a
  remote reporting the device unregistered no longer stops it: the device keeps
  serving whoever else it is attached to.
- `Telemetry`/`TelemetrySnapshot`/`Arora::telemetry` are gone — indicators are
  state. `ReadyHook` hands a store `Subscription` (opening on the whole current
  state); the TUI derives the loop rate from `arora/dt`.
- The clock (`arora/time`, `arora/dt`) travels outbound like any other state;
  a consumer that does not want it filters it on its side.
- Outbound bridge writes are gated per endpoint on `Inbound::DataRequested`:
  a change reaches only the remotes that asked. The HAL is written either way.

### Added

- `Arora::call` dispatches a `Call` in-process, synchronously, between steps —
  the same module-scoped path a bridge command takes, including the
  interpreter module's LOAD/EDIT.
- `LocalCaller` (from `Arora::caller()`), the in-process `arora_bridge::Caller`:
  dispatch `Call`s into the device while `run` owns it — enqueued immediately,
  applied at the next step's event phase like a remote's, resolved on that
  step's reply.
- `Arora::engine()` exposes the engine's `CallBridge` seam (registering
  in-process callables).

## [8.1.0] - 2026-07-15

### Added

- `studio::connect()` is exposed as an injectable Studio Bridge.

### Changed

- The `studio-bridge` integration runs on `arora-studio-bridge-client` v3.1: the
  Firebase config and bridge endpoint are baked in, and the device prompts
  interactively for the owner / device name / model family on first connect.

## [8.0.0] - 2026-07-12

### Breaking

- `with_module` loads a guest module; host functions move to `with_host_module`.

## [7.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

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

