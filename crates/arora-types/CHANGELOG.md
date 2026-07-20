# Changelog

All notable changes to `arora-types`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.0.0] - 2026-07-20

### Breaking

- `DataStore::subscribe`'s contract: a subscription's first change is the
  store's whole current state — a subscriber starts from the full picture
  without a separate snapshot read that could race the feed.
- `DataStore::clone_box` (new required method): a sibling handle onto the same
  storage, putting the stores-share-storage-across-clones fact on the trait.
- `CallBridge::arora_call` takes the `Call` alone: the call is the full
  description of the invocation — the engine routes by `Call::module_id` and
  refuses a call naming no module.

## [1.9.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [1.9.0] - 2026-07-10

### Added

- Serde over Value — any Serialize type converts to and from arora Value

## [1.8.0] - 2026-07-09

### Breaking

- Shared graph model + GraphDiff + BehaviorInterpreter::apply

### Changed

- 1.8.0 — TypeRef equality shipped without a version move

## [1.7.0] - 2026-07-08

### Added

- Schema-aware default value + validation for ty::low

### Changed

- Serde_yaml 0.9 everywhere — one emitter, one parser, formats unchanged

## [1.6.1] - 2026-07-05

### Fixed

- One wire name for a function's return type — returnType

## [1.6.0] - 2026-07-04

### Breaking

- Drop the private semio-record dependency — type records live in arora-types

## [1.5.1] - 2026-07-01

### Fixed

- Avoid clippy unnecessary_to_owned in a state test

### Changed

- Drop stray per-member Cargo.lock files; gitignore .claude/
- Group module-authoring crates under crates/arora-module-authoring/

## [1.5.0] - 2026-06-28

### Added

- DataStore trait + Slot + Subscription + arora-simple-data-store
- Add arora-types::data vocabulary (Key/State/StateChange)

### Fixed

- Make arora-types rlib-only so `cargo test --release` stops colliding

