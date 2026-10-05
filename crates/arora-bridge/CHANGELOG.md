# Changelog

All notable changes to `arora-bridge`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [6.1.0] - 2026-10-05

### Added

- `client`: what any client of a device needs beyond single ops, whatever
  carries it to the device — `find_method` (the signature a bare method name
  designates; a name several modules export designates nothing until the module
  is named, and the error names those modules), `call_of` (bind arguments by
  parameter name), `task_shaped`, `spawn` and `halt` (start and stop a task run
  through the interpreter module), `run_of` and `Run` (the run a spawn answers
  with, as named fields with keys as paths; `Run::to_value` gives it as a
  value-plane key-value), and the shapes a client reads: `KeyInfo`, `MethodInfo`,
  `MethodParam`, `method_info`.

### Changed

- Depends on arora-behavior 9 (the interpreter module's ABI `client` speaks;
  none of its types appear in this crate's API) and arora-types 3.2.

## [6.0.0] - 2026-09-29

### Changed

- **Breaking:** `BridgeOp::ListKeys` replies with
  `Vec<(String, arora_types::data::KeyMeta)>` encoded over the value plane, not
  an array of paths: a key and what the store says it is, in one round trip, so
  a bridge relays the device's keys without keeping anything of its own.

## [5.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3.

## [4.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 (its types are part of this API).

### Added

- `Caller` + `CallFuture`: the client-side counterpart of `Bridge` — carry a
  fully-specified `Call` to a device, resolve on its reply, wherever the
  device lives.

### Changed

- `Inbound::DeviceInfo(Ok(None))` means this remote no longer knows the device
  — it says nothing about the device itself and is not a stop instruction.
- `Inbound::DataRequested` is the endpoint's aggregate: `true` while at least
  one client over it wants the device's data.

## [3.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [3.0.0] - 2026-07-09

### Breaking

- The endpoint seam — an owned inbound stream, taken once

## [2.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

### Added

- A terminal operator UI — logs, indicators, and the prompt line (ARORA-51)

### Changed

- Link crate pages via docs.rs
- The device story — readme leads with what Arora runs

## [1.0.0] - 2026-07-01

### Added

- Bridge introspection — ListKeys / ListMethods (ARORA-42)

## [0.1.0] - 2026-06-28

### Added

- Add arora-bridge — the Bridge interface + FakeBridge (Phase 3)

