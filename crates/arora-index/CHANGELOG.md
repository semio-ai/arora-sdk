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
  exact, caret, tilde or wildcard ranges, and a non-empty primary hash.
  Deprecated releases and invalid versions are skipped; equal versions keep
  listing order, and returned versions are canonical. Invalid selected hashes or storage paths name the release
  in an error; non-string families are absent with a warning.
- SHA-256-verified Storage fetching and local-file fetching through
  `FetchedAsset`. Configurable 30-second connect and 60-second read-idle
  timeouts have no total deadline. Streamed responses are capped at 16 MiB
  for Firestore and a configurable 1 GiB by default for Storage.
- Non-exhaustive errors for invalid names and ranges, not found or not public,
  refused access, `Unauthorized` (401), wrong asset types, missing releases,
  hash mismatches, oversized or unexpected responses, configuration, token,
  file, cache and build failures. Network errors include their source chain;
  connection failures, timeouts, 408, 429 and 5xx are transient.
- Content-addressed caching with verified, lock-free hits and atomic writes.
  Failed verification re-checks under the write lock before deleting a blob.
  Private blobs are sealed with XSalsa20-Poly1305 and decrypted only into
  memory; keys are published atomically with user-only permissions. Blocking
  cache work, hashing and verification run off the async executor. Async stores
  accept owned or shared bytes without copying the body on the executor.
- Tolerant `resolved.json` pins keyed by the requested name. Network-only
  fallback reports `Server` or `Cache`; cached pins have no storage path.
  Writes preserve untouched entries as raw JSON and never overwrite unreadable
  files. A 404 or authenticated 403 clears only its name's parseable pin and
  its sealed blob if no other raw entry references the hash; anonymous 403s
  preserve pins. Save and cleanup failures warn without replacing live results.
- A synchronous build helper for exact public model version and hash pins,
  validation of every pin before cache lookup, verified hits without Firebase
  settings, offline builds, and Cargo environment and blob watches. Empty environment values are unset.
