# Changelog

All notable changes to `arora-engine`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [5.5.0] - 2026-10-23

### Added

- `executor::browser::BrowserComponentExecutor`, the browser's
  `wasm-component` executor, which `BrowserExecutor::component_executor()`
  gives on the same loader. It instantiates the guest core module of a
  component built for `wasm32-wasip2` and gives it, in the canonical ABI's
  form, the `arora:module/host` interface and the WASI 0.2 interfaces the
  native `ComponentExecutor` links: standard output and error go to the
  console, standard input is closed, randomness comes from
  `crypto.getRandomValues`, the clocks are `performance.now()` and
  `Date.now()`, and every pollable is ready. A component's initializer runs a
  reactor guest's `_initialize` once, as natively. A component
  importing anything else fails to load, naming the import.
- `component_layout`: the layout of a component an Arora host runs
  (`guest_module`), and the interfaces every host provides
  (`HOST_INTERFACES`, `provided`). The browser executor refuses any other
  component at compilation: one with more than one guest core module or a
  nested component, one importing anything but those interfaces, and one
  whose exports are not the world's `functions` and `dispatch` with the
  world's types, each lifting its guest module's export of that name as the
  browser calls it, or whose shim, fixups or initializer do more or other
  than `wit-component`'s; it reads no further, and does not check how the
  guest module's own imports are wired, which the browser implements by
  name.
- `SharedLoader::prepare` stages a component as it stages a core module, by
  the executable's preamble.
- On `wasm32`, `CompiledModule::new` compiles a `wasm-component` module: the
  guest core module of the component, which each engine instantiates.

### Changed

- **For guests:** the `arora:module` world is at 0.2.0 and exports
  `functions`, the ids of the functions a component serves, which the
  component executors check the functions a header declares against when
  they load it, as a core module's exports are checked. A component built
  against the world at 0.1.0 does not load. No component was published
  against 0.1.0.
- **For guests:** `ComponentExecutor` links only the interfaces
  `HOST_INTERFACES` names: a component importing `wasi:sockets` no longer
  loads (5.4 linked all of WASI 0.2). `wasi:filesystem` is linked with no
  directory preopened, as 5.4 did, and the browser executor provides the
  same.
- A component that traps runs no more calls: each later call fails, naming
  the trap, until the module is reloaded. wasmtime enters a component
  instance no more after a trap, and the browser executor holds a component
  to the same rule.
- A component that returns an error buffer, as a core module writes its
  errors, fails the call with `DispatchError::Guest`, as a core module does.
- `ComponentExecutor` refuses a call to a function its module's header does
  not declare with `DispatchError::FunctionNotFound`, as the other executors
  do.

### Fixed

- An error buffer whose message's length runs past the buffer is read up to
  the buffer's end; the `wasm` executor panicked on it.
- `BrowserExecutor` reads a core module's error buffer as its error, as the
  wasmtime executor does; it returned it as the call's result.

## [5.4.0] - 2026-10-22

### Added

- A guest that exports `_initialize` has it called once, when it loads,
  before any other export, by `WebAssemblyExecutor` and `BrowserExecutor`
  alike. `_initialize` is the entry point of a WASI reactor — a C or C++
  guest linked with `-mexec-model=reactor` — and runs the guest's static
  constructors, which nothing ran before: a C++ global with a dynamic
  initializer stayed zeroed memory. A guest that does not export it loads as
  it did.

### Security

- Wasmtime 50, from 45. Wasmtime 45 is in the affected range of:
  - [GHSA-mghf-wjfp-p5xm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm)
    (critical): on aarch64, Cranelift compiles a `br_table` of more than
    about 256k targets to a branch that can land at an address the module
    chooses — a sandbox escape under default settings.
  - [GHSA-32h6-97mm-8q3c](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-32h6-97mm-8q3c)
    (critical): the result count of an async-lifted component export's
    callback is not validated, and a guest writes past a native stack
    buffer. Reachable only with the component model's async ABI enabled,
    which `ComponentExecutor` does not enable (below).
  - [GHSA-m9f7-9w4p-gv38](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-m9f7-9w4p-gv38)
    (high): on macOS and FreeBSD, a guest with write access to a preopened
    directory escapes it by renaming a symlink. Neither wasmtime executor
    preopens a directory.

  Wasmtime is not part of this crate's interface: nothing changes for a
  caller.
- `ComponentExecutor` refuses to compile a component that uses the component
  model's concurrency ABI — async functions, streams, futures. The
  `arora:module` world declares none of them, so such a component is not an
  Arora module, and refusing it keeps that part of wasmtime out of a guest's
  reach. Wasmtime 50 enables the ABI by default; the executor turns it off,
  so it refuses the same components it refused under wasmtime 45.

## [5.3.0] - 2026-10-16

### Added

- `Engine::reload_module(&CompiledModule)`: replace the module loaded under
  the compiled module's id with a fresh instance of it. The new instance is
  created before the old one is dropped, so a version that does not load
  leaves the old one loaded and serving (`ReloadModuleError::Load`). An id
  with no module fails with `ReloadModuleError::ModuleNotFound`.
