# arora-index

This crate is the asset registry: it resolves and fetches published Studio
assets. Behavior trees use “index” for their function lookup; that lookup and
this registry are unrelated.

`Resolver` reads a scoped name, the published asset document, and every page of
releases through Firestore REST. It selects the newest non-deprecated release in
the range with a primary content hash, then `fetch` downloads that artifact
through Storage REST and verifies its SHA-256. `resolve` takes an `AssetType`
(`Model`, `Animation`, or `Face`); `Model` is the default enum value.
`latestVersion` does not select a release. An alias resolves to the asset's
current scoped name.

```rust
use arora_index::{AssetType, NoToken, Resolver, StudioConfig};

async fn example() -> Result<(), arora_index::Error> {
    let resolver = Resolver::new(StudioConfig::from_env()?, NoToken)?;
    let pin = resolver.resolve("@scope/robot", "^1.2.3", AssetType::Model).await?;
    let fetched = resolver.fetch(&pin).await?;
    assert_eq!(fetched.content_hash, pin.content_hash);
    Ok(())
}
```

`TokenSource` supplies an optional Firebase ID token asynchronously. The crate
never signs in. Firestore receives `Bearer` authorization, Storage receives
`Firebase` authorization, and `NoToken` sends neither. Anonymous 403 responses
are reported as “not found, or not public”; authenticated 403 responses are
refused. `Error` is non-exhaustive. Its asset errors distinguish invalid scoped
names and ranges, not found, refused, unauthorized, wrong asset type, no release,
hash mismatch, network failure, unexpected response, and an oversized response.
A 401 is `Unauthorized`: refresh the token or sign in again; it never permits
fallback. HTTP 408, 429 and 5xx are transient `Network` errors. Connection and
timeout errors include their source chain. Configuration, token-source, local
I/O, cache and build failures carry their corresponding context.

`Resolver::builder(config, tokens)` configures a 30-second `connect_timeout`
and a 60-second per-read idle `read_timeout`. Both are settable; there is no
total request deadline, so a large download may continue while bytes arrive.
Firestore bodies are capped at 16 MiB. Storage bodies default to a 1 GiB cap,
settable with `storage_limit(bytes)`. Bodies are consumed in chunks and stop
when the cap would be exceeded; `ResponseTooLarge` names the asset and cap.

`fetch_local(path)` returns the same `FetchedAsset` (bytes and lowercase hex
SHA-256), with `resolved: None`. A published artifact carries a `ResolvedRef`;
its `scoped_name`, canonical `version`, and `content_hash` are plain strings
that a runtime can copy into its model reference. `private` lets the runtime
decide whether it may serve those bytes. No HAL dependency is needed.

## Configuration

