# Changelog

All notable changes to `arora-registry`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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