- `Engine::unload_module(&Uuid)`: remove a module from dispatch and drop its
  instance; a call to it then fails with `ModuleNotFound`.
- `Engine::contains_module(&Uuid)`: whether a module dispatches under an id.
- `Executor::can_unload`, defaulting to `true`: whether the engine may drop
  one of the executor's modules while it runs. `NativeExecutor` answers
  `false`, because a native library cannot be closed safely while the
  process runs. The engine refuses to reload or unload such a module with
  `ReloadModuleError::Unsupported` / `UnloadModuleError::Unsupported`, before
  loading anything. The new `Unsupported` variant of `UnloadModuleError` is
  the first value any engine function returns of that enum.

Neither `reload_module` nor `unload_module` is reachable from a dispatch: a
module cannot drop an instance while a call into it is on the stack.

### Changed

- `WebAssemblyExecutor::unload_module` returns `Ok(())` instead of panicking:
  the instance belongs to the engine, which drops it.

## [5.2.1] - 2026-10-16

### Fixed

- A call to a function a WebAssembly guest's header does not declare fails
  with `DispatchError::FunctionNotFound`. The wasmtime executor panicked on
  it; the browser executor failed with `DispatchError::Internal` after
  allocating the argument in guest memory, which it never freed. Both now
  refuse the call before anything reaches the guest.

## [5.2.0] - 2026-10-05

### Added

- `compiled::CompiledModule`: a guest module compiled once, to load into any
  number of engines with `Engine::load_compiled_module`. Each engine
  instantiates the compiled code — an instance of its own, with its own memory
  — instead of compiling the executable. `CompiledModule::new(header,
  executable)` compiles a `"wasm"` module, to a `wasmtime::Module` natively and
  to a `WebAssembly.Module` on `wasm32`; a module for another executor keeps
  its bytes, which that executor loads as it loads a `ModuleDefinition`.
  Cloning it shares the compiled code; natively it is `Send + Sync`.
- `Executor::load_compiled_module`, which `Engine::load_compiled_module` calls.
  Its default hands the module's bytes to `load_module`, so an executor that
  loads its executable as it is needs no change; the WebAssembly executors
  instantiate the compiled code.

### Changed

- Every `WebAssemblyExecutor` in a process compiles and instantiates on one
  wasmtime engine instead of an engine each, created by the first executor or
  `"wasm"` compilation: a `wasmtime::Module` instantiates only on the engine
  that compiled it, so one engine is what lets devices share a compiled
  module. Each instance keeps a store of its own. The engine's pooling
  allocator reserves address space for all its slots when it is created, so
  where an engine per executor bounded the number of executors a process
  could create, one engine bounds the guest instances a process runs at once:
  1000 on 64-bit targets, 100 on 32-bit. A failure to create the engine is
  kept, and every later executor and compilation fails with it.

### Fixed

- `WebAssemblyExecutor` fails to load a module that does not export what its
  header declares, or exports no `memory`, with an error naming what is
  missing, instead of panicking; so does loading before `set_engine`.

## [5.1.0] - 2026-09-28

### Added

- `HostModule::from_exports(id, exports)`: the host module serving declared
  functions under a module id of the caller's choosing, such as the
  implementation of an `arora-module` contract. `HostModule::of` is
  `from_exports` over a declared module's own id and exports.

## [5.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3 and arora-buffers 3.

## [4.2.1] - 2026-09-25

### Fixed

- `NativeExecutor` reads a result's 4-byte size prefix little-endian, as the
  buffer writer produces it and every other executor reads it. The result
  slice it returns now spans the result instead of a byte-swapped length.

## [4.2.0] - 2026-09-24

### Added

- `HostModule::of::<M: AroraModule>()`: the host module a Rust declaration
  describes — every export attached under its own id with its frozen
  signature, so calls match arguments by parameter id and method
  introspection lists the functions.

## [4.1.0] - 2026-07-29

### Added

- `ModuleBuilder::described_function` attaches a function together with its
  declared, frozen signature; `HostModule::descriptions` hands the set back
  (`FunctionDescription`). How a host module's functions become discoverable —
  the host-side equivalent of a guest header's exports.

## [3.0.0] - 2026-07-20

### Breaking

- `arora_call` takes the `Call` alone (the module comes from `Call::module_id`;
  a call naming no module is refused). Re-pinned to `arora-types` 2.

## [2.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [2.0.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

## [1.1.0] - 2026-07-10

### Breaking

- 6.0.0 — the runtime's call dispatch is part of its face
- The golden behavior edit — a Call reaches interpreter.apply through the engine (PR 5b)

## [1.0.0] - 2026-07-09

### Breaking

- 1.0.0 — the engine on crates.io predates the workspace by months
- Drop the private semio-record dependency — type records live in arora-types

### Fixed

- Nested/recursive/dynamic type codegen (ARORA-55)
- Crates.io forbids wildcard version constraints
- Package the WIT world with the crate

### Changed

- Serde_yaml 0.9 everywhere — one emitter, one parser, formats unchanged
- Group module-authoring crates under crates/arora-module-authoring/

## [0.1.0] - 2026-06-26

### Changed

- Rename the engine crate `arora` -> `arora-engine`

