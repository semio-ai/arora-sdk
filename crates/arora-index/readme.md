# arora-index

Neutral asset types, release selection, verification and caching. Arora can
use a local directory or a source supplied by another crate. The Studio source
lives in studio-bridge.

## Features

With no default features, the crate provides `Kind`, `Release`, `ArtifactRef`,
`ResolvedRef`, version and range parsing, `select_release`, `content_hash`, `verify` and
error types. It builds for `wasm32-unknown-unknown` without an HTTP client,
Tokio or temporary files.

The default `client` feature adds `Source`, `DirSource`, `Resolver` and `Cache`.
The `build` feature adds the build helper for bespoke HALs.

## Releases and pins

`Kind` is a string newtype. Its constants are `MODEL`, `FACE`, `ANIMATION`,
`MODULE`, `TYPE_RECORD` and `BEHAVIOR`. Unknown kinds round-trip unchanged.
Pins keep the serialized field name `assetType`. An omitted kind defaults to
`model` when reading a pin.

A `Release` has a scoped name, version and opaque source `release_id`. It has
no UUID. Its `artifacts` map holds a content hash and optional byte size for
each key. Hashes are lowercase SHA-256 hex. Models use `primary`. Modules
conventionally use `interface` and one executable key per executor and target,
for example `wasm` and `native/<target-triple>`. The index does not restrict keys.
A `ResolvedRef` pins one artifact key and hash.

Versions contain three unsigned integer components, optionally preceded by
`v`, and are at least 1.0.0. Ranges are `*`, exact, `^X.Y.Z` (before the next
major version), or `~X.Y.Z` (before the next minor version). Whole-range
whitespace is ignored. Numeric components must fit `u64`.

Selection skips invalid versions, deprecated releases, and releases without
the requested artifact or a non-empty hash. The newest matching release wins.
Equal versions keep listing order. A malformed selected hash is an error;
selection does not try an older release. Returned versions are canonical.

## Sources and resolution

`Source` is async, `Send + Sync` and object safe through `async_trait`.
It lists releases of one named asset of a kind and fetches one artifact by key.
A source maps its protocol to these `SourceError` categories:

| Category | Meaning | Resolver fallback policy |
| --- | --- | --- |
| `Network` | Transient failure, including timeouts, refused connections, 408, 429 and 5xx | Use a compatible last pin |
| `NotFound` | Definitive absence or denial for the calling identity | Clear only the requested pin and its unshared sealed blob |
| `Unauthorized` | Rejected credentials, such as 401 | Return the error and preserve pins |
| `Malformed` | Invalid metadata or protocol data | Return the error and preserve pins |
| `AccessDenied` | Ambiguous denial, such as an anonymous 403 | Return the error and preserve pins |
| `TooLarge` | A source reached its byte cap | Return the error and preserve pins |

`TokenSource` and `NoToken` are optional credential interfaces for sources.
They do not sign in or choose an authorization scheme.

```rust,no_run
use arora_index::{Cache, DirSource, Kind, Resolver};

async fn models() -> Result<(), arora_index::Error> {
    let source = DirSource::new("releases");
    let cache = Cache::new("device/assets")?;
    let resolver = Resolver::new(source, cache);
    let pin = resolver.resolve("@demo/robot", "^1.0.0", Kind::MODEL, "primary").await?;
    let fetched = resolver.fetch(&pin).await?;
    assert_eq!(fetched.content_hash, pin.content_hash);
    Ok(())
}
```

`resolve` records the live pin under the requested name. A source can return
the current name for an alias. `resolve_with_fallback` tries the source first
and reports `ResolutionSource::Source` or `Cache`. Only a network failure
permits fallback. The saved pin must match the requested kind, artifact key
and range. An absent or unreadable pin leaves the network error unchanged.

`fetch(pin)` returns verified cached bytes without asking the source. On a
miss, it looks up the exact release by name, version and source id. It checks
the pinned artifact hash before downloading. It verifies the downloaded bytes,
stores them, and returns `FetchedAsset`. It never writes `resolved.json`.
A pin supplied by another party cannot replace a resolved pin.

`Resolver::builder(source, cache)` sets `connect_timeout`, `read_timeout` and
`storage_limit`. Defaults are 30 seconds, 60 seconds and 1 GiB. Both source
methods receive `SourceOptions`. A remote source enforces the connect timeout
per connection and the read timeout per idle read. There is no total download
deadline. Sources stop reading when the byte cap is exceeded. The resolver
also checks declared sizes before download and returned sizes before caching.
Hashing and cache I/O run on blocking workers.

## Directory source

`DirSource::new(root)` reads this layout. The scope directory omits `@`:

```text
<root>/demo/robot/1.2.3/
├── release.json
├── models.glb
└── module.wasm
```

`release.json` describes this release:

