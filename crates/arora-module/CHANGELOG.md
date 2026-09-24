# Changelog

All notable changes to `arora-module`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-09-24

### Added

- The declaration macros and the traits they implement, re-exported so a
  module crate depends on this crate alone: `#[module]`, `#[export]` /
  `#[param]`, `declare_module!`, `module_from_header!`, `AroraModule` and
  `AroraFunction`.
