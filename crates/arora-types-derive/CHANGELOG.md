# Changelog

All notable changes.

## [2.0.0] - 2026-09-12

### Added

- The derive emits the value-plane conversions `From<T> for Value` and
  `TryFrom<Value> for T` — a structure under the type's id with each field
  under its id, in declared order; a unit-variant enum as an enumeration
  under its variant's id — over every field kind it accepts (primitives,
  `Uuid`, `Option<T>`, `Vec<T>`, `[T; N]`, nested types, `keyvalue`, name-hash
  mode). **Breaking** for a type that also wrote those impls by hand.
- `#[arora(version = "major.minor.patch")]`: the record version the type is
  pinned at (`arora_type_version`).
- `#[arora(id = "…")]` accepts the thirteen-emoji spelling as well as hex.

## [1.2.0] - 2026-07-30

### Added

- `#[derive(AroraType)]` on enums with unit variants: emits a `ty::low`
  enumeration, pinning the type id and each variant id with
  `#[arora(id = "…")]` (payload-carrying variants and maps still rejected).
  The value-plane enums Arora exchanges (e.g. the behavior `Status`) can now
  define their schema in Rust instead of a hand-authored record.
