# Changelog

All notable changes to `arora-module-macros`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.1.0] - 2026-09-28

### Added

- `#[contract(name = "…")]` on a trait: functions several modules implement,
  each under its own module id. The trait's methods carry `#[export]` and
  `#[param]`, take `&mut self` and have no body. Beside the trait, a module
  named after it in snake case holds `ids`, `NAME`, `descriptions()` (each
  function's name and frozen signature, by id), `record(parent)` and
  `exports(implementation)`, whose functions share the implementation. A
  contract emits no artifact entry points.
- Each export's declaration carries its name as a constant, `NAME`.

### Changed

- The build fails when two functions of one module or contract share an id or
  a name, or two parameters of one function do. Before, the later one
  silently replaced the earlier in the signature or the host module.
- A parameter name that is not a Rust identifier is reported as such, at the
  `#[param]` attribute; the macro used to panic on it.

### Fixed

- The crate docs no longer say `Option` is rejected, and describe what
  `declare_module!` and `module_from_header!` generate.

## [2.0.0] - 2026-09-25

### Added

- An `Option<T>` parameter or return, where `T` is a primitive, a type
  deriving `AroraType`, or `Value`. The header declares it as
  `TypeRef::Option` and the record as `FrozenTy::FrozenOption`. An absent
  argument, or `Value::Option(None)`, is `None`; a present one arrives as
  `Value::Option(Some(…))` or as its bare element, type-checked either way.
- `module_from_header!` reads `TypeRef::Option` as an `Option<T>`.

### Changed

- **Breaking:** generates code against arora-types 3.

## [1.0.0] - 2026-09-24

### Added

- `#[export]` / `#[param]`: one exported function — its ids, its header
  export, its frozen signature, the invocation that matches arguments by
  parameter id, the client stub, and the `#[no_mangle]` entry point an
  executor looks up in a built artifact.
- `declare_module!` and `#[module]`: the aggregate — `ids`,
  `header(executor)`, `record(parent)`, `exports()`, `client`, and a marker
  type implementing `AroraModule`.
- `module_from_header!`: ids and typed stubs from the resolved header of a
  module that is not a Rust declaration.
