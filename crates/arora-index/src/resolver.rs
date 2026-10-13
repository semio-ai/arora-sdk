use crate::version::{Range, Version};
use crate::{verify, Cache, Error, FetchedAsset, ResolvedRef};
use async_trait::async_trait;
use reqwest::{Client, Url};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct StudioConfig {
    pub project_id: String,
    pub storage_bucket: String,
    pub firestore_url: String,
    pub storage_url: String,
}

impl StudioConfig {
    pub fn from_env() -> Result<Self, Error> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, Error> {
        let mut required = |name| {
            lookup(name)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| Error::Config(format!("missing {name}")))
        };
        let project_id = required("FIREBASE_PROJECT_ID")?;
        let storage_bucket = required("FIREBASE_STORAGE_BUCKET")?;
        let firestore_url = lookup("FIREBASE_FIRESTORE_EMULATOR_HOST")
            .map(|host| format!("http://{host}"))
            .unwrap_or_else(|| "https://firestore.googleapis.com".into());
        let storage_url = lookup("FIREBASE_STORAGE_EMULATOR_HOST")
            .map(|host| format!("http://{host}"))
            .unwrap_or_else(|| "https://firebasestorage.googleapis.com".into());
        Ok(Self {
            project_id,
            storage_bucket,
            firestore_url,
            storage_url,
        })
    }
}

/// Supplies identity only; the resolver never signs in.
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

pub struct Resolver<T = NoToken> {
    config: StudioConfig,
    tokens: T,
    client: Client,
}

