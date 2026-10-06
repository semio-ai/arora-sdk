# Changelog

All notable changes to `arora-simple-data-store`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [3.2.0] - 2026-10-06

### Added

- `SimpleDataStore` routes changes by subscriber prefix
  (`DataStore::subscribe_prefix`): it keeps each subscriber's prefix and sends
  it the keys under it alone, cloning each key once per subscriber covering it,
  and nothing when a change holds none of them. `subscribe` is the empty
  prefix. A dropped subscription is pruned at the next change to the store.
- `NamespacedStore::subscribe_prefix`: the device's keys under a prefix, the
  inner store's subscription to `<namespace>/<prefix>`.

### Changed

- `NamespacedStore` subscribes the inner store to its namespace's subtree, so
  over a `SimpleDataStore` a device's feed is sent its own keys and never a
  neighbour's: N devices each writing K keys a frame cost on the order of
  N · K key copies instead of N² · K. Over a store that keeps the trait's
  default, the view's feed is unchanged.
- Depends on arora-types 3.4 (`DataStore::subscribe_prefix`).

## [3.1.1] - 2026-10-02

### Fixed

- `NamespacedStore` is confined to its namespace in what it shows, as in what
  it writes: `snapshot` and `all_meta` list the keys under `<namespace>/` and
  no other, under their device-relative names, and `subscribe` delivers the
  changes to those keys alone, stripped likewise, the opening state included.
  Several devices over one shared store each flush and list their own keys,
  never a neighbour's.

### Changed

- Depends on arora-types 3.3 (`Subscription::map`).

## [3.1.0] - 2026-09-29

### Added

- Keeps `KeyMeta` for its keys and its subtrees (`meta`, `all_meta`,
  `set_meta`, `set_prefix_meta`), beside the cells rather than in them: a key can
  be described before anything writes it, and clearing a value says nothing
  about what the key is. `meta` answers with the most specific statement.
  `NamespacedStore` relays both through its namespace, the empty prefix meaning
  the whole device and never a neighbour.

## [3.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3.

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

