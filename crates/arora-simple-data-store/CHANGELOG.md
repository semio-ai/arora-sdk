# Changelog

All notable changes to `arora-simple-data-store`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [2.0.0] - 2026-07-20

### Breaking

- `subscribe` opens on the store's whole current state, delivered under the
  subscriber lock so no concurrent write can slip between snapshot and feed.

### Added

- `clone_box` on both stores (`SimpleDataStore`, `NamespacedStore`): a sibling
  handle onto the same storage.

## [1.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [1.0.0] - 2026-07-09

### Breaking

- Echo-free frame — change-only store feed, HAL-origin subtraction, non-blocking HAL sends
- One Arora builder, fold Runtime, functional step

### Added

- Runtime over Arc<dyn DataStore> + a NamespacedStore (#108)

## [0.1.0] - 2026-06-28

### Added

- DataStore trait + Slot + Subscription + arora-simple-data-store

