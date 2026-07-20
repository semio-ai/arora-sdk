# Changelog

All notable changes to `arora-hal`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [3.0.0] - 2026-07-20

### Breaking

- `FakeHal` deals only in setpoints: each `*.target_position` write is held and
  sensed back as the matching `*.position`; every other key is ignored, the way
  hardware ignores what it has no actuator for — no longer echoed back as a
  sensor reading.
- Re-pinned to `arora-types` 2.

## [2.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [2.0.0] - 2026-07-09

### Breaking

- The sensor feed as an owned stream
- Delete the io pump; step drives the sync bridge/HAL seams

### Fixed

- SilentBridge in namespaced-store test; rustfmt

## [1.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

### Changed

- Link crate pages via docs.rs
- The device story — readme leads with what Arora runs

## [0.1.0] - 2026-06-28

### Added

- Add arora-hal — the HAL trait + FakeHal (Phase 2)

