# Changelog

All notable changes to `arora-hal`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [4.2.0] - 2026-10-12

### Added

- `set_targets` reads the targets a `StateChange` sets, in either of the two
  forms a target key takes for the component `c` of the entity at path `k`:
  `k/target.c` (`hip_yaw/target.position`) and `k.target_c`
  (`hip_yaw.target_position`). Each `SetTarget` carries the key as written,
  the measured key `k.c` it targets (`hip_yaw.position`), and the value; its
  `component` and `entity` name the targeted component and entity. A change
  that sets both forms of one target yields the `k/target.c` key and value
  only: the keys of a change carry no order, so the form decides. A
  namespaced last segment `target.c` is always the first form, so
  `x/target.target_position` targets `x.target_position`.
- `fold_newest_wins` merges a later change into an earlier one, newest wins
  per key and per target: a target the later change sets or unsets, in either
  form, drops the earlier change's set of it in either form. For a HAL that
  coalesces a backlog of writes.

### Changed

- `FakeHal` takes `<joint>/target.position` as a setpoint, as it takes
  `<joint>.target_position`, and senses both back as `<joint>.position`. A
  change that sets both for one joint applies `<joint>/target.position`; an
  unset of one form beside a set of the other keeps the sensed position the
  set writes.
- `FakeHal` no longer takes `<ns>/target.target_position` as a setpoint for a
  joint named `target`: that key is now the target of `<ns>.target_position`.
  `target.target_position` at the root is still a setpoint.

## [4.1.0] - 2026-10-09

### Added

- A HAL's components' models: `HalAssets::models` states each component's
  model as a `ComponentModel` (its description, published `reference`,
  `content_hash`, whether it is `servable`, and its `mount` on another
  component's model), and `HalAssets::servable_glb` returns a servable model's
  bytes. A HAL that is not composed is the one component `DEVICE`. Both
  default to none. `content_hash` hashes a GLB as Semio Studio does.
- `Hal::assets`, defaulting to `None`: a HAL that implements `HalAssets`
  returns `Some(self)`, which is how the runtime reaches its models. `FakeHal`
  does; `set_model_glb` gives it a servable `device` model.
- `hal_module`: the ids under which the runtime exposes the HAL as a module —
  its `ID`, and the functions `MODELS`, `MODEL_GLB` (with its parameter
  `MODEL_GLB_COMPONENT`) and `MODEL_GLBS`.
- `HalDescription` and the model types derive `AroraType`, so they cross the
  value plane, and serde's `Serialize` and `Deserialize`.

### Deprecated

- `HalAssets::model_glb`, now provided (returning `None`): the runtime serves
  `models` and `servable_glb`.

## [4.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3.

## [3.0.0] - 2026-07-20

### Breaking

- `FakeHal` deals only in setpoints: each `*.target_position` write is held and
  sensed back as the matching `*.position`; every other key is ignored, the way
  hardware ignores what it has no actuator for — no longer echoed back as a
  sensor reading.
- Re-pinned to `arora-types` 2.

## [2.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [2.0.0] - 2026-07-09

### Breaking

- The sensor feed as an owned stream
- Delete the io pump; step drives the sync bridge/HAL seams

### Fixed

- SilentBridge in namespaced-store test; rustfmt

## [1.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

### Changed

- Link crate pages via docs.rs
- The device story — readme leads with what Arora runs

## [0.1.0] - 2026-06-28

### Added

- Add arora-hal — the HAL trait + FakeHal (Phase 2)

