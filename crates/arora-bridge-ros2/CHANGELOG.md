# Changelog

All notable changes to `arora-bridge-ros2`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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

