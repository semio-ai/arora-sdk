# Changelog

All notable changes to `arora-hal-restful`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [4.4.0] - 2026-10-19

### Added

- The `studio` feature: the Semio Studio connection (`arora`'s
  `studio-bridge`). `runner` enables it.
- The `arora-restful` runner takes `--studio`, which connects to Semio Studio and
  registers a device holding no saved Studio credential (`ARORA_STUDIO=1` asks
  the same), and `--no-studio`, which serves the open local bridge even for a
  device holding one.

### Changed

- The runner connects to Semio Studio only when the device holds a saved
  Studio credential or the run passes `--studio`. Otherwise it serves the open
  local bridge and logs how to register. Started without a terminal, it used
  to connect at once, signing a device with no credential up as a new
  anonymous Studio account; on a terminal, it asked for an owner.

### Deprecated

- `runner` enabling `studio`. The next major stops: a runner that connects to
  Semio Studio is then built with `--features runner,studio`, and connects
  only with `--studio`.

## [4.3.0] - 2026-10-19

### Added

- The `arora-restful` runner carries modules as the `arora` runner does: every
  module directory under its device directory's `modules/`, then each
  `--module <DIR>` (or `--module=<DIR>`, repeatable), with the records their
  `records/` carry, so the device describes a guest module's record-typed
  exports and a behavior calls them.
  - A module directory that does not read (no header, no artifact or several,
    records that do not parse or differ between directories) fails the start,
    naming the module, before the robot is reached.
  - What the engine and the device's registry decide fails when the device is
    built, after the HAL has started: an artifact the engine rejects (a native
    module where the platform loads none, a wasm it does not compile), or a
    record that does not describe its module's header.
- `-h` and `--help` print the runner's options and exit.

### Changed

- The runner's command line is parsed with clap (`arora::RunnerArgs` for
  `--module`): an option it does not take, or a missing configuration file,
  fails the start with clap's message and usage, rather than being read as
  the configuration file, or panicking.
- The runner's front end, and the log sink it installs, start before the
  HAL: what the HAL logs while it starts is shown.
- The `runner` feature requires `arora` 13.3 and `clap` 4.

## [4.2.0] - 2026-10-16

### Added

- `RestfulHal` describes its keys (`Hal::describe_keys`, arora-hal 4.3), from
  its configuration: for each joint of a POST joint-positions endpoint,
  `<joint>/target.position`, `<joint>.target_position`,
  `<joint>/target.velocity` and `<joint>.target_velocity` as `F64` inputs
  open to remote writers, and `<joint>.position` and `<joint>.velocity` as
  `F64` outputs; positions in radians, and a velocity target described as the
  speed of the position target in the same change. The `arora-restful`
  runner opened no key before; over arora 12.7 it opens these inputs, and
  every bridge lists the keys. Where a host opened a prefix covering these
  keys, they now take the HAL's meta: the outputs are closed to remote
  writers, and a target takes only an `F64` (another value was held but
  never requested).

### Changed

- Requires `arora-hal` 4.3 and `arora-types` 3.1; the `runner` feature
  requires `arora` 13, which states the description to the device's store.

## [4.1.0] - 2026-10-12

### Changed

- `RestfulHal` takes `<joint>/target.position` and `<joint>/target.velocity`
  as it takes `<joint>.target_position` and `<joint>.target_velocity`: a
  request to the joint-position endpoint, mirrored as `<joint>.position` and
  `<joint>.velocity`. A change that sets both forms of one target requests and
  mirrors the `<joint>/target.…` value; every key stays held as written. The
  queued-write task merges its backlog with arora-hal's `fold_newest_wins`,
  so the later of two targets wins whichever form each was written in.
  Requires arora-hal 4.2.

## [4.0.1] - 2026-10-09

### Changed

- `RestfulHal` states its models through arora-hal 4.1's `HalAssets::models`:
  none, as before.

## [4.0.0] - 2026-10-06

### Changed

- **Breaking:** depends on arora 12 (the device runner its example binary builds).

## [3.0.0] - 2026-09-29

### Changed

- **Breaking:** depends on arora 11 (the device runner its example binary builds).

## [2.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 / `arora-hal` 3 (their types are part of this
  API); the optional `arora` dependency moves to 9.

## [1.1.0] - 2026-07-12

### Changed

- Re-pinned the optional `arora` dependency to 8 (the breaking `with_module`
  wave); no change to this crate's own surface.

## [1.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [1.0.0] - 2026-07-09

### Breaking

- The sensor feed as an owned stream

## [0.2.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum
- Rename Behavior trait to BehaviorInterpreter
- Golden clock keys (time/dt) in the store; drop ctx.dt

## [0.1.0] - 2026-07-06

### Added

- RESTful-API robots as Arora HALs (ARORA-24)

