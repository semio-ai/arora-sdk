#![doc = include_str!("../readme.md")]

pub mod build;
mod cache;
mod resolver;
mod version;

pub use cache::Cache;
pub use resolver::{NoToken, Resolver, ResolverBuilder, StudioConfig, TokenSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// A published artifact. Identity strings can be copied directly into a HAL reference.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRef {
    pub scoped_name: String,
    pub published_asset_id: String,
    pub version: String,
    pub release_id: String,
    pub artifact_key: String,
    pub content_hash: String,
    /// Absent in cached pins; offline callers retrieve bytes by hash.
    #[serde(default)]
    pub storage_path: Option<String>,
    pub family: Option<String>,
    pub private: bool,
    #[serde(default)]
    pub asset_type: AssetType,
    pub resolved_at: u64,
}

#[derive(Debug)]
pub struct FetchedAsset {
    pub bytes: Vec<u8>,
    pub content_hash: String,
    pub resolved: Option<ResolvedRef>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AssetType {
    #[default]
    Model,
    Animation,
    Face,
}

impl AssetType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Animation => "animation",
            Self::Face => "face",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionSource {
    Server,
    Cache,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub resolved: ResolvedRef,
    pub source: ResolutionSource,
}

/// Failures remain extensible; downstream matches must include a fallback arm.
///
/// ```compile_fail
/// use arora_index::Error;
/// fn exhaustive(error: Error) {
///     match error {
///         Error::NotFound { .. } | Error::Refused { .. } |
///         Error::WrongAssetType { .. } | Error::InvalidRange { .. } |
///         Error::InvalidName { .. } | Error::Unauthorized { .. } |
///         Error::ResponseTooLarge { .. } | Error::NoRelease { .. } |
///         Error::HashMismatch { .. } | Error::Network { .. } |
///         Error::UnexpectedResponse { .. } | Error::Config(_) |
///         Error::Io { .. } | Error::Token(_) | Error::Cache(_) |
///         Error::Build { .. } => {}
///     }
/// }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("{scoped_name}: {step}: {cause}")]
    NotFound {
        scoped_name: String,
        step: String,
        cause: String,
        http_status: u16,
    },
    #[error("{scoped_name}: {step}: access refused (HTTP 403 with a token)")]
    Refused { scoped_name: String, step: String },
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
    #[error("{scoped_name}: {step}: unauthorized (HTTP 401); refresh the token or sign in again")]
    Unauthorized { scoped_name: String, step: String },
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
    #[error("{scoped_name}: {step}: network error: {detail}")]
    Network {
        scoped_name: String,
        step: String,
        detail: String,
    },
    #[error("{scoped_name}: {step}: unexpected response: {detail}")]
    UnexpectedResponse {
        scoped_name: String,
        step: String,
        detail: String,
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

pub(crate) fn verify(asset: &str, expected: &str, bytes: &[u8]) -> Result<(), Error> {
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

/// Read a local override through the same byte container as a published asset.
pub async fn fetch_local(path: impl AsRef<Path>) -> Result<FetchedAsset, Error> {
    let path = path.as_ref().to_owned();
    blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| io(&path, e))?;
        Ok(FetchedAsset {
            content_hash: content_hash(&bytes),
            bytes,
            resolved: None,
        })
    })
    .await
}

/// Empty environment values have the same meaning as missing variables.
pub(crate) fn env_value(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

pub(crate) async fn blocking<R: Send + 'static>(
    work: impl FnOnce() -> Result<R, Error> + Send + 'static,
) -> Result<R, Error> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::Cache(e.to_string()))?
}
