#![allow(dead_code)]
use arora_index::{content_hash, StudioConfig};
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
