# Changelog

All notable changes to `arora-bridge-unix`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-17

### Added

- `UnixBridge`: a device's bridge over a Unix socket file (mode `0600`), in
  Arora Buffers back to back. It carries `get`, `update`, `call`,
  `list_keys` and `describe_methods` with requests in flight together, and
  pushes each step's changes to the connections subscribed to their keys by
  prefix. It replaces a stale socket file, refuses one a device answers on,
  and removes its own on drop.
- `UnixClient` and `Changes`: a connection to it, locally or through an `ssh`
  forward of the file, or over any byte stream (`UnixClient::over`);
  `UnixClient` implements `arora_bridge::Caller`.
- `relay.py`: joins a command's standard streams to the socket file, for an
  sshd too old to forward one (OpenSSH before 6.7). Python 2.6+ or 3.
- `wire`: the messages' encoding, for clients in other languages.
