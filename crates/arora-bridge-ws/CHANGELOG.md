# Changelog

All notable changes to `arora-bridge-ws`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [6.0.0] - 2026-09-28

### Added

- A client calls **the device's methods** by name: `list_methods` answers with
  the device's module functions beside the server's registered ones, each with
  the parameters of its described signature, and `invoke` binds arguments to them
  by name. Nothing has to mirror a module's ids into the registry, which keeps
  the methods the server itself owns.
- A method that reports a behavior status is a **run**: invoking it spawns it
  through the interpreter module and answers with the run named — its id, the key
  that says how it ends, and the keys carrying feedback, result and live updates
  (`MethodInfo::task` marks it). The new `halt` message stops it by id.
- `subscribe { keys }` chooses which keys a connection is pushed; absent `keys`
  means all of them, as before. A client receives what it asked for, so no key
  needs to be filtered by the device or the app around it.
- The control panel lists the methods, calls them with arguments, stops a run it
  started, and subscribes to the keys it displays.

### Changed

- **Breaking:** `MethodInfo` has a `task` field. It derives `Default`, so
  `MethodInfo { path, ..Default::default() }` builds one.
- **Breaking:** `process_message` is no longer public: it took the server's
  handlers as loose arguments, and the device seam made that signature the
  server's own business.

## [5.0.0] - 2026-09-25

### Changed

- **Breaking:** depends on arora-types 3 and arora-bridge 5.

## [4.0.0] - 2026-07-20

### Breaking

- Re-pinned to `arora-types` 2 / `arora-bridge` 4 (their types are part of this
  API).

## [3.1.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [3.1.0] - 2026-07-10

### Fixed

- A dead server ends the inbound stream; run_with_hal fails fast on bind

## [3.0.0] - 2026-07-09

### Breaking

- Receiver-as-stream endpoints
- Delete the io pump; step drives the sync bridge/HAL seams

## [2.0.0] - 2026-07-09

### Breaking

- Synchronous try_recv/try_send seam; Inbound enum

## [1.0.0] - 2026-07-07

### Breaking

- Rename arora-websocket to arora-bridge-ws

