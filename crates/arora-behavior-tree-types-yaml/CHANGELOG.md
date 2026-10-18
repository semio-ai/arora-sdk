# Changelog

All notable changes to `arora-behavior-tree-types-yaml`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-10-18

### Changed

- `Status` and `TickId` live in the `arora` scope
  (`61726f72-6100-0000-f01d-000000000000`): their paths are `arora.Status`
  and `arora.TickId`, and `records/folder/` holds the scope's folder. A
  `module.yaml` naming them by path names those.

## [0.1.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [0.1.0] - 2026-06-26

### Changed

- Bring the behavior-tree crates back into the workspace
- Extract behavior-tree into its own repo; consume it via git

