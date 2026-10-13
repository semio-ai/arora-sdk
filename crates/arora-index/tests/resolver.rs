mod support;
use arora_index::{content_hash, fetch_local, Error, NoToken, Resolver, StudioConfig, TokenSource};
use async_trait::async_trait;
use serde_json::json;
use support::*;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Token;
#[async_trait]
impl TokenSource for Token {
    async fn token(&self) -> Result<Option<String>, Error> {
        Ok(Some("test-token".into()))
    }
}

#[tokio::test]
async fn public_alias_resolves_and_fetches_without_authorization() {
    let server = MockServer::start().await;
    chain(&server, false).await;
    storage(&server, BYTES, 200).await;
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    let pin = resolver.resolve(NAME, "^1.0.0").await.unwrap();
    assert_eq!(pin.scoped_name, CURRENT);
    assert_eq!(pin.published_asset_id, "asset");
    assert_eq!(pin.version, "1.2.3");
    assert_eq!(pin.artifact_key, "primary");
    assert_eq!(pin.family.as_deref(), Some("test"));
    assert!(!pin.private);
    assert!(pin.resolved_at > 0);
    let fetched = resolver.fetch(&pin).await.unwrap();
    assert_eq!(fetched.bytes, BYTES);
    assert_eq!(fetched.content_hash, content_hash(BYTES));
    assert_eq!(fetched.resolved, Some(pin));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests
        .iter()
        .all(|r| !r.headers.contains_key("authorization")));
    assert!(requests
        .last()
        .unwrap()
        .url
        .as_str()
        .contains("publishedAssets%2Fasset%2Freleases%2Frelease%2Fartifacts%2Fprimary"));
}

#[tokio::test]
async fn authenticated_private_chain_uses_each_services_header() {
    let server = MockServer::start().await;
    chain(&server, true).await;
    storage(&server, BYTES, 200).await;
    let resolver = Resolver::new(config(&server), Token).unwrap();
    let pin = resolver.resolve(NAME, "*").await.unwrap();
    assert!(pin.private);
    resolver.fetch(&pin).await.unwrap();
    for request in server.received_requests().await.unwrap() {
        assert_eq!(
            request.headers["authorization"],
            if request.url.path().starts_with("/v0/") {
                "Firebase test-token"
            } else {
                "Bearer test-token"
            }
        );
    }
}

#[tokio::test]
async fn missing_name_and_refused_errors_preserve_the_cause() {
    for (status, authenticated) in [(403, false), (404, true), (403, true)] {
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(status).set_body_json(json!({"error": {"code": status, "message": "denied", "status": "PERMISSION_DENIED"}}))).mount(&server).await;
        let error = if authenticated {
            Resolver::new(config(&server), Token)
                .unwrap()
                .resolve(NAME, "*")
                .await
                .unwrap_err()
        } else {
            Resolver::new(config(&server), NoToken)
                .unwrap()
                .resolve(NAME, "*")
                .await
                .unwrap_err()
        };
        assert!(error.to_string().contains(NAME));
        assert!(error.to_string().contains("name lookup"));
        if authenticated && status == 403 {
            assert!(matches!(error, Error::Refused { .. }));
        } else {
            assert!(matches!(error, Error::NotFound { .. }));
            assert_eq!(error.to_string().contains("or not public"), !authenticated);
        }
    }
}

#[tokio::test]
async fn anonymous_private_document_and_bytes_are_not_found_or_not_public() {
    let server = MockServer::start().await;
    name(&server).await;
    Mock::given(path(format!("{ROOT}/publishedAssets/asset")))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    let error = resolver.resolve(NAME, "*").await.unwrap_err();
    assert!(error
        .to_string()
        .contains("asset document: not found, or not public"));
    server.reset().await;
    chain(&server, true).await;
    storage(&server, b"denied", 403).await;
    let pin = resolver.resolve(NAME, "*").await.unwrap();
    let error = resolver.fetch(&pin).await.unwrap_err();
    assert!(error
        .to_string()
        .contains("Storage bytes: not found, or not public"));
}

#[tokio::test]
async fn range_type_and_hash_errors() {
    let server = MockServer::start().await;
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    assert!(matches!(
        resolver.resolve(NAME, ">=1").await,
        Err(Error::InvalidRange { .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    chain(&server, false).await;
    assert!(matches!(
        resolver.resolve(NAME, "2.0.0").await,
        Err(Error::NoRelease { .. })
    ));
    storage(&server, b"wrong", 200).await;
    let pin = resolver.resolve(NAME, "*").await.unwrap();
    assert!(
        matches!(resolver.fetch(&pin).await, Err(Error::HashMismatch { expected, actual, .. }) if expected == content_hash(BYTES) && actual == content_hash(b"wrong"))
    );
    server.reset().await;
    documents(&server, false, "face").await;
    assert!(
        matches!(resolver.resolve(NAME, "*").await, Err(Error::WrongAssetType { expected, actual, .. }) if expected == "model" && actual == "face")
    );
}

#[tokio::test]
async fn three_pages_include_the_newest_release() {
    let server = MockServer::start().await;
    documents(&server, false, "model").await;
    let listing = format!("{ROOT}/publishedAssets/asset/releases");
    Mock::given(path(&listing))
        .and(|r: &wiremock::Request| !r.url.query_pairs().any(|(k, _)| k == "pageToken"))
        .and(query_param("pageSize", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"documents": [release("a", "2.9.0")], "nextPageToken": "page + two"}),
        ))
        .mount(&server)
        .await;
    Mock::given(path(&listing))
        .and(query_param("pageToken", "page + two"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"nextPageToken": "three"})))
        .mount(&server)
        .await;
    Mock::given(path(&listing))
        .and(query_param("pageToken", "three"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"documents": [release("c", "2.10.0")]})),
        )
        .mount(&server)
        .await;
    let pin = Resolver::new(config(&server), NoToken)
        .unwrap()
        .resolve(NAME, "*")
        .await
        .unwrap();
    assert_eq!(pin.version, "2.10.0");
    assert_eq!(pin.release_id, "c");
    assert_eq!(server.received_requests().await.unwrap().len(), 5);
}

