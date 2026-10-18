# Changelog

All notable changes to `arora-module`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.2.0] - 2026-10-18

### Added

- `scope = SCOPE` in a declaration, the scope its records live in, and its
  `type_records()`, the records of the types its functions name, each in the
  scope its type states or in the module's; see arora-module-macros 3.0.0.
  Requires arora-types 3.8.

### Changed

- A `Key` parameter or return is frozen in a declaration's record as the
  string primitive (arora-module-macros 3.0.0).

## [2.1.0] - 2026-09-28

### Added

- `#[contract]`: functions several modules implement, each under its own
  module id; see arora-module-macros 2.1.0.

### Changed

- Duplicate ids and names in a declaration fail the build; see
  arora-module-macros 2.1.0.

## [2.0.0] - 2026-09-25

### Added

- Optional parameters and returns: see arora-module-macros 2.0.0.

### Changed

- **Breaking:** depends on arora-types 3, arora-buffers 3 and
  arora-module-macros 2.

## [1.0.0] - 2026-09-24

### Added

- The declaration macros and the traits they implement, re-exported so a
  module crate depends on this crate alone: `#[module]`, `#[export]` /
  `#[param]`, `declare_module!`, `module_from_header!`, `AroraModule` and
  `AroraFunction`.
