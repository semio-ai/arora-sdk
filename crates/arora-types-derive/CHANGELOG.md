# Changelog

All notable changes.

## [1.2.0] - 2026-07-30

### Added

- `#[derive(AroraType)]` on enums with unit variants: emits a `ty::low`
  enumeration, pinning the type id and each variant id with
  `#[arora(id = "…")]` (payload-carrying variants and maps still rejected).
  The value-plane enums Arora exchanges (e.g. the behavior `Status`) can now
  define their schema in Rust instead of a hand-authored record.
