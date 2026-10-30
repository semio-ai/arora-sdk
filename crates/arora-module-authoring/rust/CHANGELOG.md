# Changelog

All notable changes to `arora-module-rust`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [5.0.0] - 2026-10-30

### Changed

- **Breaking:** depends on arora-module-core 4, whose types its API carries
  (`ModuleAsset`, `GenerationError::ModuleDeclarationError`).
- Records are written with `yaml_serde` 0.10 and names converted with
  `convert_case` 0.12; the generated sources and records are unchanged.

## [4.1.0] - 2026-10-23

### Added

- Each function's `dispatch_buffer_<id>(input)`: a call as an artifact
  receives it, the size-prefixed argument buffer in and the result buffer
  out, with the function's error as `Err`. Its `arora_function_<id>` entry
  point calls it, and so does the module's `dispatch_buffer(method, arg)`,
  which, with `function_ids()`, `arora_module::artifact!` serves a
  component's `functions` and `dispatch` from: a crate built for
  `wasm32-wasip2` with `arora_module::artifact!(arora_generated::export)` at
  its root is a component. An argument that is not an Arora Buffer
  (`arora_buffers::well_formed`, arora-buffers 3.1) is the call's error. An
  argument of another type than a parameter's still panics, which a
  component's trap makes final until it is reloaded.
- Built for `wasm32-wasip2`, the generated `arora_dispatch` and
  `arora_dispatch_indirect` call the engine through the `arora:module`
  world's `host` interface (arora-module 2.3), so the crate depends on
  arora-module there.

## [4.0.0] - 2026-10-22

### Changed

- **Breaking:** depends on arora-module-core 3, whose `ModuleAsset`s
  `generate_sources` takes.
- The generated header carries the module's version, author, license,
  description and executor bounds as its `module.yaml` declares them, and is
  written as `header.json` beside `module.yaml`.

## [3.0.0] - 2026-10-16

### Added

- `ExternTypes`: the records whose Rust type is declared elsewhere (a
  `#[derive(AroraType)]` type), each with the path generated code names it
  by. Such a record is not generated; fields, parameters and returns of it
  name the declared type. A module using such a record at another version
  than its declared type's fails to generate.

### Changed

- **Breaking:** a generated enumeration or structure is a declared type: it
  derives `AroraType` under the record's id, version, and variant or field
  ids, and its `Value` conversions are the derive's. Generated code needs
  arora-types 3.7. The type's module holds the type only: no
  `serialize_to_writer`, `deserialize_from_reader`, `Into<Box<[u8]>>`,
  `TryFrom<&[u8]>` or `*_RAW_ID` constants.
- **Breaking:** export and import shims carry a value of a record type as its
  `Value`, through `arora_buffers::froto_value`.
- **Breaking:** `generate_sources` takes `&ExternTypes`;
  `generate_enumeration_source` takes the record version and returns a
  `GenerationError`; `generate_structure_source` takes the record version and
  `&ExternTypes`.
- **Breaking:** removed `generate_into_impl`, `generate_try_from_impl`,
  `struct_field_const_id_ident`, `struct_field_ident`,
  `struct_field_intermediate_variable_ident`, `enum_variant_ident`,
  `enum_variant_const_id_ident`, `PrefixWithMod` and `CheckType`.
- A generated enumeration also derives `Copy` and `Eq`.
- A record named `String`, `Uuid`, `Value`, `Vec`, `Option`, `Box`,
  `HashMap`, `BTreeMap` or `HashSet` fails to generate: the derive would read
  a field of that type as the standard one.
- A structure field that is an optional reference to its own structure is
  boxed (`Option<Box<T>>`).
- These records cannot be declared with `#[derive(AroraType)]` and fail to
  generate: an enumeration variant carrying a value (it generated as a unit
  variant), a field holding a dynamic array value or an array of dynamic
  values, and an optional array or optional dynamic value field.

### Fixed

- An import shim whose signature names a record type compiles: it imports
  `arora_generated` when it names a generated type, and no longer emits the
  types it names as bare paths.

## [2.0.0] - 2026-09-25

### Added

- Optional types: an optional parameter, return or structure field generates
  as `Option<T>`, travels as the buffers' optional framing (byte-identical to
  `serde_uuid`'s `Value::Option`) and converts to and from `Value::Option`. An
  absent optional argument or structure field is `None`; a present one is read
  framed or as its bare element.

### Changed

- **Breaking:** depends on arora-types 3, arora-module-core 2 and
  arora-registry 2.

### Fixed

- The generated export and import shims read a size-prefixed buffer as the
  size its prefix states. They read 4 bytes past its end.

## [1.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 / `arora-registry` 1 / `arora-module-core` 1
  (their type records are part of this API).

## [0.2.1] - 2026-07-08

### Fixed

- Array-of-struct wire layout conforms to serde_uuid (ARORA-55).
- Typed Value vocabulary + array-of-enum wire conformance (ARORA-55).

## [0.2.0] - 2026-07-08

### Fixed

- Nested/recursive/dynamic type codegen (ARORA-55).

## [0.1.1] - 2026-07-05

### Changed

- serde_yaml 0.9 — one emitter, one parser, module formats unchanged.

## [0.1.0] - 2026-06-28

### Breaking

- Dropped the private `semio-record` dependency — type records live in
  `arora-types`.

### Added

- Emit `Into`/`TryFrom<Value>` for generated structs.

### Changed

- Grouped under `crates/arora-module-authoring/` with the other
  module-authoring crates.