impl<T: TokenSource> Resolver<T> {
    pub fn new(config: StudioConfig, tokens: T) -> Result<Self, Error> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self {
            config,
            tokens,
            client,
        })
    }

    pub async fn resolve(&self, scoped_name: &str, range: &str) -> Result<ResolvedRef, Error> {
        self.resolve_as(scoped_name, range, "model").await
    }

    pub async fn resolve_as(
        &self,
        scoped_name: &str,
        range: &str,
        expected_type: &str,
    ) -> Result<ResolvedRef, Error> {
        let parsed = Range::parse(range)?;
        let key = scoped_name.replace('/', ":");
        let name = self
            .document(scoped_name, "name lookup", &["scopedNames", &key])
            .await?;
        let name = Fields::document(&name, scoped_name, "name lookup")?;
        let id = name.string("publishedAssetId")?;
        let asset = self
            .document(scoped_name, "asset document", &["publishedAssets", id])
            .await?;
        let asset = Fields::document(&asset, scoped_name, "asset document")?;
        let actual = asset.string("assetType")?;
        if actual != expected_type {
            return Err(Error::WrongAssetType {
                scoped_name: scoped_name.into(),
                expected: expected_type.into(),
                actual: actual.into(),
            });
        }
        let current_name = asset.string("scopedName")?.to_owned();
        let private = asset.optional_string("publicRole")?.is_none();
        let mut releases = Vec::new();
        let mut page: Option<String> = None;
        let mut seen = HashSet::new();
        loop {
            let mut url = self.firestore(&["publishedAssets", id, "releases"])?;
            url.query_pairs_mut().append_pair("pageSize", "100");
            if let Some(page) = &page {
                url.query_pairs_mut().append_pair("pageToken", page);
            }
            let value = self.json(scoped_name, "release listing", url).await?;
            let object = value
                .as_object()
                .ok_or_else(|| unexpected(scoped_name, "release listing", "expected an object"))?;
            if let Some(documents) = object.get("documents") {
                let documents = documents.as_array().ok_or_else(|| {
                    unexpected(scoped_name, "release listing", "documents must be an array")
                })?;
                releases.extend(documents.iter().cloned());
            }
            match object.get("nextPageToken") {
                None => break,
                Some(Value::String(token)) if !token.is_empty() && seen.insert(token.clone()) => {
                    page = Some(token.clone())
                }
                _ => {
                    return Err(unexpected(
                        scoped_name,
                        "release listing",
                        "invalid or repeated nextPageToken",
                    ))
                }
            }
        }
        let (release, version) = select(&releases, scoped_name, range, &parsed)?;
        let fields = Fields::document(release, scoped_name, "release selection")?;
        let primary = fields.map("artifacts")?.unwrap().map("primary")?.unwrap();
        let hash = primary.string("contentHash")?;
        if !crate::valid_hash(hash) {
            return Err(primary.error("contentHash must be 64 lowercase hex digits"));
        }
        let storage = primary
            .map("storage")?
            .ok_or_else(|| primary.error("missing storage"))?;
        let storage_path = storage.string("fileBucket")?.to_owned();
        if storage_path.is_empty() {
            return Err(storage.error("fileBucket is empty"));
        }
        let release_id = release
            .get("name")
            .and_then(Value::as_str)
            .and_then(|s| s.rsplit('/').next())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fields.error("missing document name"))?
            .to_owned();
        Ok(ResolvedRef {
            scoped_name: current_name,
            published_asset_id: id.into(),
            version: version.to_string(),
            release_id,
            artifact_key: "primary".into(),
            content_hash: hash.into(),
            storage_path,
            family: fields.optional_string("family")?.map(str::to_owned),
            private,
            resolved_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| Error::Config(e.to_string()))?
                .as_millis() as u64,
        })
    }

    /// Resolve a model online and save its pin. Only network failures permit a last-pin fallback.
    /// The fallback still has to satisfy the requested range; bytes are read separately from the cache.
    pub async fn resolve_with_fallback(
        &self,
        scoped_name: &str,
        range: &str,
        cache: &Cache,
    ) -> Result<ResolvedRef, Error> {
        let parsed = Range::parse(range)?;
        match self.resolve(scoped_name, range).await {
            Ok(pin) => {
                cache.save_pin(scoped_name, &pin)?;
                Ok(pin)
            }
            Err(e @ Error::Network { .. }) => match cache.last_pin(scoped_name)? {
                Some(pin) if Version::parse(&pin.version).is_some_and(|v| parsed.contains(v)) => {
                    Ok(pin)
                }
                _ => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    /// Always contact Storage and verify the returned bytes.
    pub async fn fetch(&self, resolved: &ResolvedRef) -> Result<FetchedAsset, Error> {
        if resolved.storage_path.is_empty() {
            return Err(Error::Cache(
                "offline pin has no storage path; read its bytes from the cache".into(),
            ));
        }
        let mut url = endpoint(
            &self.config.storage_url,
            &[
                "v0",
                "b",
                &self.config.storage_bucket,
                "o",
                &resolved.storage_path,
            ],
        )?;
        url.query_pairs_mut().append_pair("alt", "media");
        let bytes = self
            .read(&resolved.scoped_name, "Storage bytes", url, true)
            .await?;
        verify(&resolved.scoped_name, &resolved.content_hash, &bytes)?;
        Ok(FetchedAsset {
            bytes,
            content_hash: resolved.content_hash.clone(),
            resolved: Some(resolved.clone()),
        })
    }

    fn firestore(&self, path: &[&str]) -> Result<Url, Error> {
        let mut parts = vec![
            "v1",
            "projects",
            &self.config.project_id,
            "databases",
            "(default)",
            "documents",
        ];
        parts.extend_from_slice(path);
        endpoint(&self.config.firestore_url, &parts)
    }

    async fn document(&self, name: &str, step: &str, path: &[&str]) -> Result<Value, Error> {
        self.json(name, step, self.firestore(path)?).await
    }

    async fn json(&self, name: &str, step: &str, url: Url) -> Result<Value, Error> {
        let bytes = self.read(name, step, url, false).await?;
        serde_json::from_slice(&bytes).map_err(|e| unexpected(name, step, &e.to_string()))
    }

    async fn read(
        &self,
        name: &str,
        step: &str,
        url: Url,
        storage: bool,
    ) -> Result<Vec<u8>, Error> {
        let token = self.tokens.token().await?;
        let mut request = self.client.get(url);
        if let Some(token) = &token {
            request = request.header(
                reqwest::header::AUTHORIZATION,
                format!("{} {token}", if storage { "Firebase" } else { "Bearer" }),
            );
        }
        let network = |e: reqwest::Error| {
            if e.is_builder() {
                unexpected(name, step, "invalid request or authorization header")
            } else {
                Error::Network {
                    scoped_name: name.into(),
                    step: step.into(),
                    detail: e.without_url().to_string(),
                }
            }
        };
        let response = request.send().await.map_err(network)?;
        let status = response.status();
        if status.as_u16() == 403 && token.is_some() {
            return Err(Error::Refused {
                scoped_name: name.into(),
                step: step.into(),
            });
        }
        if status.as_u16() == 404 || (status.as_u16() == 403 && token.is_none()) {
            return Err(Error::NotFound {
                scoped_name: name.into(),
                step: step.into(),
                cause: if token.is_none() {
                    "not found, or not public"
                } else {
                    "not found"
                }
                .into(),
            });
        }
        if status.is_server_error() {
            return Err(Error::Network {
                scoped_name: name.into(),
                step: step.into(),
                detail: format!("HTTP {status}"),
            });
        }
        if !status.is_success() {
            return Err(unexpected(name, step, &format!("HTTP {status}")));
        }
        Ok(response.bytes().await.map_err(network)?.to_vec())
    }
}

fn endpoint(base: &str, parts: &[&str]) -> Result<Url, Error> {
    let mut url = Url::parse(base).map_err(|e| Error::Config(format!("invalid endpoint: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::Config("endpoint must be an HTTP(S) URL".into()));
    }
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|_| Error::Config("endpoint cannot hold path segments".into()))?
        .pop_if_empty()
        .extend(parts);
    Ok(url)
}

fn unexpected(name: &str, step: &str, detail: &str) -> Error {
    Error::UnexpectedResponse {
        scoped_name: name.into(),
        step: step.into(),
        detail: detail.into(),
    }
}

struct Fields<'a> {
    fields: &'a Map<String, Value>,
    name: &'a str,
    step: &'a str,
}

impl<'a> Fields<'a> {
    fn document(value: &'a Value, name: &'a str, step: &'a str) -> Result<Self, Error> {
        let fields = value
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| unexpected(name, step, "missing document fields"))?;
        Ok(Self { fields, name, step })
    }
    fn error(&self, detail: &str) -> Error {
        unexpected(self.name, self.step, detail)
    }
    fn optional_string(&self, key: &str) -> Result<Option<&'a str>, Error> {
        match self.fields.get(key) {
            None => Ok(None),
            Some(value) if value.get("nullValue") == Some(&Value::Null) => Ok(None),
            Some(value) => value
                .get("stringValue")
                .and_then(Value::as_str)
                .map(Some)
                .ok_or_else(|| self.error(&format!("{key} must be a string"))),
        }
    }
    fn string(&self, key: &str) -> Result<&'a str, Error> {
        self.optional_string(key)?
            .ok_or_else(|| self.error(&format!("missing {key}")))
    }
    fn boolean(&self, key: &str) -> Result<bool, Error> {
        match self.fields.get(key) {
            None => Ok(false),
            Some(value) => value
                .get("booleanValue")
                .and_then(Value::as_bool)
                .ok_or_else(|| self.error(&format!("{key} must be a boolean"))),
        }
    }
    fn map(&self, key: &str) -> Result<Option<Self>, Error> {
        let Some(value) = self.fields.get(key) else {
            return Ok(None);
        };
        let map = value
            .get("mapValue")
            .and_then(Value::as_object)
            .ok_or_else(|| self.error(&format!("{key} must be a map")))?;
        // Firestore omits fields for an empty map.
        let fields = match map.get("fields") {
            Some(value) => value
                .as_object()
                .ok_or_else(|| self.error("map fields must be an object"))?,
            None if map.is_empty() => map,
            None => return Err(self.error("missing map fields")),
        };
        Ok(Some(Self {
            fields,
            name: self.name,
            step: self.step,
        }))
    }
}