`StudioConfig` accepts explicit endpoints or reads these variables through
`from_env`. `from_lookup` accepts a lookup closure for deterministic configuration.
An empty value is unset for every environment variable the crate reads.

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
keeps the first candidate in listing order for ties, replacing it only
with a strictly newer version. A candidate needs a valid version and a non-empty
string at `primary.contentHash`; only a boolean `true` marks it deprecated.
Releases without fields, a valid version, or a primary hash are skipped.
The selected release must have a 64-digit lowercase hexadecimal hash and a
storage path other than an empty string, `.` or `..`. Invalid selected fields
are errors naming that release; they do not select an older release instead.
A non-string `family` is absent, with a warning. Listing shapes and
document names are checked separately. Requested names must be `@scope/name`,
with both parts non-empty, exactly one slash and no colon. Server document ids
cannot be empty, `.`, `..`, or contain a slash. Ranges are `*`, exact versions, `^X.Y.Z` (before
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

`store` verifies bytes before writing. `read` verifies every hit. A failed
verification returns a miss and takes the write lock to re-read and verify the
file before deleting it, so a concurrent writer's valid replacement is safe.
Public and private reads use distinct files. Private bytes use XSalsa20-Poly1305
with a random 24-byte nonce followed by ciphertext; encryption and decryption
take place in memory. The key is written completely to a mode-0600 temporary
file and published without replacing an existing key. An existing key of invalid
length is replaced under the cache lock with a warning; old ciphertext then
becomes a cache miss. Blob and pin updates use a temporary file in the same
directory and an atomic rename. The lock also prevents concurrent pin updates
from losing other names. Verified reads take no lock and public and private hits
work in read-only directories; a corrupt blob that cannot be removed still
returns a miss, with a warning.

`save_pin(requested_name, pin)` and `last_pin(requested_name)` maintain the
resolution map, including aliases under the name requested. Each entry has
`version`, `releaseId`, `artifactKey`, `contentHash`, `resolvedAt`, `scopedName`,
`publishedAssetId`, `family`, `private`, and `assetType`. Pins written without
`assetType` default to `Model`. It omits `storagePath`: the field is
`Option<String>` and a restored pin has `None`. `Cache::fetch(&pin)` supplies
its verified bytes; a Studio fetch without a storage path reports that the
bytes must be read from the cache by hash.

Pins are parsed independently. Invalid entries cannot supply a pin, but saving
or clearing preserves every untouched entry verbatim as raw JSON. An unreadable
file reads as empty; writes leave it untouched and warn. A file that can be read
but is not a JSON object holds no recoverable pins, and the next save replaces
it. Failure to save after a live resolve logs a warning and still returns the
live result. Unreadable pins never replace a network error with a cache error.

`resolve_with_fallback(name, range, expected_type, cache)` always tries the
server first and returns `Resolution { resolved, source }`. `source` is
`ResolutionSource::Server` or `ResolutionSource::Cache`. Only a network error
permits a saved pin, and its version and asset type must still match.
A live 404 or authenticated 403 (`Refused`) clears only the requested name's
parseable pin. Its sealed blob is deleted only if no other raw entry references
that hash, including entries that cannot supply a pin. An anonymous 403 leaves
pins intact. An absent or unparsable pin changes nothing; unrelated blobs and
public blobs remain, including bytes stored before their pin is saved.
Cleanup failures warn and preserve the original asset error.

The async resolver dispatches cache work, including pin saves and clears,
through `spawn_blocking`. Hashing and verification for Storage and local files
also run off the executor. Direct `Cache` methods are synchronous; `fetch_async`
and `store_async` run their filesystem and verification work on blocking workers.
`store_async` accepts an owned `FetchedAsset` or `Arc<FetchedAsset>`, moving or
sharing the bytes without copying the body on the executor. Resolution does not
fetch bytes: choose `Resolver::fetch` or `Cache::fetch_async` according to the returned source.
Use a separate cache per identity when storing private pins.

## Build scripts

A hardware-specific crate can pin public models in its manifest. A generic HAL
loads models at runtime and need not pin a robot model.

```toml
[build-dependencies]
arora-index = { path = "../arora-index" }

[package.metadata.arora.models.robot]
scoped_name = "@scope/robot"
version = "1.2.3"
content_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

Replace the example hash with the published artifact's SHA-256. In `build.rs`:

```rust
fn main() -> Result<(), arora_index::Error> {
    arora_index::build::models()
}
```

The helper emits `cargo:rustc-env=ARORA_MODEL_ROBOT=<absolute path>`, usable as
`include_bytes!(env!("ARORA_MODEL_ROBOT"))`. Names accept lowercase letters,
digits, `_` and `-`; output names are uppercase with `-` replaced by `_`.
Colliding output names are an error. Versions must be exact canonical versions
(no ranges, `v` prefix, whitespace or leading zeroes); a lowercase 64-digit
hash is mandatory. Every entry's scoped name, version and hash is validated
before any cache lookup or blob is exposed. A download must match both the
version and hash. Private
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
including the Firebase variables, and for each absolute blob path so Cargo
reruns the helper if a cached blob disappears. `cache_directory` exposes the
cache-directory lookup with a closure. `models_with` accepts environment lookup and
output closures for embedding and testing.

## Tests

Run `cargo test --release` for this package. Unit tests beside the implementation
exercise version selection, typed Firestore values, validation, status and
fallback decisions, bounded streams, environment lookup, build pins, and cache
files. Cache tests use temporary directories for encryption, atomic publication,
permissions and locking. The transport timeout test uses a bounded local TCP
peer; decision tests need no HTTP server.

Four end-to-end tests use `tests/fake_studio`: a public alias through paged
releases and Storage, an authenticated private chain, refused-connection
fallback followed by definitive revocation, and a pinned build followed by an
offline rebuild. Their waits have deadlines. Assertion-specific test comments
identify distinctions that line coverage cannot show. No normal test contacts
a Google or Firebase host.

The ignored emulator test also requires `ARORA_ASSETS_EMULATOR_TEST=1`, both
emulator hosts on localhost, and a project id starting with `demo-`. With an
unset or empty opt-in variable it prints a skip message and returns, even
when invoked with `--ignored`.

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
