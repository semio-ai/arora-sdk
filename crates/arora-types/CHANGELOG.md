# Changelog

All notable changes to `arora-types`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [3.6.0] - 2026-10-10

### Changed

- `DataStore::write` and `Slot::set` state that observers see changes only: a
  key set to the value it already holds, or an unset of an absent key, notifies
  nothing. A behavior that writes its outputs every tick relies on it to make
  no change while a value holds. `arora-simple-data-store` (and its namespaced
  view) already behaves so; an implementation that notifies every write does
  not meet the contract.

## [3.5.0] - 2026-10-06

### Added

- `Value::conforms_to(&Type)`: whether a value is of a type — its own variant,
  exactly, with no conversion (an `F32` is not of `F64`, an `ArrayValue` of
  floats not of `ArrayF64`, an `Option` not of its content's type). A `Type`
  names the outer shape only, so what a compound value holds is not checked.
  It is the check a key's stated type (`KeyMeta::ty`) asks of a value written
  to it from outside the device.

## [3.4.0] - 2026-10-06

### Added

- `DataStore::subscribe_prefix`: a subscription to the keys under a prefix —
  the subtree `prefix_covers` says it covers, the empty prefix being the whole
  store. It opens on the current state of those keys and delivers a later
  change with what it holds of them, under their full paths, and not at all
  when it touches none of them. Provided: the default reads `subscribe`'s feed
  through `Subscription::map`, so every store keeps working; a store that keeps
  each subscriber's prefix overrides it to send a subscriber its own keys
  alone.

## [3.3.0] - 2026-10-02

### Added

- `Subscription::map`: a subscription read through a translation of its
  changes — how a view of a store, such as one namespace of it, derives its
  feed from the store's own, with no thread relaying the feed and no store
  knowing the view. The opening state is delivered whatever the translation
  leaves of it, even nothing: it is the view's own opening state. A later
  change the translation leaves empty is not delivered, as a write outside the
  view changes nothing the view holds.

## [3.2.0] - 2026-10-02

### Added

- `KeyMeta::unit`, set with `KeyMeta::in_unit`: what a numeric key's values are
  counted in — radians, metres, a fraction — named freely for whoever displays
  or converts them. A name, not an algebra: Arora relays it as written and
  computes nothing from it. On the wire it is `unit`, present only when set, so
  a reader tells "no unit" from the field's absence rather than from a null.

## [3.1.1] - 2026-10-02

### Fixed

- The walk's optional hooks no longer name ROS 2 CDR as a format with no
  optional form: arora-msgs-ros2 2.1 writes an optional as a bounded sequence
  `T[<=1]`.

## [3.1.0] - 2026-09-29

### Added

- **`KeyMeta`, and the store seams that carry it**: what a key is beyond the
  value it holds — the shape, the range it runs over, where it rests, what it is
  for, and whether a remote writer may set it. `DataStore::meta`, `all_meta`,
  `set_meta` and `set_prefix_meta` read and set it, each with a default
  implementation (no meta, and a refusal to keep any), so a store opts in when
  it has something to say and none is forced. It is the store's because it is a
  property of the key: every bridge relays the same answer instead of each
  keeping its own.
- **A key is closed to remote writers unless its meta opens it**
  (`KeyMeta::editable`, false by default): a network peer does not get to set a
  key the device never offered. A device opens a key, or a whole subtree with
  `set_prefix_meta` — the empty prefix opening everything, for a sandbox. `meta`
  resolves to the most specific statement, the key's own replacing its
  subtree's whole; `prefix_covers` is what a prefix covers, on segment
  boundaries.
- `Value::kind()`: the value's `Type` — which variant it is. What a consumer
  needs to render a value or to check that another fits the same slot, and what
  types a key off the value it holds; `type_uuid()` stays the compound record's
  own id.

## [3.0.0] - 2026-09-25

### Added

- `FrozenTy::FrozenOption` and `UnfrozenTy::UnfrozenOption`: the record
  vocabulary's optional type, an absent value or a present one of type
  `element`. Freezing, dependency collection and serde carry it.
- `FrozenTy::is_option` and `FrozenTy::as_option`.
- `module::high::TypeRef::Option`: a hand-written `module.yaml` declares an
  optional as `{ kind: option, id: … }`.
- The typed wire walk reads and writes `TypeRef::Option`: a presence flag,
  then the element when present. A bare element is written as a present
  value.

### Changed

- **Breaking:** `FrozenTy` and `UnfrozenTy` have a new variant, so an
  exhaustive `match` on either must handle it.
- **Breaking:** `ValueWriter::begin_option` and `ValueReader::enter_option`
  are required methods. A format with no optional form returns an error.

## [2.6.0] - 2026-09-22

### Added

- `module::declared`: the `AroraModule` trait (`id`, `header(executor)`,
  `record(parent)`, `exports`) and `AroraFunction` — one exported function,
  callable — what a module declared in Rust implements, the twin of
  `AroraType` for modules. The executor is the exporter's to name, so
  `header` takes it; a module linked into the host has no header and is
  described by its `record`.
- `AroraType::arora_type_version()`: the record version a frozen form pins a
  type at, `1.0.0` unless `#[arora(version = "…")]` says otherwise.
- `Value::array_of`, `Value::array_of_type` and `Value::into_elements`: pack
  elements of one type into the array form the value plane uses for it, and
  unpack any array form.
- `From<Key> for Value` / `TryFrom<Value> for Key`.

### Changed

- `#[derive(AroraType)]` (arora-types-derive 1.3) also emits `From<T> for
  Value` and `TryFrom<Value> for T`. A type that wrote those impls by hand
  must drop them (the compiler reports the conflict, E0119).

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

