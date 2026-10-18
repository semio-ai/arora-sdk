# Changelog

All notable changes to `arora-registry`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.1.0] - 2026-10-18

### Added

- `LocalRegistry::module_at` and `LocalRegistry::type_at`: a record by its
  exact id and version, answered from memory.
- `LocalRegistry::missing_dependencies`: the records a record depends on,
  transitively, that the registry does not hold; a well-known type is never
  missing.
- `LocalRegistry::add_declared_module::<M>(version)`: a Rust-declared
  module's record, in the scope its declaration states, and the records of
  the types its functions name, each in the scope its type states or in the
  module's; each scope's folder is added with them, and a module declared
  without a scope lives at the root. Fails, naming them, when the module
  names a type without a record, when a type record differs from the one
  held at its id and version, and when another record holds a type's name in
  its scope.
- `local_yaml::records_as_yaml` and `local_yaml::write_records_to_yaml_dir`:
  the records a registry holds, in the form `load_records` and
  `load_records_from_yaml_dir` read. The same records write the same YAML.
- `RecordKind::directory` and `RecordKind::from_directory`.

### Changed

- A `LocalRegistry` keeps a record it already holds identically under its
  id and version (a structure's fields and an enumeration's variants in the
  same order) instead of failing; a different record there still fails.
  Several versions of one record are held side by side.
- `load_records` and `load_records_from_yaml_dir` add each folder after its
  parent, whatever order the folders come in.
- Another record under a path a record holds fails as `DuplicateSelector`
  whatever its version: a name is unique within its folder, and so within
  its scope.
- `TypeDefinitionFrozen` is arora-types' (3.8), re-exported here unchanged,
  and so is `local::ROOT_ID`.
- `tokio` is a development dependency only; `anyhow` and `url`, unused, are
  gone.

## [2.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3. A frozen type's dependencies are
  collected through `FrozenTy::dependencies`, so an optional's element counts.

## [1.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 (its type records are part of this API).

## [0.1.2] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [0.1.1] - 2026-07-05

### Breaking

- Drop the private semio-record dependency — type records live in arora-types

### Added

- Run the opinionated arora runtime on wasm
- Gate the remote (Semio store) registry behind a `remote` feature
- Source Selector/RecordType from arora-types
- Emit record files per module and expose wasm Registry

### Fixed

- Drop the unsafe Send assertions
- Crates.io forbids wildcard version constraints
- Clippy + readme fallout of the semio-record removal
- Resolve hand-written lints; allow on generated subtrees
- IndexMap-based semio-record and improved WASM building

### Changed

- Serde_yaml 0.9 everywhere — one emitter, one parser, formats unchanged
- Group module-authoring crates under crates/arora-module-authoring/
- Bring arora-types into the workspace as a path crate
- Use published arora-types 1.3.0 (drop the git-pin)
- Make the engine build and test without behavior-tree
- Take &str in ReadableRegistry::resolve_path

## [0.1.0] - 2022-01-22

- Release.

