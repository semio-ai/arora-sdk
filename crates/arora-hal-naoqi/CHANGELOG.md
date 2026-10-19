# Changelog

All notable changes to `arora-hal-naoqi`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-10-19

### Added

- The `arora-naoqi` runner carries modules as the `arora` runner does: every
  module directory under its device directory's `modules/`, then each
  `--module <dir>` (repeatable), with the records their `records/` carry, so
  the device describes a guest module's record-typed exports and a tree's
  leaves call them. A module directory that does not read fails the start,
  naming the module, before the robot is reached. The binary cross-compiled
  for the robot is static, so it loads WebAssembly modules; a native module
  fails the start, naming it.

### Changed

- The `runner` feature requires `arora` 13.3 (`AroraBuilder::with_module_dirs`).

## [0.1.0] - 2026-10-12

### Added

- `NaoqiHal`: NAO and Pepper robots as Arora devices over their NAOqi middleware,
  through `libqi-vibe` (the `qi` framework in pure Rust, no C++ SDK), from NAOqi 2.1 on.
  A host name (`tcp://<robot_name>.local:9559`) is tried at every address it resolves to.
- The device describes itself: its model family (`nao`, `pepper` — `Juliette` in NAOqi's
  configuration — or `romeo`), its hardware version (the body's base version, the head's
  when it differs, and the body type: `V4.0 (head VEvolution, naoH25)`) and its NAOqi
  version, from `ALRobotModel`, `ALMemory`'s `RobotConfig/*`, `ALMotion.getRobotConfig` and
  `ALSystem`.
- `standard`: the Arora robot standard the HAL maps NAOqi onto, as data
  (`standard/arora-robot.json`, Vizij's profile shape): every key with its kind, type,
  unit and range. Local to this HAL until Arora has a standard.
- Sensed keys: `<joint>.position|velocity|stiffness|temperature|current`,
  `battery.charge|current|charging`, `imu.angle.{x,y}`, `imu.gyroscope.{x,y,z}`,
  `imu.accelerometer.{x,y,z}`, `sonar.{left,right}.distance`, `touch.head.*`,
  `touch.hand.*`, `bumper.*`, `chest_button`.
- Commanded keys: `<joint>.target_position`, `<joint>.target_stiffness`,
  `led.<group>.color`, `led.<group>.intensity`, `velocity.{x,y}`, `rotation.z`.
- Autonomous Life is turned off when the HAL connects (`ALAutonomousLife.setState`), so
  that the robot's own life does not move the joints the device drives;
  `disable_autonomous_life: false` in the configuration keeps it on.
- Behavior leaves as host modules: `say` (the trees' speech contract, in the robot's
  voice through `ALTextToSpeech`; `NaoqiHal::voice` and `say::install`) and
  `gestures::hands_ready`.
- `arora-naoqi` runner binary (feature `runner`): the HAL, the leaves, and a default
  behavior tree (`trees/ready.groot.xml`: "Arora is ready" with a hand gesture), replayed
  by the terminal UI's `r` key or by writing `command.ready`, switched on by the native
  `IsStringSet` and `UnsetString` nodes; `--tree` and `--no-tree`
  choose another tree or none. Cross-compiles for the NAO (`i686-unknown-linux-musl`).
