use crate::selection::validate_name;
use crate::version::{Range, Version};
use crate::{
    select_release, valid_hash, Cache, Error, FetchedAsset, Kind, Release, Resolution,
    ResolutionSource, ResolvedRef, Source, SourceError, SourceOptions,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A source and a caller-owned cache directory, usually `<device dir>/assets`.
pub struct Resolver<S: Source> {
    source: S,
    cache: Cache,
    options: SourceOptions,
}
pub struct ResolverBuilder<S: Source> {
    resolver: Resolver<S>,
}
impl<S: Source> ResolverBuilder<S> {
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.resolver.options.connect_timeout = timeout;
        self
    }
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.resolver.options.read_timeout = timeout;
        self
    }
    pub fn storage_limit(mut self, bytes: usize) -> Self {
        self.resolver.options.storage_limit = bytes;
        self
    }
    pub fn build(self) -> Resolver<S> {
        self.resolver
    }
}
impl<S: Source> Resolver<S> {
    pub fn new(source: S, cache: Cache) -> Self {
        Self {
            source,
            cache,
            options: SourceOptions::default(),
        }
    }
    pub fn builder(source: S, cache: Cache) -> ResolverBuilder<S> {
        ResolverBuilder {
            resolver: Self::new(source, cache),
        }
    }

    /// Resolve live and record the pin under the requested name. Does not download bytes.
    pub async fn resolve(
        &self,
        name: &str,
        range: &str,
        kind: Kind,
        artifact_key: &str,
    ) -> Result<ResolvedRef, Error> {
        validate_name(name)?;
        Range::parse(name, range)?;
        let releases = self.source.releases(name, &kind, self.options).await?;
        let release = select_release(&releases, name, range, artifact_key)?;
        validate_release(release, &kind)?;
        let pin = ResolvedRef {
            scoped_name: release.scoped_name.clone(),
            version: Version::parse(&release.version).unwrap().to_string(),
            release_id: release.release_id.clone(),
            artifact_key: artifact_key.into(),
            content_hash: release.artifacts[artifact_key].content_hash.clone(),
            family: release.family.clone(),
            private: release.private,
            asset_type: kind,
            resolved_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let saved = pin.clone();
        let requested = name.to_owned();
        if let Err(error) = self
            .cache
            .blocking(move |cache| cache.save_pin(&requested, &saved))
            .await
        {
            log::warn!(target: "arora-assets", "{name}: could not save resolved pin: {error}");
        }
        Ok(pin)
    }

    /// Only a network failure permits a saved pin compatible with kind, key and range.
    pub async fn resolve_with_fallback(
        &self,
        name: &str,
        range: &str,
        kind: Kind,
        artifact_key: &str,
    ) -> Result<Resolution, Error> {
        let parsed = Range::parse(name, range)?;
        match self.resolve(name, range, kind.clone(), artifact_key).await {
            Ok(resolved) => Ok(Resolution {
                resolved,
                source: ResolutionSource::Source,
            }),
            Err(error) => {
                match &error {
                    Error::Source(SourceError::Network(_)) => {
                        let requested = name.to_owned();
                        if let Ok(Some(pin)) = self
                            .cache
                            .blocking(move |cache| cache.last_pin(&requested))
                            .await
                        {
                            if pin.asset_type == kind
                                && pin.artifact_key == artifact_key
                                && valid_hash(&pin.content_hash)
                                && Version::parse(&pin.version).is_some_and(|v| parsed.contains(v))
                            {
                                return Ok(Resolution {
                                    resolved: pin,
                                    source: ResolutionSource::Cache,
                                });
                            }
                        }
                    }
                    Error::Source(SourceError::NotFound(_)) => {
                        let requested = name.to_owned();
                        if let Err(cleanup) = self
                            .cache
                            .blocking(move |cache| cache.remove_pin(&requested))
                            .await
                        {
                            log::warn!(target: "arora-assets", "{name}: could not clear resolved pin: {cleanup}");
                        }
                    }
                    _ => {}
                }
                Err(error)
            }
        }
    }

    /// Return verified cached bytes or check the exact release and hash before download.
    /// This method stores blobs but never records or replaces a resolved pin.
    pub async fn fetch(&self, pin: &ResolvedRef) -> Result<FetchedAsset, Error> {
        if let Some(asset) = self.cache.fetch_async(pin).await? {
            return Ok(asset);
        }
        validate_name(&pin.scoped_name)?;
        let releases = self
            .source
            .releases(&pin.scoped_name, &pin.asset_type, self.options)
            .await?;
        let release = releases
            .iter()
            .find(|r| {
                r.release_id == pin.release_id
                    && r.scoped_name == pin.scoped_name
                    && Version::parse(&r.version).is_some_and(|v| v.to_string() == pin.version)
            })
            .ok_or_else(|| {
                SourceError::NotFound(format!(
                    "{}: pinned release {} at {}",
                    pin.scoped_name, pin.release_id, pin.version
                ))
            })?;
        validate_release(release, &pin.asset_type)?;
        if release.private != pin.private {
            return Err(SourceError::Malformed(format!(
                "{}: pinned visibility differs",
                pin.scoped_name
            ))
            .into());
        }
        let artifact = release.artifacts.get(&pin.artifact_key).ok_or_else(|| {
            SourceError::NotFound(format!(
                "{}: artifact {}",
                pin.scoped_name, pin.artifact_key
            ))
        })?;
        if artifact.content_hash != pin.content_hash {
            return Err(Error::HashMismatch {
                asset: pin.scoped_name.clone(),
                expected: pin.content_hash.clone(),
                actual: artifact.content_hash.clone(),
            });
        }
        if artifact
            .size
            .is_some_and(|size| size > self.options.storage_limit as u64)
        {
            return Err(self.too_large(pin));
        }
        let bytes = self
            .source
            .fetch(release, &pin.artifact_key, self.options)
            .await?;
        if bytes.len() > self.options.storage_limit {
            return Err(self.too_large(pin));
        }
        let pin = pin.clone();
        let cache = self.cache.clone();
        crate::blocking(move || {
            cache.store(&pin.content_hash, &bytes, pin.private)?;
            Ok(FetchedAsset {
                bytes,
                content_hash: pin.content_hash.clone(),
                resolved: Some(pin),
            })
        })
        .await
    }
    fn too_large(&self, pin: &ResolvedRef) -> Error {
        Error::ResponseTooLarge {
            scoped_name: pin.scoped_name.clone(),
            step: "artifact bytes".into(),
            limit: self.options.storage_limit,
        }
    }
}
fn validate_release(release: &Release, kind: &Kind) -> Result<(), Error> {
    validate_name(&release.scoped_name)?;
    if release.kind != *kind {
        return Err(Error::WrongAssetType {
            scoped_name: release.scoped_name.clone(),
            expected: kind.as_str().into(),
            actual: release.kind.as_str().into(),
        });
    }
    if release.release_id.is_empty() {
        return Err(SourceError::Malformed("empty release id".into()).into());
    }
    Ok(())
}
#[cfg(test)]
mod tests;