#[tokio::test]
async fn empty_and_malformed_responses_are_distinct() {
    for (body, empty) in [
        (json!({}), true),
        (json!({"documents": null}), false),
        (json!({"documents": [{}]}), false),
        (json!({"nextPageToken": 1}), false),
    ] {
        let server = MockServer::start().await;
        documents(&server, false, "model").await;
        Mock::given(path(format!("{ROOT}/publishedAssets/asset/releases")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let err = Resolver::new(config(&server), NoToken)
            .unwrap()
            .resolve(NAME, "*")
            .await
            .unwrap_err();
        if empty {
            assert!(matches!(err, Error::NoRelease { .. }));
        } else {
            assert!(matches!(err, Error::UnexpectedResponse { .. }));
        }
    }
}

#[test]
fn environment_configuration_uses_injected_lookup() {
    let config = StudioConfig::from_lookup(|name| match name {
        "FIREBASE_PROJECT_ID" => Some("project".into()),
        "FIREBASE_STORAGE_BUCKET" => Some("bucket".into()),
        _ => None,
    })
    .unwrap();
    assert_eq!(config.firestore_url, "https://firestore.googleapis.com");
    assert_eq!(config.storage_url, "https://firebasestorage.googleapis.com");
    let config = StudioConfig::from_lookup(|name| {
        Some(
            match name {
                "FIREBASE_FIRESTORE_EMULATOR_HOST" => "localhost:8080",
                "FIREBASE_STORAGE_EMULATOR_HOST" => "127.0.0.1:9199",
                _ => "fixture",
            }
            .into(),
        )
    })
    .unwrap();
    assert_eq!(config.firestore_url, "http://localhost:8080");
    assert_eq!(config.storage_url, "http://127.0.0.1:9199");
    assert!(StudioConfig::from_lookup(|_| None)
        .unwrap_err()
        .to_string()
        .contains("FIREBASE_PROJECT_ID"));
    assert!(StudioConfig::from_lookup(
        |name| (name == "FIREBASE_PROJECT_ID").then(|| "fixture".into())
    )
    .unwrap_err()
    .to_string()
    .contains("FIREBASE_STORAGE_BUCKET"));
}

#[tokio::test]
async fn local_file_returns_bytes_hash_and_no_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.glb");
    std::fs::write(&path, BYTES).unwrap();
    let fetched = fetch_local(&path).await.unwrap();
    assert_eq!(fetched.bytes, BYTES);
    assert_eq!(fetched.content_hash, content_hash(BYTES));
    assert!(fetched.resolved.is_none());
    let missing = dir.path().join("missing.glb");
    assert!(fetch_local(&missing)
        .await
        .unwrap_err()
        .to_string()
        .contains(missing.to_str().unwrap()));
}

#[tokio::test]
async fn refused_never_uses_a_saved_pin_and_invalid_range_does_not_request() {
    let server = MockServer::start().await;
    chain(&server, false).await;
    let resolver = Resolver::new(config(&server), Token).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cache = arora_index::Cache::new(dir.path()).unwrap();
    resolver
        .resolve_with_fallback(NAME, "*", &cache)
        .await
        .unwrap();
    server.reset().await;
    assert!(matches!(
        resolver
            .resolve_with_fallback(NAME, "invalid", &cache)
            .await,
        Err(Error::InvalidRange { .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    assert!(matches!(
        resolver.resolve_with_fallback(NAME, "*", &cache).await,
        Err(Error::Refused { .. })
    ));
}

#[tokio::test]
async fn another_expected_type_and_absent_public_role() {
    let server = MockServer::start().await;
    name(&server).await;
    let mut face = asset(false, "face");
    face["fields"].as_object_mut().unwrap().remove("publicRole");
    Mock::given(path(format!("{ROOT}/publishedAssets/asset")))
        .respond_with(ResponseTemplate::new(200).set_body_json(face))
        .mount(&server)
        .await;
    Mock::given(path(format!("{ROOT}/publishedAssets/asset/releases")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"documents": [release("release", "1.2.3")]})),
        )
        .mount(&server)
        .await;
    let pin = Resolver::new(config(&server), NoToken)
        .unwrap()
        .resolve_as(NAME, "*", "face")
        .await
        .unwrap();
    assert!(pin.private);
}

#[tokio::test]
async fn repeated_page_token_is_an_unexpected_response() {
    let server = MockServer::start().await;
    documents(&server, false, "model").await;
    Mock::given(path(format!("{ROOT}/publishedAssets/asset/releases")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"nextPageToken": "same"})))
        .mount(&server)
        .await;
    assert!(matches!(
        Resolver::new(config(&server), NoToken)
            .unwrap()
            .resolve(NAME, "*")
            .await,
        Err(Error::UnexpectedResponse { .. })
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}
