# Changelog

All notable changes to `arora-buffers`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 (its types are part of this API).

## [1.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [1.0.0] - 2026-07-10

### Breaking

- Serde straight to the wire, and the F64/array protocol fixes it flushed out

## [0.2.0] - 2026-07-08

### Fixed

- Nested/recursive/dynamic type codegen (ARORA-55)

## [0.1.1] - 2026-07-05

### Added

- Add TYPE_ERROR discriminant and BufferWriter::add_error

### Fixed

- Decode the size header little-endian

### Changed

- Serde_yaml 0.9 everywhere — one emitter, one parser, formats unchanged
- Add description + MIT license to leaf crates for crates.io
- Bring arora-types into the workspace as a path crate

## [0.1.0] - 2022-01-22

- Release.

