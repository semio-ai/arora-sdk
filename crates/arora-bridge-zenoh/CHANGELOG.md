# Changelog

All notable changes to `arora-bridge-zenoh`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-18

### Added

- `ZenohBridge`: a device's bridge over Zenoh, under a prefix of a session
  the application opened. It answers queries on `PREFIX/rpc` holding a
  request (`get`, `update`, `call`, `list_keys`, `describe_methods`), each
  request and answer one Arora Buffer, and publishes each step's change key
  by key on `PREFIX/state/PATH`: a put holding the value as an Arora Buffer
  (an option), a delete for an unset key. It tells the device whether a
  subscriber matches its keys (`DataRequested`), and publishes from a thread
  of its own, what waits being each key's newest value only, without
  holding replaced frames. It holds up to `ZenohBridge::REQUESTS_IN_FLIGHT`
  requests, answers a query beyond with an error, and refuses a request
  over 64 MiB.
- `ZenohClient`: requests as queries, sent once the session knows a device
  on the prefix (up to `ZenohClient::DEVICE_WAIT`), and subscriptions by key
  prefix delivered on `Changes`, one key per change, unread changes held
  newest value only per key, `subscribe` returning once the device
  publishes to them; it implements `Caller`.
