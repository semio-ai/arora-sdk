# Changelog

All notable changes to `arora-engine`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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

