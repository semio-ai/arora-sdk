# Changelog

All notable changes to `arora-behavior-tree-types`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.2.0] - 2026-10-18

### Added

- `BEHAVIOR_TREE_FOLDER`, re-exported from arora-behavior: the
  `behavior_tree` folder, declared. `BEHAVIOR_TREE_FOLDER_ID` and
  `declare_behavior_tree_folder` take its id and name.

### Changed

- `TickId`'s record (`AroraType::record`, arora-types 3.8) lives in
  `BEHAVIOR_TREE_FOLDER` whatever names it. Requires arora-types 3.8 and
  arora-behavior 9.5.

## [2.1.0] - 2026-10-16

### Added

- `TickId`: the Rust type of the `behavior_tree.TickId` record, deriving
  `AroraType` under its ids.

## [2.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3 and arora-behavior 9.

## [1.1.0] - 2026-07-30

### Changed

- `Status` (the enum, its value conversions, the id constants, and
  `declare_status_enumeration`) moved to `arora-behavior` (ARORA-82) and is
  re-exported here unchanged, so existing consumers keep compiling; new code
  links `arora-behavior`. Same ids, same wire form.

## [1.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 (its type records are part of this API).

## [0.1.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [0.1.0] - 2026-06-26

### Fixed

- IndexMap-based semio-record and improved WASM building

### Changed

- Bring the behavior-tree crates back into the workspace
- Extract behavior-tree into its own repo; consume it via git

