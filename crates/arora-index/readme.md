# arora-index

This crate is the asset registry: it resolves and fetches published Studio
assets. Behavior trees use “index” for their function lookup; that lookup and
this registry are unrelated.

`Resolver` reads a scoped name, the published asset document, and every page of
releases through Firestore REST. It selects the newest non-deprecated release
in the range with a primary content hash, then `fetch` downloads that artifact
through Storage REST and verifies its SHA-256. `resolve` expects a model;
`resolve_as` accepts another asset type. `latestVersion` does not select a release.
An alias resolves to the asset's current scoped name.

```rust,no_run
use arora_index::{NoToken, Resolver, StudioConfig};

# async fn example() -> Result<(), arora_index::Error> {
let resolver = Resolver::new(StudioConfig::from_env()?, NoToken)?;
let pin = resolver.resolve("@scope/robot", "^1.2.3").await?;
let fetched = resolver.fetch(&pin).await?;
assert_eq!(fetched.content_hash, pin.content_hash);
# Ok(())
# }
```

`TokenSource` supplies an optional Firebase ID token asynchronously. The crate
never signs in. Firestore receives `Bearer` authorization, Storage receives
`Firebase` authorization, and `NoToken` sends neither. Anonymous 403 responses
are reported as “not found, or not public”; authenticated 403 responses are
refused. Network failures, malformed responses, wrong types, invalid ranges,
missing releases and hash mismatches have distinct errors.

`fetch_local(path)` returns the same `FetchedAsset` (bytes and lowercase hex
SHA-256), with `resolved: None`. A published artifact carries a `ResolvedRef`;
its `scoped_name`, canonical `version`, and `content_hash` are plain strings
that a runtime can copy into its model reference. `private` lets the runtime
decide whether it may serve those bytes. No HAL dependency is needed.

## Configuration

`StudioConfig` accepts explicit endpoints or reads these variables through
`from_env`. `from_lookup` accepts a lookup closure for deterministic configuration.

| Variable | Meaning |
| --- | --- |
| `FIREBASE_PROJECT_ID` | Required Firebase project id |
| `FIREBASE_STORAGE_BUCKET` | Required Storage bucket |
| `FIREBASE_FIRESTORE_EMULATOR_HOST` | Optional `host:port`, used as `http://host:port` |
| `FIREBASE_STORAGE_EMULATOR_HOST` | Optional `host:port`, used as `http://host:port` |

Without emulator variables, the endpoints are `https://firestore.googleapis.com`
and `https://firebasestorage.googleapis.com`. Documents use the `(default)`
database: `scopedNames/{name-with-colons}`, `publishedAssets/{id}`, and
`publishedAssets/{id}/releases`. The primary artifact's `storage.fileBucket`
is the object path within the configured bucket.

Versions contain three unsigned integer components, optionally preceded by
`v`, and must be at least 1.0.0. Selection compares components numerically and
keeps listing order for ties. Ranges are `*`, exact versions, `^X.Y.Z` (before
`(X+1).0.0`), or `~X.Y.Z` (before `X.(Y+1).0`). Whitespace around the whole
range is ignored. Components must fit `u64`; upper bounds use wider arithmetic
so the maximum component remains usable.

## Cache

The caller chooses a directory with `Cache::new`, usually `<device dir>/assets`:

```text
<dir>/
├── blobs/<contentHash>          public bytes
├── blobs/<contentHash>.sealed   encrypted private bytes
├── key                          32-byte key, mode 0600 on Unix
├── resolved.json                requested scoped name → last pin
└── .lock                        coordination across processes
```

`store` verifies bytes before writing. `read` verifies every hit, removing
corrupt blobs and returning a miss. Public and private reads use distinct
files. Private bytes use XSalsa20-Poly1305 with a random 24-byte nonce followed
by ciphertext; encryption and decryption take place in memory. The key is
created exclusively and shared under the cache lock. Blob and pin updates use
a temporary file in the same directory and an atomic rename. The lock also
prevents concurrent pin updates from losing other names.

