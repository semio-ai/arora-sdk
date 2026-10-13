use crate::version::{Range, Version};
use crate::{
    env_value, verify, AssetType, Cache, Error, FetchedAsset, Resolution, ResolutionSource,
    ResolvedRef,
};
use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
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
        let mut required =
            |name| env_value(lookup(name)).ok_or_else(|| Error::Config(format!("missing {name}")));
        let project_id = required("FIREBASE_PROJECT_ID")?;
        let storage_bucket = required("FIREBASE_STORAGE_BUCKET")?;
        let firestore_url = env_value(lookup("FIREBASE_FIRESTORE_EMULATOR_HOST"))
            .map(|host| format!("http://{host}"))
            .unwrap_or_else(|| "https://firestore.googleapis.com".into());
        let storage_url = env_value(lookup("FIREBASE_STORAGE_EMULATOR_HOST"))
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

const FIRESTORE_LIMIT: usize = 16 * 1024 * 1024;
const STORAGE_LIMIT: usize = 1024 * 1024 * 1024;

pub struct Resolver<T = NoToken> {
    config: StudioConfig,
    tokens: T,
    client: Client,
    storage_limit: usize,
}

pub struct ResolverBuilder<T = NoToken> {
    config: StudioConfig,
    tokens: T,
    client: reqwest::ClientBuilder,
    storage_limit: usize,
}

impl<T: TokenSource> ResolverBuilder<T> {
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.client = self.client.connect_timeout(timeout);
        self
    }
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.client = self.client.read_timeout(timeout);
        self
    }
    pub fn storage_limit(mut self, bytes: usize) -> Self {
        self.storage_limit = bytes;
        self
    }

    pub fn build(self) -> Result<Resolver<T>, Error> {
        let client = self
            .client
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Resolver {
            config: self.config,
            tokens: self.tokens,
            client,
            storage_limit: self.storage_limit,
        })
    }
}

impl<T: TokenSource> Resolver<T> {
    pub fn new(config: StudioConfig, tokens: T) -> Result<Self, Error> {
        Self::builder(config, tokens).build()
    }

    pub fn builder(config: StudioConfig, tokens: T) -> ResolverBuilder<T> {
        ResolverBuilder {
            config,
            tokens,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .read_timeout(Duration::from_secs(60)),
            storage_limit: STORAGE_LIMIT,
        }
    }