fn select<'a>(
    releases: &'a [Value],
    name: &str,
    raw_range: &str,
    range: &Range,
) -> Result<(&'a Value, Version), Error> {
    let mut candidates = Vec::new();
    for release in releases {
        let fields = Fields::document(release, name, "release selection")?;
        if fields.boolean("deprecated")? {
            continue;
        }
        let Some(version) = Version::parse(fields.string("version")?) else {
            continue;
        };
        if range.contains(version) {
            candidates.push((release, version));
        }
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));
    for (release, version) in candidates {
        let fields = Fields::document(release, name, "release selection")?;
        if let Some(artifacts) = fields.map("artifacts")? {
            if let Some(primary) = artifacts.map("primary")? {
                if primary.optional_string("contentHash")?.is_some() {
                    return Ok((release, version));
                }
            }
        }
    }
    Err(Error::NoRelease {
        scoped_name: name.into(),
        range: raw_range.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn selection_is_stable_and_skips_unusable_releases() {
        let release = |id, version, deprecated, hash| {
            json!({"name": id, "fields": {
                "version": {"stringValue": version}, "deprecated": {"booleanValue": deprecated},
                "artifacts": {"mapValue": {"fields": {"primary": {"mapValue": {"fields": hash}}}}}
            }})
        };
        let hash = json!({"contentHash": {"stringValue": "hash"}});
        let releases = vec![
            release("deprecated", "9.0.0", true, hash.clone()),
            release("missing", "8.0.0", false, json!({})),
            release("old", "0.9.9", false, hash.clone()),
            release("bad", "garbage", false, hash.clone()),
            release("earlier", "2.9.0", false, hash.clone()),
            release("first", "v2.10.0", false, hash.clone()),
            release("second", "2.10.0", false, hash),
        ];
        let (chosen, version) =
            select(&releases, "name", "*", &Range::parse("*").unwrap()).unwrap();
        assert_eq!(chosen["name"], "first");
        assert_eq!(version.to_string(), "2.10.0");
    }
}
