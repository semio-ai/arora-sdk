//! A fake Studio for the tests: the name entries, published-asset documents,
//! release listings and Storage objects the resolver reads, served by
//! `wiremock`, plus the pins, token source and warning capture the tests share.

#![allow(dead_code)]
use arora_index::{content_hash, AssetType, Error, ResolvedRef, StudioConfig, TokenSource};
use async_trait::async_trait;
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const NAME: &str = "@test/old";
pub const CURRENT: &str = "@test/current";
pub const BYTES: &[u8] = b"fixture model bytes";
pub const OBJECT: &str = "publishedAssets/asset/releases/release/artifacts/primary";
pub const ROOT: &str = "/v1/projects/demo-test/databases/(default)/documents";

pub fn config(server: &MockServer) -> StudioConfig {
    StudioConfig {
        project_id: "demo-test".into(),
        storage_bucket: "demo-test.bucket".into(),
        firestore_url: server.uri(),
        storage_url: server.uri(),
    }
}

pub fn asset(private: bool, asset_type: &str) -> Value {
    json!({"name": "publishedAssets/asset", "fields": {
        "scopedName": {"stringValue": CURRENT}, "assetType": {"stringValue": asset_type},
        "publicRole": if private { json!({"nullValue": null}) } else { json!({"stringValue": "viewer"}) },
        "latestVersion": {"stringValue": "99.0.0"}, "deprecated": {"booleanValue": false}
    }})
}

pub fn release(id: &str, version: &str) -> Value {
    json!({"name": format!("publishedAssets/asset/releases/{id}"), "fields": {
        "version": {"stringValue": version}, "family": {"stringValue": "test"},
        "artifacts": {"mapValue": {"fields": {"primary": {"mapValue": {"fields": {
            "contentHash": {"stringValue": content_hash(BYTES)},
            "storage": {"mapValue": {"fields": {"fileBucket": {"stringValue": OBJECT}}}},
            "role": {"stringValue": "model"}, "minRole": {"stringValue": "viewer"}
        }}}}}}
    }})
}

pub async fn name(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/scopedNames/@test:old")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"fields": {
            "publishedAssetId": {"stringValue": "asset"}, "kind": {"stringValue": "alias"}
        }})))
        .mount(server)
        .await;
}

pub async fn documents(server: &MockServer, private: bool, asset_type: &str) {
    name(server).await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/publishedAssets/asset")))
        .respond_with(ResponseTemplate::new(200).set_body_json(asset(private, asset_type)))
        .mount(server)
        .await;
}

pub async fn chain(server: &MockServer, private: bool) {
    documents(server, private, "model").await;
    Mock::given(method("GET"))
        .and(path(format!("{ROOT}/publishedAssets/asset/releases")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"documents": [release("release", " v1.2.3 ")]})),
        )
        .mount(server)
        .await;
}

pub async fn storage(server: &MockServer, bytes: &[u8], status: u16) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/v0/b/demo-test.bucket/o/{}",
            OBJECT.replace('/', "%2F")
        )))
        .and(query_param("alt", "media"))
        .respond_with(ResponseTemplate::new(status).set_body_bytes(bytes))
        .mount(server)
        .await;
}

pub struct Token;
#[async_trait]
impl TokenSource for Token {
    async fn token(&self) -> Result<Option<String>, Error> {
        Ok(Some("test-token".into()))
    }
}
pub fn pin() -> ResolvedRef {
    ResolvedRef {
        scoped_name: CURRENT.into(),
        published_asset_id: "asset".into(),
        version: "1.2.3".into(),
        release_id: "release".into(),
        artifact_key: "primary".into(),
        content_hash: content_hash(BYTES),
        storage_path: Some(OBJECT.into()),
        family: None,
        private: true,
        resolved_at: 123,
        asset_type: AssetType::Model,
    }
}
pub async fn listing(server: &MockServer, documents: Vec<Value>) {
    self::documents(server, false, "model").await;
    Mock::given(path(format!("{ROOT}/publishedAssets/asset/releases")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"documents": documents})))
        .mount(server)
        .await;
}

pub struct Warnings(pub std::sync::Mutex<Vec<String>>);
pub static HASH_THREADS: std::sync::Mutex<Vec<(String, std::thread::ThreadId)>> =
    std::sync::Mutex::new(Vec::new());
pub static WARNINGS: Warnings = Warnings(std::sync::Mutex::new(Vec::new()));
impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "arora-assets"
    }
    fn log(&self, record: &log::Record<'_>) {
        if record.level() <= log::Level::Warn {
            self.0.lock().unwrap().push(record.args().to_string());
        }
        if record.level() == log::Level::Trace && record.target() == "arora-assets" {
            HASH_THREADS
                .lock()
                .unwrap()
                .push((record.args().to_string(), std::thread::current().id()));
        }
    }
    fn flush(&self) {}
}
pub fn capture_warnings() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        log::set_logger(&WARNINGS).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
}

pub fn model_pin(private: bool) -> ResolvedRef {
    ResolvedRef {
        scoped_name: CURRENT.into(),
        published_asset_id: "asset".into(),
        version: "1.2.3".into(),
        release_id: "release".into(),
        artifact_key: "primary".into(),
        content_hash: content_hash(BYTES),
        storage_path: Some(OBJECT.into()),
        asset_type: AssetType::Model,
        family: Some("test".into()),
        private,
        resolved_at: 123,
    }
}

pub fn assert_hash_off_executor(size: usize, since: usize) {
    let messages = HASH_THREADS.lock().unwrap();
    let threads: Vec<_> = messages[since..]
        .iter()
        .filter(|(message, _)| message == &format!("computing SHA-256 for {size} bytes"))
        .map(|(_, thread)| *thread)
        .collect();
    assert!(!threads.is_empty(), "hashing must be observed");
    assert!(
        threads
            .iter()
            .all(|thread| *thread != std::thread::current().id()),
        "hashing must run off the async executor"
    );
}
