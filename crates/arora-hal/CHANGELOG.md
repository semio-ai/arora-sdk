# Changelog

All notable changes to `arora-hal`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

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

