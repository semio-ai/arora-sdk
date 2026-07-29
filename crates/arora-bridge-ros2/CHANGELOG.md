# Changelog

All notable changes to `arora-bridge-ros2`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [3.2.0] - 2026-07-29

### Added

- **The action plane** (`/{namespace}/actions/{name}`): every *task-run* method
  — one returning the behavior-tree `Status` enumeration, the signature of a
  spawnable behavior — is exposed as a full ROS 2 action, discovered like the
  service plane (nothing declared). SendGoal spawns the run through the
  engine's interpreter-module SPAWN ABI (spoken over the value plane, pinned
  against `arora-behavior` by a dev-dependency test); the goal advances by
  watching the run's per-goal status key on the outbound state stream;
  CancelGoal issues the handle's stop call (the interpreter's halt) and the
  halt-ended run reports `CANCELED`; GetResult resolves at terminal with the
  result value the run wrote, lazily typed — as is feedback. Message types are
  synthesised at runtime and served over ros2-client's raw action server
  (`ros2-client-multi-rmw` 0.11.0), on both backends; the status topic is
  transient-local with history 1, per the ROS actions design. Actions claim
  exactly the methods the service plane skips (enum returns), so the planes
  never overlap. Covered by synthesis/lifecycle unit tests and a live-DDS
  LookAt lifecycle test (goal → feedback → cancel → errno result) against a
  typed ROS 2 action client.

### Changed

- `ros2-client-multi-rmw` 0.10.2 → 0.11.0 (the raw action server / raw
  publisher).

## [3.1.0] - 2026-07-22

### Added

- Dual middleware backend. A `zenoh` feature selects an rmw_zenoh-compatible
  ROS 2-over-Zenoh backend alongside the default `dds` backend; exactly one is
  active. Consumers typically expose these as `ros2-dds` / `ros2-zenoh`.

### Changed

- `ros2-client` now comes from the `ros2-client-multi-rmw` fork (still imported
  as `ros2_client`), which carries both backends. Native `std_msgs` scalar
  topics interoperate with C++ `rmw_zenoh` peers on the send direction. The
  default (`dds`) build is unchanged for existing consumers.

## [3.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 / `arora-bridge` 4 (their types are part of this
  API).

## [2.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [2.0.0] - 2026-07-09

### Breaking

- Receiver-as-stream endpoints
- Delete the io pump; step drives the sync bridge/HAL seams

## [1.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

## [0.1.0] - 2026-07-07

### Added

- ROS 2 as an Arora Bridge (the ros2 bridge seam)

