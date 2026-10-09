# Changelog

All notable changes to `arora-hal-ros2`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [5.1.1] - 2026-10-09

### Fixed

- Requires `ros2-client-multi-rmw` 0.13.1. Before it, `Ros2Hal`'s
  `wait_for_subscription` before its first publish could miss a match under
  load and wait forever: the match event was dropped from a bounded status
  channel after the wait's one-time check (semio-ai/ros2-client#42).

## [5.1.0] - 2026-10-09

### Added

- `Ros2Hal` states the robot's model to the runtime (`Hal::assets`): the file
  at `model_glb_path`, as the `device` component's model, with its content
  hash. The device serves the file's bytes only when the config's new
  `model_glb_servable` is true; it is false by default, and an override that
  names a model file states its servability with it. Depends on arora-hal 4.1
  and, for the runner, arora 12.2.

## [5.0.0] - 2026-10-06

### Changed

- **Breaking:** depends on arora 12 (the device runner its example binary builds).

## [4.1.0] - 2026-10-04

### Changed

- The build downloads no robot models and needs no network. Studio v2 refuses
  the legacy model paths that `build.rs` read. Thus each CI run failed, and so
  did each local build, unless `models/` already held the models or the build
  set `ARORA_HAL_ROS2_SKIP_MODELS=1`. The built-in configs keep
  `models/<name>.glb` as their default model path, and the developer puts the
  robot's GLB there.
- `ARORA_HAL_ROS2_SKIP_MODELS` has no effect.
- A missing model file gives an error that names its path and
  `model_glb_path`, when the HAL builds its joint map and when it reads the
  model for `model_glb`.

### Added

- `ROS2RobotConfig::apply_overrides` applies `model_glb_path`, and applies
  `joint_ids` when the overrides set `Override` or `Extend`. An overrides file
  can now point a built-in robot at a local model:
  `arora-ros2 quori overrides.json` with `{ "model_glb_path": "/path/quori.glb" }`.

## [4.0.0] - 2026-09-29

### Changed

- **Breaking:** depends on arora 11 (the device runner its example binary builds).

## [2.0.1] - 2026-07-30

### Changed

- ros2-client re-pinned to 0.12.

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

- ROS 2 robots as Arora HALs — one binary, robots as configs (ARORA-24)

