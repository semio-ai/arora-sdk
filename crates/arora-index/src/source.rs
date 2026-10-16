use crate::{Error, Kind, Release, SourceError};
use async_trait::async_trait;
use std::time::Duration;

/// Per-operation limits. A remote source applies the connect deadline to each
/// connection and the read deadline to each idle read, not the whole download.
/// Sources must bound body reads by `storage_limit`, stopping on overflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceOptions {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub storage_limit: usize,
}
impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(60),
            storage_limit: 1024 * 1024 * 1024,
        }
    }
}

/// Registry protocol boundary. Listing order breaks ties in release selection.
/// A source returns the asset's current name even for an alias. It maps its
/// protocol onto [`SourceError`]; only `Network` permits offline fallback and
/// only definitive `NotFound` clears pins. An anonymous denial is `AccessDenied`.
///
/// Implementations enforce `options` during I/O. Artifact bytes are verified
/// again by the resolver before caching. No source writes resolved.json.
#[async_trait]
pub trait Source: Send + Sync {
    async fn releases(
        &self,
        scoped_name: &str,
        kind: &Kind,
        options: SourceOptions,
    ) -> Result<Vec<Release>, SourceError>;
    async fn fetch(
        &self,
        release: &Release,
        artifact_key: &str,
        options: SourceOptions,
    ) -> Result<Vec<u8>, SourceError>;
}

#[async_trait]
impl<T: Source + ?Sized> Source for &T {
    async fn releases(
        &self,
        name: &str,
        kind: &Kind,
        options: SourceOptions,
    ) -> Result<Vec<Release>, SourceError> {
        (**self).releases(name, kind, options).await
    }
    async fn fetch(
        &self,
        release: &Release,
        key: &str,
        options: SourceOptions,
    ) -> Result<Vec<u8>, SourceError> {
        (**self).fetch(release, key, options).await
    }
}

/// Optional credentials supplied by a caller. Token acquisition is source-specific.
#[async_trait]
pub trait TokenSource: Send + Sync {
    async fn token(&self) -> Result<Option<String>, Error>;
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NoToken;
#[async_trait]
impl TokenSource for NoToken {
    async fn token(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
}
