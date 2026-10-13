//! A fake Studio for the tests: the name entries, published-asset documents,
//! release listings and Storage objects the resolver reads, served by
//! `wiremock`, plus the pins and token source for these paths.

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

pub async fn bounded<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(std::time::Duration::from_secs(5), future)
        .await
        .expect("fake Studio deadline")
}