    pub async fn resolve(
        &self,
        scoped_name: &str,
        range: &str,
        expected_type: AssetType,
    ) -> Result<ResolvedRef, Error> {
        validate_name(scoped_name)?;
        let parsed = Range::parse(scoped_name, range)?;
        let key = scoped_name.replace('/', ":");
        let name = self
            .document(scoped_name, "name lookup", &["scopedNames", &key])
            .await?;
        let name = Fields::document(&name, scoped_name, "name lookup")?;
        let id = name.string("publishedAssetId")?;
        validate_id(id, scoped_name, "name lookup")?;
        let asset = self
            .document(scoped_name, "asset document", &["publishedAssets", id])
            .await?;
        let asset = Fields::document(&asset, scoped_name, "asset document")?;
        let actual = asset.string("assetType")?;
        if actual != expected_type.as_str() {
            return Err(Error::WrongAssetType {
                scoped_name: scoped_name.into(),
                expected: expected_type.as_str().into(),
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
        let release_id = release["name"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_owned();
        let step = format!("release {release_id}");
        let fields = Fields::document(release, scoped_name, &step)?;
        let primary = fields.map("artifacts")?.unwrap().map("primary")?.unwrap();
        let hash = primary.string("contentHash")?;
        if !crate::valid_hash(hash) {
            return Err(primary.error("contentHash must be 64 lowercase hex digits"));
        }
        let storage = primary
            .map("storage")?
            .ok_or_else(|| primary.error("missing storage"))?;
        let storage_path = storage.string("fileBucket")?.to_owned();
        validate_storage_path(&storage_path, scoped_name, &step)?;
        let family = fields.fields.get("family").and_then(|value| {
            let family = value.get("stringValue").and_then(Value::as_str);
            if family.is_none() {
                log::warn!(target: "arora-assets", "{scoped_name}: {step}: family must be a string; treating family as absent");
            }
            family.map(str::to_owned)
        });
        Ok(ResolvedRef {
            scoped_name: current_name,
            published_asset_id: id.into(),
            version: version.to_string(),
            release_id,
            artifact_key: "primary".into(),
            content_hash: hash.into(),
            storage_path: Some(storage_path),
            asset_type: expected_type,
            family,
            private,
            resolved_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| Error::Config(e.to_string()))?
                .as_millis() as u64,
        })
    }

    /// Always resolve online first. Only transient network failures permit a compatible saved pin.
    pub async fn resolve_with_fallback(
        &self,
        scoped_name: &str,
        range: &str,
        expected_type: AssetType,
        cache: &Cache,
    ) -> Result<Resolution, Error> {
        validate_name(scoped_name)?;
        let parsed = Range::parse(scoped_name, range)?;
        match self.resolve(scoped_name, range, expected_type).await {
            Ok(pin) => {
                let name = scoped_name.to_owned();
                let saved = pin.clone();
                if let Err(error) = cache
                    .blocking(move |cache| cache.save_pin(&name, &saved))
                    .await
                {
                    log::warn!(target: "arora-assets", "{scoped_name}: could not save resolved pin: {error}");
                }
                Ok(Resolution {
                    resolved: pin,
                    source: ResolutionSource::Server,
                })
            }
            Err(e @ Error::Network { .. }) => {
                let name = scoped_name.to_owned();
                let pin = cache
                    .blocking(move |cache| cache.last_pin(&name))
                    .await
                    .ok()
                    .flatten();
                match pin {
                    Some(pin)
                        if pin.asset_type == expected_type
                            && Version::parse(&pin.version).is_some_and(|v| parsed.contains(v)) =>
                    {
                        Ok(Resolution {
                            resolved: pin,
                            source: ResolutionSource::Cache,
                        })
                    }
                    _ => Err(e),
                }
            }
            Err(
                e @ (Error::NotFound {
                    http_status: 404, ..
                }
                | Error::Refused { .. }),
            ) => {
                let name = scoped_name.to_owned();
                if let Err(error) = cache.blocking(move |cache| cache.remove_pin(&name)).await {
                    log::warn!(target: "arora-assets", "{scoped_name}: could not clear resolved pin: {error}");
                }
                Err(e)
            }
            Err(e) => Err(e),
        }
    }

    /// Always contact Storage and verify the returned bytes.
    pub async fn fetch(&self, resolved: &ResolvedRef) -> Result<FetchedAsset, Error> {
        let storage_path = resolved.storage_path.as_deref().ok_or_else(|| {
            Error::Cache(format!(
                "{}: offline pin has no storage path; read the bytes from the cache by hash {}",
                resolved.scoped_name, resolved.content_hash
            ))
        })?;
        validate_storage_path(
            storage_path,
            &resolved.scoped_name,
            &format!("release {}", resolved.release_id),
        )?;
        let mut url = endpoint(
            &self.config.storage_url,
            &["v0", "b", &self.config.storage_bucket, "o", storage_path],
        )?;
        url.query_pairs_mut().append_pair("alt", "media");
        let bytes = self
            .read(&resolved.scoped_name, "Storage bytes", url, true)
            .await?;
        let resolved = resolved.clone();
        crate::blocking(move || {
            verify(&resolved.scoped_name, &resolved.content_hash, &bytes)?;
            Ok(FetchedAsset {
                bytes,
                content_hash: resolved.content_hash.clone(),
                resolved: Some(resolved),
            })
        })
        .await
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
                    detail: error_chain(&e.without_url()),
                }
            }
        };
        let response = request.send().await.map_err(network)?;
        let status = response.status();
        if status.as_u16() == 401 {
            return Err(Error::Unauthorized {
                scoped_name: name.into(),
                step: step.into(),
            });
        }
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
                http_status: status.as_u16(),
                cause: if status.as_u16() == 403 {
                    "not found, or not public"
                } else {
                    "not found"
                }
                .into(),
            });
        }
        if status.is_server_error() || matches!(status.as_u16(), 408 | 429) {
            return Err(Error::Network {
                scoped_name: name.into(),
                step: step.into(),
                detail: format!("HTTP {status}"),
            });
        }
        if !status.is_success() {
            return Err(unexpected(name, step, &format!("HTTP {status}")));
        }
        let limit = if storage {
            self.storage_limit
        } else {
            FIRESTORE_LIMIT
        };
        read_body(
            response.bytes_stream().map(|chunk| chunk.map_err(network)),
            limit,
            name,
            step,
        )
        .await
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
    let mut newest: Option<(&Value, Version)> = None;
    for release in releases {
        let document_name = release
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| unexpected(name, "release listing", "missing document name"))?;
        validate_id(
            document_name.rsplit('/').next().unwrap_or_default(),
            name,
            "release listing",
        )?;
        let Ok(fields) = Fields::document(release, name, "release selection") else {
            continue;
        };
        if fields
            .fields
            .get("deprecated")
            .and_then(|v| v.get("booleanValue"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            continue;
        }
        let Some(version) = fields.string("version").ok().and_then(Version::parse) else {
            continue;
        };
        if !range.contains(version) {
            continue;
        }
        let usable = (|| {
            let primary = fields.map("artifacts").ok()??.map("primary").ok()??;
            let hash = primary.string("contentHash").ok()?;
            if hash.is_empty() {
                return None;
            }
            Some(())
        })();
        if usable.is_some() && newest.is_none_or(|(_, kept)| version > kept) {
            newest = Some((release, version));
        }
    }
    newest.ok_or_else(|| Error::NoRelease {
        scoped_name: name.into(),
        range: raw_range.into(),
    })
}

