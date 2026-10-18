# Changelog

All notable changes.

## [2.0.0] - 2026-10-18

### Changed

- **Breaking:** the derived impl implements `AroraType::record`,
  `AroraType::register_records` and `AroraType::scope`, which arora-types
  declares from 3.8: the derive builds only against arora-types 3.8 or later,
  which re-exports it.

### Added

- The derived type's record: `record(default_scope)` is a structure (each
  field typed as a record names it: a primitive by kind — a `Key` as a string
  — a derived type by a reference at that type's `arora_type_version`, a uuid
  or a `keyvalue` field by a reference at `1.0.0`) or an enumeration of unit
  variants; `register_records` adds the records of every type it names. A
  struct with a fixed-length array field has no record.
- `#[arora(scope = SCOPE)]` on a type: the `Scope` its record lives in
  whatever names it. A type without it lives in the scope of what names it
  (at the root when that has none), and the types it names that state no
  scope live where it does. `register_records` collects the scopes with the
  records. On a field or a variant, `scope` is an error.

## [1.4.0] - 2026-10-16

### Added

- A `Box<T>` field (or `Option<Box<T>>`, `Vec<Box<T>>` element) is referenced
  and converted as `T`: a type can contain itself.
- A `()` field: the unit primitive, `Value::Unit`.
- `#[arora(name = "…")]` on a field or a variant names it in arora when that
  name is not its Rust identifier.

### Changed

- An optional field also reads its bare element as a present value, and an
  absent optional field reads as `None` instead of failing.

### Fixed

- An empty `Vec` of an enumeration converts to `Value::ArrayEnumeration`. It
  converted to `Value::ArrayStructure`.

## [1.3.0] - 2026-09-22

### Added

- `#[derive(AroraType)]` also emits the value-plane conversions `From<T> for
  Value` and `TryFrom<Value> for T` — a structure under the type's id with
  each field under its id, in declared order; a unit-variant enum as an
  enumeration under its variant's id — over every field kind it accepts
  (primitives, `Uuid`, `Option<T>`, `Vec<T>`, `[T; N]`, nested types,
  `keyvalue`, name-hash mode). A type that also wrote those impls by hand
  must drop them (E0119). The generated code calls `Value::array_of_type` and
  `Value::into_elements`, so it needs arora-types 2.6.
- `#[arora(version = "major.minor.patch")]`: the record version the type is
  pinned at (`arora_type_version`).

## [1.2.0] - 2026-07-30

### Added

- `#[derive(AroraType)]` on enums with unit variants: emits a `ty::low`
  enumeration, pinning the type id and each variant id with
  `#[arora(id = "…")]` (payload-carrying variants and maps still rejected).
  The value-plane enums Arora exchanges (e.g. the behavior `Status`) can now
  define their schema in Rust instead of a hand-authored record.
