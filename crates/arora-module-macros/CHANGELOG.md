# Changelog

All notable changes to `arora-module-macros`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.0.0] - 2026-09-25

### Added

- An `Option<T>` parameter or return, where `T` is a primitive, a type
  deriving `AroraType`, or `Value`. The header declares it as
  `TypeRef::Option` and the record as `FrozenTy::FrozenOption`. An optional
  travels as `Value::Option`: an absent argument, or `Value::Option(None)`, is
  `None`, and a bare element is a type mismatch.
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
