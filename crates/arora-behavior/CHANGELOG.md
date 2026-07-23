# Changelog

All notable changes to `arora-behavior`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [6.0.0] - 2026-07-23

### Breaking

- Renamed the "golden" concept to "built-in" (ARORA-59): the `golden` module is
  now `built_in`, and `golden::is_golden` is now `built_in::is_built_in`. The
  reserved keys and their wire names (`arora/time`, `arora/dt`, the `arora/`
  prefix) are unchanged — only the Rust identifiers and prose changed.

## [5.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 (its types are part of this API).

### Changed

- The golden keys (`arora/time`, `arora/dt`) travel outbound with every other
  change; a remote that does not want them filters them on its side.

## [4.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [4.0.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

## [3.1.0] - 2026-07-10

### Breaking

- The golden behavior edit — a Call reaches interpreter.apply through the engine (PR 5b)
- Call_bridge, and edition that defers lowering to the tick
- A slot's direction is which Node list holds it
- Lower Groot import onto the shared Graph; arora glue

## [3.0.0] - 2026-07-09

### Breaking

- Shared graph model + GraphDiff + BehaviorInterpreter::apply

## [2.0.0] - 2026-07-08

### Breaking

- Rename Behavior trait to BehaviorInterpreter

## [1.0.0] - 2026-07-08

### Breaking

- Golden clock keys (time/dt) in the store; drop ctx.dt

### Changed

- Link crate pages via docs.rs
- The device story — readme leads with what Arora runs
- Add description + MIT license to leaf crates for crates.io

## [0.1.0] - 2026-06-29

### Added

- A Behavior the runtime ticks, in an arora-behavior crate (VIZ-33)

