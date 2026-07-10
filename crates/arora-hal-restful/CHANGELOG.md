# Changelog

All notable changes to `arora-hal-restful`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [1.0.0] - 2026-07-09

### Breaking

- The sensor feed as an owned stream

## [0.2.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum
- Rename Behavior trait to BehaviorInterpreter
- Golden clock keys (time/dt) in the store; drop ctx.dt

## [0.1.0] - 2026-07-06

### Added

- RESTful-API robots as Arora HALs (ARORA-24)

