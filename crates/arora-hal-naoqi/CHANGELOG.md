# Changelog

All notable changes to `arora-hal-naoqi`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-10-23

### Changed

- `runner` no longer enables `studio`: the `arora-naoqi` runner it builds serves
  the open local bridge and takes no Studio option. A runner that connects to
  Semio Studio is built with `--features runner,studio`.
- The `arora-naoqi` runner takes `--studio` and `--no-studio` only under this
  crate's `studio` feature, whichever other crate a build enables arora's
  `studio-bridge` for: without `studio` it refuses `--studio`.
- Requires arora 16: a runner built with `studio` connects to Semio Studio only
  when the run passes `--studio` (or sets `ARORA_STUDIO=1`), a device holding a
  saved Studio credential included. A run that does not ask serves the open
  local bridge, and logs that `--studio` connects with the credential the
  device holds.

  To migrate a runner that connects with its saved credential, build it with
  `--features runner,studio` and pass `--studio` on every run that should
  connect, or set `ARORA_STUDIO=1` in its environment.
- A runner built with `studio` enrolls a device that is not enrolled with
  Semio Studio before it connects (arora 16). A person enters the code
  the run shows (in the terminal UI's prompt line, and in the log) in Studio,
  choosing the device's owner; or the run redeems the enrollment token
  `STUDIO_ENROLLMENT_TOKEN` holds. `DEVICE_OWNERS` and the owner prompt are
  gone, and a device an earlier `arora-naoqi` registered enrolls again.

## [0.2.0] - 2026-10-19

### Added

- The `studio` feature: the runner's Semio Studio connection (`arora`'s
  `studio-bridge`); it adds nothing to the library. `runner` enables it.
- The `arora-naoqi` runner takes `--studio`, which connects to Semio Studio and
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

## [0.1.1] - 2026-10-19

### Added

- The `arora-naoqi` runner carries modules as the `arora` runner does: every
  module directory under its device directory's `modules/`, then each
  `--module <DIR>` (or `--module=<DIR>`, repeatable), with the records their
  `records/` carry, so the device describes a guest module's record-typed
  exports and a tree's leaves call them.
  - A module directory that does not read (no header, no artifact or several,
    records that do not parse or differ between directories) fails the start,
    naming the module, before the robot is reached.
  - What the engine and the device's registry decide fails when the device is
    built, after the HAL has started: an artifact the engine rejects (a native
    module where the platform loads none, a wasm it does not compile), or a
    record that does not describe its module's header. The binary
    cross-compiled for the robot is static, so a native module fails there,
    naming it.

### Changed

- The runner's command line is parsed with clap (`arora::RunnerArgs` for
  `--module`): `-h` and `--help` print its options, and `--tree` and
  `--no-tree` together are refused rather than the last one winning.
- The `runner` feature requires `arora` 13.3 and `clap` 4.

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
