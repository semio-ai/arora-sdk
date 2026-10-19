# Changelog

All notable changes to `arora-animation-module`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - unreleased

### Added

- The Arora module `@arora/animation` 3.0.0 (named `animation` in the
  `arora` scope, record path `arora.animation`, id
  `76697a69-6a00-0000-0d00-000000000000`) over `arora-animation`: a
  `wasm32-wasip1` guest (`animation`) and a contract a host registers with
  a state per registration (`AnimationContract`, `exports`).
- It continues the module `@vizij/animation` 2.0.0 with the same module,
  function, parameter and record ids, and adds `load_clip`, `reload_clip`,
  `set_rate`, `step_with_velocities`, `set_length` and `set_reversed`;
  `set_loop` takes `bounce`.
- Records, in the `arora` scope: the clip format 2.0 records `Clip`,
  `ClipTrack`, `ClipKeypoint` and `ClipTransition` at 2.0.0, and
  `StepSamples` 1.0.0. `PlayerState` 1.3.0 adds `reversed` and `waiting`,
  `InstanceState` 1.2.0 adds `rate`. Every other record
  `@vizij/animation` 2.0.0 publishes in the `vizij` scope is at the next
  minor, with the same fields: `AnimationClip`, `AnimTrack` and `Keypoint`
  1.2.0; `TransitionHandle`, `TrackOutput`, `StepValues`, `OutputKeys`,
  `BakedAnimation` and `BakedTrack` 1.1.0. A registry holds one record per
  id and version, so both modules' records load into one registry.
- The `module-dir` feature: `module_dir::write`, the module directory a
  device loads the guest from (`header.json`, the guest as
  `animation.wasm`, and `records/`), and the `module_dir` example.
