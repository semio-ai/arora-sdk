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
  prefix. A connection queues what it has not written, up to
  `UnixBridge::BACKLOG_BYTES` of values, and keys a `subscribe` names
  newest only are replaced rather than queued, so a slow client skips camera
  frames without falling behind on the rest; the answers a connection has to
  write have a cap of the same size. A client that closes its writing half is
  still written its answers. The socket file is bound in a private directory
  and given its mode before it is moved into place, so no one connects before
  it is set. It replaces a stale socket file (one refusing connections),
  refuses one a device answers on and any file that is not a socket, and
  removes its own on drop.
- `UnixClient` and `Changes`: a connection to it, locally or through an `ssh`
  forward of the file, or over any byte stream (`UnixClient::over`);
  `UnixClient` implements `arora_bridge::Caller`; `subscribe_latest` names
  the keys pushed newest only. It reads while it writes a request, and closes
  the connection when its unread `Changes` pass `UnixBridge::BACKLOG_BYTES`.
- `relay.py`: joins a command's standard streams to the socket file, for an
  sshd too old to forward one (OpenSSH before 6.7). Python 2.6+ or 3. Each
  way has its own buffer, so a slow side never stops the other way.
- `wire`: the messages' encoding, for clients in other languages.