`save_pin(requested_name, pin)` and `last_pin(requested_name)` maintain the
resolution map, including aliases under the name requested. Each entry has
`version`, `releaseId`, `artifactKey`, `contentHash`, `resolvedAt`, `scopedName`,
`publishedAssetId`, `family`, and `private`. It omits `storagePath`: a restored
pin has an empty path and `Cache::fetch(&pin)` supplies its verified bytes.

`resolve_with_fallback` always tries the server first and saves a successful
model resolution. Only a network error (including a 5xx response) permits the
last pin, and only if its version still satisfies the range. This method does
not fetch bytes; use `Resolver::fetch` for an online pin or `Cache::fetch` for
cached bytes. Cache methods perform synchronous filesystem I/O; applications
with latency-sensitive async tasks can call them through `spawn_blocking`.
Use a separate cache per identity when storing private pins.

## Build scripts

A hardware-specific crate can pin public models in its manifest. A generic HAL
loads models at runtime and need not pin a robot model.

```toml
[build-dependencies]
arora-index = { path = "../arora-index" }

[package.metadata.arora.models]
robot = { scoped_name = "@scope/robot", version = "1.2.3", content_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" }
```

Replace the example hash with the published artifact's SHA-256. In `build.rs`:

```rust,no_run
fn main() -> Result<(), arora_index::Error> {
    arora_index::build::models()
}
```

The helper emits `cargo:rustc-env=ARORA_MODEL_ROBOT=<absolute path>`, usable as
`include_bytes!(env!("ARORA_MODEL_ROBOT"))`. Names accept lowercase letters,
digits, `_` and `-`; output names are uppercase with `-` replaced by `_`.
Colliding output names are an error. Versions must be exact canonical versions
(no ranges, `v` prefix, whitespace or leading zeroes); a lowercase 64-digit
hash is mandatory. A download must match both the version and hash. Private
assets are refused. The helper uses a current-thread Tokio runtime and the
same resolver; call it outside an existing async runtime.

| Variable | Build helper meaning |
| --- | --- |
| `CARGO_MANIFEST_DIR` | Manifest directory supplied by Cargo |
| `ARORA_ASSETS_CACHE_DIR` | Explicit cache directory |
| `CARGO_HOME` | Otherwise use `$CARGO_HOME/arora-assets` |
| `HOME` | Otherwise use `$HOME/.cargo/arora-assets` |
| `ARORA_ASSETS_OFFLINE` | `1` forbids network access |

A verified public cache hit needs no Firebase configuration. To fill a missing
pin, build once with offline mode unset and Firebase configuration supplied.
The helper emits rerun directives for the manifest and every variable it reads,
including the Firebase variables. `models_with` accepts environment lookup and
output closures for embedding and testing.

## Tests

Normal tests use temporary directories and localhost `wiremock` servers.
The ignored emulator test also requires `ARORA_ASSETS_EMULATOR_TEST=1`, both
emulator hosts on localhost, and a project id starting with `demo-`.

From a semio_studio checkout, start the emulators explicitly with a demo project:

```sh
firebase emulators:start --only firestore,storage --project demo-arora-assets
```

Then, from the SDK workspace:

```sh
ARORA_ASSETS_EMULATOR_TEST=1 \
FIREBASE_PROJECT_ID=demo-arora-assets \
FIREBASE_STORAGE_BUCKET=demo-arora-assets.appspot.com \
FIREBASE_FIRESTORE_EMULATOR_HOST=127.0.0.1:8080 \
FIREBASE_STORAGE_EMULATOR_HOST=127.0.0.1:9199 \
cargo test --manifest-path crates/arora-index/Cargo.toml --test emulator -- --ignored
```

The `demo-` project keeps requests local. Always supply `--project`; the Studio
checkout's default project is production. The test seeds public and private
fixtures, fetches the public model anonymously, and checks that the private
model returns “not found, or not public”.