pub(crate) fn validate_name(name: &str) -> Result<(), Error> {
    let valid = name
        .strip_prefix('@')
        .and_then(|s| s.split_once('/'))
        .is_some_and(|(scope, asset)| {
            !scope.is_empty() && !asset.is_empty() && !asset.contains('/')
        })
        && !name.contains(':');
    if !valid {
        return Err(Error::InvalidName {
            scoped_name: name.into(),
        });
    }
    Ok(())
}

fn validate_storage_path(path: &str, name: &str, step: &str) -> Result<(), Error> {
    if path.is_empty() || matches!(path, "." | "..") {
        return Err(unexpected(
            name,
            step,
            &format!("invalid storage path {path:?}"),
        ));
    }
    Ok(())
}

fn validate_id(id: &str, name: &str, step: &str) -> Result<(), Error> {
    if id.is_empty() || matches!(id, "." | "..") || id.contains('/') {
        return Err(unexpected(
            name,
            step,
            &format!("invalid document id {id:?}"),
        ));
    }
    Ok(())
}

fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    detail
}

async fn read_body<B: AsRef<[u8]>>(
    stream: impl Stream<Item = Result<B, Error>>,
    limit: usize,
    name: &str,
    step: &str,
) -> Result<Vec<u8>, Error> {
    futures_util::pin_mut!(stream);
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        append_chunk(&mut bytes, chunk?.as_ref(), limit, name, step)?;
    }
    Ok(bytes)
}

fn append_chunk(
    bytes: &mut Vec<u8>,
    chunk: &[u8],
    limit: usize,
    name: &str,
    step: &str,
) -> Result<(), Error> {
    if chunk.len() > limit.saturating_sub(bytes.len()) {
        return Err(Error::ResponseTooLarge {
            scoped_name: name.into(),
            step: step.into(),
            limit,
        });
    }
    bytes.extend_from_slice(chunk);
    Ok(())
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
        let hash = json!({"contentHash": {"stringValue": crate::content_hash(b"test")}, "storage": {"mapValue": {"fields": {"fileBucket": {"stringValue": "object"}}}}});
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
            select(&releases, "name", "*", &Range::parse("name", "*").unwrap()).unwrap();
        assert_eq!(chosen["name"], "first");
        assert_eq!(version.to_string(), "2.10.0");
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[test]
    fn builder_timeouts_have_no_total_deadline() {
        let config = StudioConfig {
            project_id: String::new(),
            storage_bucket: String::new(),
            firestore_url: String::new(),
            storage_url: String::new(),
        };
        let builder = Resolver::builder(config.clone(), NoToken);
        let debug = format!("{:?}", builder.client);
        assert!(debug.contains("connect_timeout: 30s"), "{debug}");

        assert!(!debug.contains(", timeout:"), "{debug}");
        assert_eq!(builder.storage_limit, 1024 * 1024 * 1024);
        let built = builder.build().unwrap();
        assert!(format!("{:?}", built.client).contains("read_timeout: 60s"));
        let builder = Resolver::builder(config, NoToken)
            .connect_timeout(Duration::from_millis(12))
            .read_timeout(Duration::from_millis(34));
        let debug = format!("{:?}", builder.client);
        assert!(debug.contains("connect_timeout: 12ms"), "{debug}");
        let built = builder.build().unwrap();
        assert!(format!("{:?}", built.client).contains("read_timeout: 34ms"));
        assert!(!debug.contains(", timeout:"), "{debug}");
    }

    #[test]
    fn cap_checks_each_chunk_before_extending_the_buffer() {
        let mut bytes = vec![];
        append_chunk(&mut bytes, &[1; 3], 5, "@test/model", "bytes").unwrap();
        let error = append_chunk(&mut bytes, &[2; 3], 5, "@test/model", "bytes").unwrap_err();
        assert!(matches!(error, Error::ResponseTooLarge { limit: 5, .. }));
        assert_eq!(bytes, [1; 3]);
        append_chunk(&mut bytes, &[3; 2], 5, "@test/model", "bytes").unwrap();
        assert_eq!(bytes.len(), 5);
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    #[tokio::test]
    async fn oversized_stream_stops_before_polling_the_rest() {
        let mut polled = 0;
        let stream =
            futures_util::stream::iter((0..10).map(|_| Ok(vec![0; 3]))).inspect(|_| polled += 1);
        let error = read_body(stream, 5, "@test/model", "bytes")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::ResponseTooLarge { limit: 5, .. }));
        assert_eq!(polled, 2);
    }
}
