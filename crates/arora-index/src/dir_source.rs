use crate::selection::validate_name;
use crate::{content_hash, ArtifactRef, Kind, Release, Source, SourceError, SourceOptions};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Local releases at `<root>/<scope>/<name>/<version>/release.json`.
/// See the crate readme for the manifest format. Artifact files stay inside
/// their release directory, including when a path contains symbolic links.
#[derive(Clone, Debug)]
pub struct DirSource {
    root: PathBuf,
}
impl DirSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn directory(&self) -> &Path {
        &self.root
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: String,
    kind: Kind,
    #[serde(default)]
    deprecated: bool,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    family: Option<String>,
    artifacts: BTreeMap<String, FileArtifact>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileArtifact {
    file: PathBuf,
    content_hash: String,
    #[serde(default)]
    size: Option<u64>,
}
fn malformed(detail: impl std::fmt::Display) -> SourceError {
    SourceError::Malformed(detail.to_string())
}
fn file_error(error: std::io::Error) -> SourceError {
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
            SourceError::NotFound(error.to_string())
        }
        _ => malformed(error),
    }
}
fn contained(root: &Path, path: &Path) -> Result<PathBuf, SourceError> {
    let root = root.canonicalize().map_err(file_error)?;
    let path = path.canonicalize().map_err(file_error)?;
    if !path.starts_with(&root) {
        return Err(malformed("file escapes source directory"));
    }
    Ok(path)
}
fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, SourceError> {
    let file = fs::File::open(path).map_err(file_error)?;
    let mut bytes = Vec::new();
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    if bytes.len() > limit {
        return Err(SourceError::TooLarge { limit });
    }
    Ok(bytes)
}
fn manifest(path: &Path) -> Result<Manifest, SourceError> {
    let path = contained(path, &path.join("release.json")).map_err(malformed)?;
    serde_json::from_slice(&read_limited(&path, 16 * 1024 * 1024).map_err(malformed)?)
        .map_err(malformed)
}
fn asset_path(root: &Path, name: &str) -> Result<PathBuf, SourceError> {
    validate_name(name).map_err(malformed)?;
    contained(root, &root.join(&name[1..]))
}
async fn worker<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, SourceError> + Send + 'static,
) -> Result<T, SourceError> {
    tokio::task::spawn_blocking(work).await.map_err(malformed)?
}
#[async_trait]
impl Source for DirSource {
    async fn releases(
        &self,
        name: &str,
        kind: &Kind,
        _options: SourceOptions,
    ) -> Result<Vec<Release>, SourceError> {
        let root = self.root.clone();
        let name = name.to_owned();
        let kind = kind.clone();
        worker(move || {
            let asset = asset_path(&root, &name)?;
            let mut paths = fs::read_dir(&asset)
                .map_err(file_error)?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(file_error)?;
            paths.sort();
            let mut releases = Vec::new();
            for path in paths {
                let path = contained(&asset, &path)?;
                if !path.is_dir() {
                    continue;
                }
                let document = manifest(&path)?;
                if document.kind != kind {
                    continue;
                }
                let id = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| malformed("non-UTF-8 version directory"))?;
                if document.version != id {
                    return Err(malformed("release version differs from directory name"));
                }
                releases.push(Release {
                    scoped_name: name.clone(),
                    version: document.version,
                    release_id: id.into(),
                    kind: document.kind,
                    deprecated: document.deprecated,
                    private: document.private,
                    family: document.family,
                    artifacts: document
                        .artifacts
                        .into_iter()
                        .map(|(key, artifact)| {
                            (
                                key,
                                ArtifactRef {
                                    content_hash: artifact.content_hash,
                                    size: artifact.size,
                                },
                            )
                        })
                        .collect(),
                });
            }
            Ok(releases)
        })
        .await
    }
    async fn fetch(
        &self,
        release: &Release,
        key: &str,
        options: SourceOptions,
    ) -> Result<Vec<u8>, SourceError> {
        let root = self.root.clone();
        let release = release.clone();
        let key = key.to_owned();
        worker(move || {
            let asset = asset_path(&root, &release.scoped_name)?;
            let id = Path::new(&release.release_id);
            if id.components().count() != 1
                || !matches!(id.components().next(), Some(Component::Normal(_)))
            {
                return Err(malformed("invalid directory release id"));
            }
            let directory = contained(&asset, &asset.join(id))?;
            let document = manifest(&directory)?;
            if document.version != release.version
                || document.kind != release.kind
                || document.private != release.private
            {
                return Err(malformed("release metadata changed before fetch"));
            }
            let artifact = document
                .artifacts
                .get(&key)
                .ok_or_else(|| SourceError::NotFound(format!("artifact {key}")))?;
            if release
                .artifacts
                .get(&key)
                .is_none_or(|a| a.content_hash != artifact.content_hash)
            {
                return Err(malformed("artifact metadata changed before fetch"));
            }
            if artifact
                .file
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(malformed(
                    "artifact file must be a relative path without dot components",
                ));
            }
            let path = contained(&directory, &directory.join(&artifact.file))?;
            let bytes = read_limited(&path, options.storage_limit)?;
            if content_hash(&bytes) != artifact.content_hash {
                return Err(malformed(format!(
                    "{}: artifact {key}: SHA-256 mismatch",
                    release.scoped_name
                )));
            }
            Ok(bytes)
        })
        .await
    }
}
#[cfg(test)]
mod tests;