```json
{
  "version": "1.2.3",
  "kind": "model",
  "deprecated": false,
  "private": false,
  "family": "robot",
  "artifacts": {
    "primary": {
      "file": "models.glb",
      "contentHash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "size": 1234
    }
  }
}
```

Use the actual file's hash and size. `size` and `family` are optional.
`deprecated` and `private` default to false. The version must equal the
directory name, which is also the source release id. Listing order is the
sorted directory order. Artifact files are relative to the release directory.
They can use subdirectories. Dot components, absolute paths and symbolic links
that escape the directory are refused. Metadata reads have a 16 MiB cap;
artifact reads use `storage_limit`. The source verifies artifact hashes.
Local files have no connection or socket read timeout.

## Cache

The caller passes the cache directory, usually `<device dir>/assets`:

```text
<device dir>/assets/
├── blobs/
│   ├── <hash>            public bytes
│   └── <hash>.sealed     encrypted private bytes
├── key                   32-byte key, mode 0600 on Unix
├── resolved.json         requested scoped name → last pin
└── .lock                 writer coordination
```

The device runtime, CLI and build-script processes that use the same cache
coordinate writes through `.lock`. Use a separate cache per identity for
private pins. `store` verifies bytes before writing. `read` verifies every hit.
Verified reads take no lock and work in read-only directories. A corrupt hit
is a miss. Cleanup takes the lock and checks the file again before deleting
it, so it cannot remove a valid concurrent replacement.

Private bytes use XSalsa20-Poly1305 with a random 24-byte nonce followed by
ciphertext. They are decrypted only in memory. The key is published atomically
without replacing a valid existing key. An invalid key is replaced under the
lock with a warning. Old ciphertext then becomes a cache miss. Blob and pin
writes use a temporary file in the same directory and atomic rename.

`save_pin` and `last_pin` maintain `resolved.json`. Its entries carry
`scopedName`, `version`, `releaseId`, `artifactKey`, `contentHash`, `assetType`,
`family`, `private` and `resolvedAt`. Pins are parsed independently. Updates
preserve untouched entries as raw JSON. Unreadable files are never overwritten.
Save and cleanup failures warn and preserve the live result or source error.
Clearing a pin deletes its sealed blob only when no other raw entry references
the hash. Public blobs and blobs without pins remain.

Direct `Cache` methods are synchronous. `fetch_async` and `store_async` use
blocking workers. Async stores accept owned or shared bytes without copying
the body on the executor.

## Build scripts

A bespoke HAL can pin public models at exact versions and hashes. Its
`build.rs` passes a Studio source from studio-bridge or a `DirSource`.
A generic HAL loads models at runtime. The project tool will pass the same
`ARORA_MODEL_<NAME>` variables from a project's pins.

```toml
[build-dependencies]
arora-index = { path = "../arora-index", features = ["build"] }

[package.metadata.arora.models.robot]
scoped_name = "@demo/robot"
version = "1.2.3"
content_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

Use the artifact's actual hash. In `build.rs`:

```rust,no_run
fn main() -> Result<(), arora_index::Error> {
    let source = arora_index::DirSource::new("releases");
    arora_index::build::models_with(&source)
}
```

The helper emits `cargo:rustc-env=ARORA_MODEL_ROBOT=<absolute path>`.
Use `include_bytes!(env!("ARORA_MODEL_ROBOT"))` in the HAL's code. The table
is plural because a crate can pin several models; each entry emits one
variable, named for that one model.
Names use lowercase letters, digits, `_` and `-`. Output names are uppercase
with `-` replaced by `_`. Colliding output names are errors. Versions must be
canonical and exact: no ranges, `v`, whitespace or leading zeroes. Hashes must
be 64 lowercase hex digits. All entries are validated before any cache lookup.
A fetched release must be public and match both version and hash.

| Variable | Cache and build behavior |
| --- | --- |
| `CARGO_MANIFEST_DIR` | Manifest directory supplied by Cargo |
| `ARORA_ASSETS_CACHE_DIR` | Explicit cache directory |
| `CARGO_HOME` | Otherwise use `$CARGO_HOME/arora-assets` |
| `HOME` | Otherwise use `$HOME/.cargo/arora-assets` |
| `ARORA_ASSETS_OFFLINE` | `1` forbids source access on a cache miss |

A verified public cache hit needs no source access. `models_with_env` accepts
explicit environment and output closures. `cache_directory` exposes the cache
lookup. The helper watches the manifest, these environment variables and each
absolute blob path. It reruns if a blob disappears. It downloads on a thread
of its own, so a build script may call it inside or outside a Tokio runtime.
Sources supply any watches
needed for their own configuration in the caller's build script.

## Tests

`cargo test -p arora-index --all-features` runs unit tests, one directory-source
resolve and fetch path, one directory-source build path, and a local doctest.
Unit tests use an in-memory source for fallback, exact pins and resolver limits.
Cache tests cover sealing, verification, publication, locking and raw pin
preservation. No test uses a remote service. See [testing](../../docs/testing.md).
