# arora-index changelog

## [0.1.0]

### Added

- Feature-free asset kinds, keyed artifacts, releases, exact pins, version and
  range parsing, stable release selection, SHA-256 verification and error types.
  These types build for `wasm32-unknown-unknown`. Unknown kinds are preserved.
- An async source trait with protocol error categories and caller-supplied
  timeout and size limits. A directory source reads local release manifests
  and verifies artifact files.
- A generic resolver with network-only fallback, definitive pin removal and
  exact-pin hash checks before download. Fetches cache verified bytes without
  replacing resolved pins.
- A content-addressed cache with sealed private bytes, atomic publication,
  writer locking, verified reads and independent pin recovery.
- A build helper for bespoke HALs with an explicit source, exact public models,
  manifest validation, offline cache hits and Cargo output and change watches.
