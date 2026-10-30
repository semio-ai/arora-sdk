# Changelog

All notable changes to `arora-bridge-ws`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [7.2.5] - 2026-10-30

### Changed

- The WebSocket server builds on `tokio-tungstenite` 0.30, with the same
  message and frame size limits.

## [7.2.4] - 2026-10-12

### Fixed

- The control panel's number box writes any value within the bounds a key's
  meta states and its type holds: a key without a range (a joint target in
  radians, a counter) takes values beyond 0..1, which only its slider spans.
  An integer key's values stay within its type (`u8` within 0..255, a 64-bit
  integer within the integers a JavaScript number holds exactly).
- The control panel reads `i16`, `i8`, `u16` and `u8` values as numbers, for
  its controls and a method's arguments alike.

## [7.2.3] - 2026-10-12

### Changed

- The control panel shows a refused write in its status line with the device's
  reason (`Write refused: …`) until the device takes a write, and reads back
  the controls with no write waiting, so they show what the device holds
  rather than the value it refused. The device takes or refuses a batch of
  writes whole. A message the server could not read shows its `error` answer
  in the status line.

## [7.2.2] - 2026-10-10

### Fixed

- The control panel keeps each key exactly as the device lists it, for display,
  lookup and writes: `/a` and `a//b` are written as `/a` and `a//b`, not as `a`
  and `a/b`, since any string is a key and two keys that differ by a slash are
  two keys. A control's inputs name it by its index rather than by its path, so
  a key holding quotes, angle brackets, backslashes or control characters is
  written as listed; a row whose label would not tell it from a sibling's shows
  the full path.

## [7.2.1] - 2026-10-06

### Changed

- The control panel writes a key as the variant its meta states (`f32`, `i32`,
  `u8`, …), integers rounded, rather than as `f64` for every numeric key: a
  device refuses a write that is not of its key's type.

## [7.2.0] - 2026-10-05

### Changed

- `invoke` of a method name several modules export fails, naming those
  modules, instead of calling one of them. The `invoke` message names a method
  by its bare name only, so such a method is unreachable over this wire;
  `list_methods` lists it once per module.
- `KeyInfo`, `MethodInfo` and `MethodParam` are `arora_bridge::client`'s,
  re-exported at the same paths: every client of a device reads keys and
  methods in these shapes. Calling by name and starting and halting runs use
  `arora_bridge::client` likewise.
- Depends on arora-bridge 6.1; no longer on arora-behavior-tree-types.

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

