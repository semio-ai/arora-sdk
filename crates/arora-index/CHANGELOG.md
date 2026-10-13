# arora-index changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-12

### Added

- Async Studio resolution with `AssetType::{Model, Animation, Face}` (default
  `Model`), aliases, and caller-supplied identity through `TokenSource` or
  `NoToken`; the resolver never signs in.
- Numeric release selection from every page: valid versions at least 1.0.0,
  exact, caret, tilde or wildcard ranges, and the newest usable primary
  artifact. Malformed and deprecated releases are skipped; equal versions
  keep listing order, and returned versions are canonical.
- SHA-256-verified Storage fetching and local-file fetching through
  `FetchedAsset`. Configurable 30-second connect and 60-second read-idle
  timeouts have no total deadline. Streamed responses are capped at 16 MiB
  for Firestore and a configurable 1 GiB by default for Storage.
- Non-exhaustive errors for invalid names and ranges, not found or not public,
  refused access, `Unauthorized` (401), wrong asset types, missing releases,
  hash mismatches, oversized or unexpected responses, configuration, token,
  file, cache and build failures. Network errors include their source chain;
  connection failures, timeouts, 408, 429 and 5xx are transient.
- Content-addressed caching with verified, lock-free reads and atomic writes.
  Private blobs are sealed with XSalsa20-Poly1305 and decrypted only into
  memory; keys are published atomically with user-only permissions. Blocking
  cache work runs off the async executor.
- Tolerant `resolved.json` pins keyed by the requested name. Network-only
  fallback reports `Server` or `Cache`; cached pins have no storage path.
  A definitive not-found or refused response clears the pin and unreferenced
  sealed blobs. A failed pin save warns without failing a live resolution.
- A synchronous build helper for exact public model version and hash pins,
  verified cache hits without Firebase settings, offline builds, and Cargo
  environment and blob watches. Empty environment values are unset.
