# Changelog

All notable changes to `arora-module-core`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 / `arora-registry` 1 (their type records are
  part of this API).

## [0.2.0] - 2026-07-08

### Fixed

- Nested/recursive/dynamic type codegen (ARORA-55).

## [0.1.1] - 2026-07-05

### Changed

- serde_yaml 0.9 — one emitter, one parser, module formats unchanged.

## [0.1.0] - 2026-06-28

### Breaking

- Dropped the private `semio-record` dependency — type records live in
  `arora-types`.

### Changed

- Grouped under `crates/arora-module-authoring/` with the other
  module-authoring crates.
