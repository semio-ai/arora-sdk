# Changelog

All notable changes to `arora-bridge-ws`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [7.1.0] - 2026-10-02

### Added

- `list_keys_resp` carries a key's unit under `__meta.unit` (`KeyMeta::unit`,
  arora-types 3.2), present only when the store says one: a client tells "no
  unit" from the field's absence rather than from a null.

### Changed

- Depends on arora-types 3.2.

## [7.0.0] - 2026-09-29

### Added

- `list_keys` answers from the device, asked as the client asks: every key it
  holds, each with the `KeyMeta` its store keeps under `__meta` — the shape, the
  range, where it rests, what it is for, whether a client may write it. A module
  loaded while the device runs is listed at once, the same way `list_methods`
  already worked.
- A write is the device's to accept: it reaches the keys the device opened in
  its store and nothing else, and a refusal is the client's answer, naming the
  path, instead of an acknowledgement sent before the write was applied. Opening
  a key no longer means mirroring it into a bridge.
- The control panel lists the keys that are not controls read-only, with the
  value they held when listed and a refresh, so a device that describes nothing
  shows itself.

### Removed

- **Breaking:** the `Registry`, and with it `KeyInfo`'s declaration fields and
  method registration. A bridge relays the device and keeps nothing of its own:
  keys and their meta come from the store, functions from the modules that export
  them. A device that wants a method of its own exports it, and it is then
  reachable over every bridge rather than this one.

### Changed

- **Breaking:** the `DeviceMethods` seam is `Device` and answers for keys as well
  as methods; `AroraWSServer::set_device` replaces `set_device_methods`.
- **Breaking:** `KeyInfo` is a path and a `KeyMeta`; `list_keys_resp` carries
  `{"path": …, "__meta": {…}}` — the name every Arora bridge gives a key's meta,
  so a key segment called `meta` never collides with it.
- **Breaking:** `WriteValuesHandler` returns a future, so a write is answered
  once the device has taken it.

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

