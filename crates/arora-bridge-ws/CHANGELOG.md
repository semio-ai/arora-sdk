# Changelog

All notable changes to `arora-bridge-ws`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [3.1.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [3.1.0] - 2026-07-10

### Fixed

- A dead server ends the inbound stream; run_with_hal fails fast on bind

## [3.0.0] - 2026-07-09

### Breaking

- Receiver-as-stream endpoints
- Delete the io pump; step drives the sync bridge/HAL seams

## [2.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

## [1.0.0] - 2026-07-07

### Breaking

- Rename arora-websocket to arora-bridge-ws

