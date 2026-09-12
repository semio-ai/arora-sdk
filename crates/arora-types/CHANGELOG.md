# Changelog

All notable changes to `arora-types`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [3.0.0] - 2026-09-12

### Added

- `module::declared`: the `AroraModule` trait (`id`, `header(executor)`,
  `record(parent)`, `host_functions`) and `HostFunction`, what a module
  declared in Rust implements — the twin of `AroraType` for modules.
- `AroraType::arora_type_version()`: the record version a frozen form pins a
  type at, `1.0.0` unless `#[arora(version = "…")]` says otherwise.
- `Value::array_of`, `Value::array_of_type` and `Value::into_elements`: pack
  elements of one type into the array form the value plane uses for it, and
  unpack any array form.
- `arora_types::id`: the identifier spellings (`arora-id`) — hex, or thirteen
  emoji.
- `From<Key> for Value` / `TryFrom<Value> for Key`.

### Changed

- **Breaking:** `Header::executor` and `high::ModuleDefinition::executor` are
  `Option<Executor>`: the executor is named by the export that builds an
  artifact, not by a declaration, and a header without one describes a module
  linked into the host rather than loaded. Existing YAML parses unchanged
  (`#[serde(default)]`).
- `#[derive(AroraType)]` (arora-types-derive 2) also emits `From<T> for Value`
  and `TryFrom<Value> for T`.

## [2.5.1] - 2026-08-31

### Fixed

- `to_value_seeded` now gives sequences their declared array form: a field
  declared as an array of a scalar packs into the typed array
  (`Value::ArrayU8`/`…`, what the typed walk and the ROS 2 CDR codec read),
  and an array of a registered type becomes a `Value::ArrayStructure` whose
  elements carry that type's ids. Before, every sequence became a
  `Value::ArrayValue` of scalars, so a seeded message with a `uint8[]` field
  (`sensor_msgs/Image`) could not be encoded. `from_value_seeded` reads both
  forms back, including `ArrayStructure` elements.

## [2.5.0] - 2026-07-30

### Changed

- `#[derive(AroraType)]` now supports unit-variant enums (arora-types-derive
  1.2).

## [2.4.0] - 2026-07-29

### Fixed

- `value_serde`: unseeded enums now travel **name-carrying** — a unit variant
  as its bare name string, any other variant as one `KeyValue` entry keyed by
  the variant name (completing what ARORA-80 ③ did for structs). The old form
  hashed the names into an `Enumeration`, which serde's tagged-enum
  representations (e.g. `FrozenTy`'s adjacent tagging) could not decode: the
  tag routes through `deserialize_any`, where a hash is unrecognisable — so
  `DescribeMethods`' `Vec<MethodSignature>` failed to round-trip the moment a
  device had any method to describe. Values written in the old form still
  decode where the enum type is known.

## [2.1.0] - 2026-07-24

### Added

- `Key::select(&self, value: &Value)` (ARORA-72): read a key's attribute
  sub-path out of a value — a `Value::Structure` field by **id**, a
  `Value::KeyValue` field by **key**, or an array by **index**. An empty
  attribute path returns the value unchanged. Selection lives with `Key`/`Value`
  so any consumer (the shared behavior model, Vizij) can project a value by a
  key path without a registry at runtime.

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

