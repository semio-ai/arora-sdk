# Changelog

All notable changes to `arora-module`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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
