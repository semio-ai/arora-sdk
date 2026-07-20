# Changelog

All notable changes to `arora-bridge`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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

