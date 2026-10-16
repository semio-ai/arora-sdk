//! Neutral asset types, release selection and SHA-256 verification.
//! The default `client` feature adds a source trait, a directory source and a
//! resolver with a sealed cache. The `build` feature adds bespoke HAL build pins.
//! No default features are needed for `wasm32-unknown-unknown`.
//!
//! Module artifact keys conventionally include `interface`, `wasm` and
//! `native/<target-triple>`. Keys are open strings. Models use `primary`.
//!
//! ```
//! # #[cfg(feature = "client")]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use arora_index::{content_hash, Cache, DirSource, Kind, Resolver};
//! let dir = tempfile::tempdir()?;
//! let release = dir.path().join("source/demo/robot/1.0.0");
//! std::fs::create_dir_all(&release)?;
//! std::fs::write(release.join("model.glb"), b"model bytes")?;
//! std::fs::write(release.join("release.json"), serde_json::to_vec(&serde_json::json!({
//!     "version": "1.0.0", "kind": "model", "deprecated": false,
//!     "artifacts": {"primary": {"file": "model.glb", "contentHash": content_hash(b"model bytes")}}
//! }))?)?;
//! let resolver = Resolver::new(DirSource::new(dir.path().join("source")),
//!     Cache::new(dir.path().join("assets"))?);
//! tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
//!     let pin = resolver.resolve("@demo/robot", "^1.0.0", Kind::MODEL, "primary").await?;
//!     assert_eq!(resolver.fetch(&pin).await?.bytes, b"model bytes");
//!     Ok::<_, arora_index::Error>(())
//! })?;
//! # Ok(()) }
//! # #[cfg(not(feature = "client"))]
//! # fn main() {}
//! ```

#[cfg(feature = "build")]
pub mod build;
#[cfg(feature = "client")]
mod cache;
#[cfg(feature = "client")]
mod dir_source;
#[cfg(feature = "client")]
mod resolver;
mod selection;
#[cfg(feature = "client")]
mod source;
pub mod version;

#[cfg(feature = "client")]
pub use cache::Cache;
#[cfg(feature = "client")]
pub use dir_source::DirSource;
#[cfg(feature = "client")]
pub use resolver::{Resolver, ResolverBuilder};
pub use selection::select_release;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(feature = "client")]
pub use source::{NoToken, Source, SourceOptions, TokenSource};
use std::borrow::Cow;
use std::collections::BTreeMap;
#[cfg(feature = "client")]
use std::path::Path;
use std::path::PathBuf;

/// An extensible kind, serialized as a plain string. Unknown kinds are preserved.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct Kind(pub Cow<'static, str>);

impl Kind {
    pub const MODEL: Self = Self(Cow::Borrowed("model"));
    pub const FACE: Self = Self(Cow::Borrowed("face"));
    pub const ANIMATION: Self = Self(Cow::Borrowed("animation"));
    pub const MODULE: Self = Self(Cow::Borrowed("module"));
    pub const TYPE_RECORD: Self = Self(Cow::Borrowed("type-record"));
    pub const BEHAVIOR: Self = Self(Cow::Borrowed("behavior"));
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<String> for Kind {
    fn from(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}
impl From<&str> for Kind {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl Default for Kind {
    fn default() -> Self {
        Self::MODEL
    }
}

/// Content identity for one key in a release. Size is in bytes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub content_hash: String,
    #[serde(default)]
    pub size: Option<u64>,
}

/// Metadata returned by a source. `release_id` is opaque to the index.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub scoped_name: String,
    pub version: String,
    pub release_id: String,
    pub kind: Kind,
    #[serde(default)]
    pub deprecated: bool,
    pub artifacts: BTreeMap<String, ArtifactRef>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub private: bool,
}

/// An exact artifact pin. `assetType` is the serialized kind field in resolved.json.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRef {
    pub scoped_name: String,
    pub version: String,
    pub release_id: String,
    pub artifact_key: String,
    pub content_hash: String,
    pub family: Option<String>,
    pub private: bool,
    #[serde(default)]
    pub asset_type: Kind,
    pub resolved_at: u64,
}

#[derive(Debug)]
pub struct FetchedAsset {
    pub bytes: Vec<u8>,
    pub content_hash: String,
    pub resolved: Option<ResolvedRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionSource {
    Source,
    Cache,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub resolved: ResolvedRef,
    pub source: ResolutionSource,
}

/// Protocol categories used by the fallback policy. Sources preserve diagnostic context.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SourceError {
    /// Transient failures: timeouts, refused connections, HTTP 408, 429 and 5xx.
    #[error("network: {0}")]
    Network(String),
    /// Definitive absence or denial for this identity. Clears the named saved pin.
    #[error("not found or no access: {0}")]
    NotFound(String),
    /// Rejected credentials (HTTP 401). Never allows fallback or pin removal.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    /// Invalid metadata or protocol data. Never allows fallback or pin removal.
    #[error("malformed data: {0}")]
    Malformed(String),
    /// Ambiguous denial, such as an anonymous HTTP 403. Preserves pins without fallback.
    #[error("access denied: {0}")]
    AccessDenied(String),
    /// A source stopped reading at its byte cap. Preserves pins without fallback.
    #[error("response exceeds the {limit} byte cap")]
    TooLarge { limit: usize },
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("{scoped_name}: expected asset type {expected}, got {actual}")]
    WrongAssetType {
        scoped_name: String,
        expected: String,
        actual: String,
    },
    #[error("{scoped_name}: invalid version range {range:?}")]
    InvalidRange { scoped_name: String, range: String },
    #[error("{scoped_name}: invalid scoped name; expected @scope/name without colons")]
    InvalidName { scoped_name: String },
    #[error("{scoped_name}: {step}: response exceeds the {limit} byte cap")]
    ResponseTooLarge {
        scoped_name: String,
        step: String,
        limit: usize,
    },
    #[error("{scoped_name}: no release in range {range:?}")]
    NoRelease { scoped_name: String, range: String },
    #[error("{asset}: SHA-256 mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    #[error("configuration: {0}")]
    Config(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("token source: {0}")]
    Token(String),
    #[error("cache: {0}")]
    Cache(String),
    #[error("model entry {entry:?}: {detail}")]
    Build { entry: String, detail: String },
}

#[cfg(feature = "client")]
pub(crate) fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// Lowercase hexadecimal SHA-256, the published content identity.
pub fn content_hash(bytes: &[u8]) -> String {
    log::trace!(target: "arora-assets", "computing SHA-256 for {} bytes", bytes.len());
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Verify bytes against a lowercase SHA-256 content identity.
pub fn verify(asset: &str, expected: &str, bytes: &[u8]) -> Result<(), Error> {
    let actual = content_hash(bytes);
    if actual != expected {
        return Err(Error::HashMismatch {
            asset: asset.into(),
            expected: expected.into(),
            actual,
        });
    }
    Ok(())
}

/// Empty environment values have the same meaning as missing variables.
#[cfg(feature = "build")]
pub(crate) fn env_value(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

#[cfg(feature = "client")]
pub(crate) async fn blocking<R: Send + 'static>(
    work: impl FnOnce() -> Result<R, Error> + Send + 'static,
) -> Result<R, Error> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::Cache(e.to_string()))?
}

#[cfg(all(test, feature = "client"))]
mod test_support;

#[cfg(test)]
mod tests;
