# Changelog

All notable changes to `arora-hal-naoqi`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-09-29

### Added

- `NaoqiHal`: NAO and Pepper robots as Arora devices over their NAOqi middleware,
  through the `qi` crate of `libqi-rs` (pure Rust, no C++ SDK, no cross-compilation).
- Sensed keys: `<joint_id>.position|velocity|stiffness|temperature|current`,
  `battery.charge|current|charging`, `imu.angle.{x,y}`, `imu.gyroscope.{x,y,z}`,
  `imu.accelerometer.{x,y,z}`, `sonar.{left,right}.distance`, `touch.head.*`,
  `touch.hand.*`, `bumper.*`, `chest_button`.
- Commanded keys: `<joint_id>.target_position`, `<joint_id>.target_stiffness`, `text`,
  `led.<group>.color`, `led.<group>.intensity`, `velocity.{x,y}`, `rotation.z`.
- `arora-naoqi` runner binary (feature `runner`).
