# Changelog

All notable changes to `arora-module-rust`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.0.0] - 2026-09-25

### Added

- Optional types: an optional parameter, return or structure field generates
  as `Option<T>`, travels as the buffers' optional framing (byte-identical to
  `serde_uuid`'s `Value::Option`) and converts to and from `Value::Option`. An
  absent optional argument or structure field is `None`; a present one is read
  framed or as its bare element.

### Changed

- **Breaking:** depends on arora-types 3, arora-module-core 2 and
  arora-registry 2.

### Fixed

- The generated export and import shims read a size-prefixed buffer as the
  size its prefix states. They read 4 bytes past its end.

## [1.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 / `arora-registry` 1 / `arora-module-core` 1
  (their type records are part of this API).

## [0.2.1] - 2026-07-08

### Fixed

- Array-of-struct wire layout conforms to serde_uuid (ARORA-55).
- Typed Value vocabulary + array-of-enum wire conformance (ARORA-55).

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

### Added

- Emit `Into`/`TryFrom<Value>` for generated structs.

### Changed

- Grouped under `crates/arora-module-authoring/` with the other
  module-authoring crates.
