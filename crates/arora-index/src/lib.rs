#![doc = include_str!("../readme.md")]

pub mod build;
mod cache;
mod resolver;
mod version;

pub use cache::Cache;
pub use resolver::{NoToken, Resolver, StudioConfig, TokenSource};
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
    pub storage_path: String,
    pub family: Option<String>,
    pub private: bool,
    pub resolved_at: u64,
}

#[derive(Debug)]
pub struct FetchedAsset {
    pub bytes: Vec<u8>,
    pub content_hash: String,
    pub resolved: Option<ResolvedRef>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{scoped_name}: {step}: {cause}")]
    NotFound {
        scoped_name: String,
        step: String,
        cause: String,
    },
    #[error("{scoped_name}: {step}: access refused (HTTP 403 with a token)")]
    Refused { scoped_name: String, step: String },
    #[error("{scoped_name}: expected asset type {expected}, got {actual}")]
    WrongAssetType {
        scoped_name: String,
        expected: String,
        actual: String,
    },
    #[error("invalid version range {range:?}")]
    InvalidRange { range: String },
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
    let path = path.as_ref();
    let bytes = tokio::fs::read(path).await.map_err(|e| io(path, e))?;
    Ok(FetchedAsset {
        content_hash: content_hash(&bytes),
        bytes,
        resolved: None,
    })
}
