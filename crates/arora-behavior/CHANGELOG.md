# Changelog

All notable changes to `arora-behavior`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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

