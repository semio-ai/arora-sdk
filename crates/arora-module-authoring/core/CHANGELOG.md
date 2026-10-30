# Changelog

All notable changes to `arora-module-core`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [4.0.0] - 2026-10-30

### Changed

- **Breaking:** `ModuleDeclarationError::YAMLError` holds a
  `yaml_serde::Error`: module definitions are read with `yaml_serde` 0.10, the
  maintained continuation of the deprecated `serde_yaml`. The YAML read and
  written is unchanged.

## [3.0.0] - 2026-10-22

### Changed

- **Breaking:** `ModuleAsset::Module` carries a `ModuleMetadata` — the
  executor, author, license and description the definition declares — in
  place of the executor's name.
- **Breaking:** `generate_header_file` takes the module's version and its
  `ModuleMetadata`, and writes them into the header. The header it wrote had
  version 0.0.0, no author, license or description, and no executor bounds;
  a device matches a module's record by the header's version, so a module
  directory carrying such a header was not described from its record.

### Added

- `generate_header_file` writes the header as `header.json` too: the form a
  module directory carries beside its artifact.

## [2.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3 and arora-registry 2.
- A header's `TypeRef::Option`, written or resolved, maps to the record's
  optional form, and a frozen optional of a scalar is written back as
  `TypeRef::Option`. An optional of anything else has no header form and fails
  the export.

### Fixed

- An array of a primitive (`kind: array, id: f32`) resolves to that
  primitive's array kind. It resolved to the scalar.

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
