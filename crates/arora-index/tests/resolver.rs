mod fake_studio;
use arora_index::{
    content_hash, fetch_local, AssetType, Cache, Error, NoToken, ResolutionSource, Resolver,
};
use fake_studio::*;
use serde_json::json;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn public_alias_resolves_and_fetches_without_authorization() {
    let server = MockServer::start().await;
    chain(&server, false).await;
    storage(&server, BYTES, 200).await;
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    let pin = resolver
        .resolve(NAME, "^1.0.0", AssetType::Model)
        .await
        .unwrap();
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
    let pin = resolver.resolve(NAME, "*", AssetType::Model).await.unwrap();
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
                .resolve(NAME, "*", AssetType::Model)
                .await
                .unwrap_err()
        } else {
            Resolver::new(config(&server), NoToken)
                .unwrap()
                .resolve(NAME, "*", AssetType::Model)
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
    let error = resolver
        .resolve(NAME, "*", AssetType::Model)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("asset document: not found, or not public"));
    server.reset().await;
    chain(&server, true).await;
    storage(&server, b"denied", 403).await;
    let pin = resolver.resolve(NAME, "*", AssetType::Model).await.unwrap();
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
        resolver.resolve(NAME, ">=1", AssetType::Model).await,
        Err(Error::InvalidRange { .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    chain(&server, false).await;
    assert!(matches!(
        resolver.resolve(NAME, "2.0.0", AssetType::Model).await,
        Err(Error::NoRelease { .. })
    ));
    storage(&server, b"wrong", 200).await;
    let pin = resolver.resolve(NAME, "*", AssetType::Model).await.unwrap();
    assert!(
        matches!(resolver.fetch(&pin).await, Err(Error::HashMismatch { expected, actual, .. }) if expected == content_hash(BYTES) && actual == content_hash(b"wrong"))
    );
    server.reset().await;
    documents(&server, false, "face").await;
    assert!(
        matches!(resolver.resolve(NAME, "*", AssetType::Model).await, Err(Error::WrongAssetType { expected, actual, .. }) if expected == "model" && actual == "face")
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
        .resolve(NAME, "*", AssetType::Model)
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
            .resolve(NAME, "*", AssetType::Model)
            .await
            .unwrap_err();
        if empty {
            assert!(matches!(err, Error::NoRelease { .. }));
        } else {
            assert!(matches!(err, Error::UnexpectedResponse { .. }));
        }
    }
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
        .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
        .await
        .unwrap();
    server.reset().await;
    assert!(matches!(
        resolver
            .resolve_with_fallback(NAME, "invalid", AssetType::Model, &cache)
            .await,
        Err(Error::InvalidRange { .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    assert!(matches!(
        resolver
            .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
            .await,
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
        .resolve(NAME, "*", AssetType::Face)
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
            .resolve(NAME, "*", AssetType::Model)
            .await,
        Err(Error::UnexpectedResponse { .. })
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[test]
fn hashes_match_known_answers() {
    assert_eq!(
        content_hash(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        content_hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[tokio::test]
async fn malformed_releases_are_skipped_and_deprecation_is_tolerant() {
    let server = MockServer::start().await;
    let mut bad_fields = release("no-fields", "9.0.0");
    bad_fields.as_object_mut().unwrap().remove("fields");
    let mut no_version = release("no-version", "9.0.0");
    no_version["fields"]
        .as_object_mut()
        .unwrap()
        .remove("version");
    let mut wrong_version = release("wrong-version", "9.0.0");
    wrong_version["fields"]["version"] = json!({"integerValue": "9"});
    let mut bad_artifacts = release("bad-artifacts", "9.0.0");
    bad_artifacts["fields"]["artifacts"] = json!({"stringValue": "bad"});
    for deprecated in [
        None,
        Some(json!({"nullValue": null})),
        Some(json!({"stringValue": "true"})),
    ] {
        server.reset().await;
        let mut chosen = release("chosen", "2.0.0");
        if let Some(deprecated) = deprecated {
            chosen["fields"]["deprecated"] = deprecated;
        }
        listing(
            &server,
            vec![
                bad_fields.clone(),
                no_version.clone(),
                wrong_version.clone(),
                bad_artifacts.clone(),
                release("invalid", "invalid"),
                chosen,
                release("old", "1.0.0"),
            ],
        )
        .await;
        let resolved = Resolver::new(config(&server), NoToken)
            .unwrap()
            .resolve(NAME, "*", AssetType::Model)
            .await
            .unwrap();
        assert_eq!(resolved.release_id, "chosen");
    }
}

#[tokio::test]
async fn equal_versions_keep_the_first_usable_release() {
    let server = MockServer::start().await;
    listing(
        &server,
        vec![
            release("first", "v2.10.0"),
            release("second", "2.10.0"),
            release("older", "2.9.0"),
        ],
    )
    .await;
    let resolved = Resolver::new(config(&server), NoToken)
        .unwrap()
        .resolve(NAME, "*", AssetType::Model)
        .await
        .unwrap();
    assert_eq!(resolved.release_id, "first");
}

#[tokio::test]
async fn invalid_names_ids_and_ranges_name_the_asset() {
    let server = MockServer::start().await;
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    for name in [
        "",
        "scope/name",
        "@/name",
        "@scope/",
        "@scope/name/other",
        "@scope:other/name",
    ] {
        let error = resolver
            .resolve(name, "*", AssetType::Model)
            .await
            .unwrap_err();
        assert!(matches!(&error, Error::InvalidName { scoped_name } if scoped_name == name));
    }
    let error = resolver
        .resolve(NAME, "bad range", AssetType::Model)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, Error::InvalidRange { scoped_name, range } if scoped_name == NAME && range == "bad range")
    );
    assert!(error.to_string().contains(NAME));
    assert!(server.received_requests().await.unwrap().is_empty());
    for id in ["", ".", "..", "one/two"] {
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"fields": {"publishedAssetId": {"stringValue": id}}})),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            resolver.resolve(NAME, "*", AssetType::Model).await,
            Err(Error::UnexpectedResponse { .. })
        ));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
    for name in [
        None,
        Some("publishedAssets/asset/releases/"),
        Some("publishedAssets/asset/releases/.."),
        Some("."),
    ] {
        server.reset().await;
        let mut document = release("ignored", "invalid");
        match name {
            Some(name) => document["name"] = json!(name),
            None => {
                document.as_object_mut().unwrap().remove("name");
            }
        }
        listing(&server, vec![document, release("valid", "1.0.0")]).await;
        assert!(matches!(
            resolver.resolve(NAME, "*", AssetType::Model).await,
            Err(Error::UnexpectedResponse { .. })
        ));
    }
}

#[tokio::test]
async fn cache_failures_never_replace_live_success_or_network_error() {
    capture_warnings();
    let server = MockServer::start().await;
    chain(&server, false).await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    fs::create_dir(dir.path().join("resolved.json")).unwrap();
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    let resolved = resolver
        .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
        .await
        .unwrap();
    assert_eq!(resolved.source, ResolutionSource::Server);
    assert!(lock(&WARNINGS.0)
        .iter()
        .any(|m| m.contains("could not save resolved pin") && m.contains(NAME)));
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    for truncated in [false, true] {
        if truncated {
            fs::remove_dir(dir.path().join("resolved.json")).unwrap();
            fs::write(dir.path().join("resolved.json"), "{").unwrap();
        }
        assert!(matches!(
            resolver
                .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
                .await,
            Err(Error::Network { .. })
        ));
    }
}

#[tokio::test]
async fn resolution_reports_source_type_and_millisecond_timestamp() {
    let server = MockServer::start().await;
    chain(&server, false).await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    let millis = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let before = millis();
    let online = resolver
        .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
        .await
        .unwrap();
    let after = millis();
    assert!((before..=after).contains(&online.resolved.resolved_at));
    assert_eq!(online.source, ResolutionSource::Server);
    assert_eq!(online.resolved.storage_path.as_deref(), Some(OBJECT));
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let offline = resolver
        .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
        .await
        .unwrap();
    assert_eq!(offline.source, ResolutionSource::Cache);
    assert_eq!(offline.resolved.storage_path, None);
    let error = resolver
        .fetch(&offline.resolved)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains(CURRENT) && error.contains("cache by hash"));
    assert!(matches!(
        resolver
            .resolve_with_fallback(NAME, "*", AssetType::Face, &cache)
            .await,
        Err(Error::Network { .. })
    ));
    assert_eq!(AssetType::default(), AssetType::Model);
    for (kind, text) in [
        (AssetType::Model, "model"),
        (AssetType::Animation, "animation"),
        (AssetType::Face, "face"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), text);
        assert_eq!(
            serde_json::from_value::<AssetType>(json!(text)).unwrap(),
            kind
        );
    }
}

#[tokio::test]
async fn unauthorized_never_falls_back_but_transient_statuses_do() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    cache.save_pin(NAME, &pin()).unwrap();
    let resolver = Resolver::new(config(&server), Token).unwrap();
    for status in [401, 429, 408] {
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let live = resolver
            .resolve(NAME, "*", AssetType::Model)
            .await
            .unwrap_err();
        let result = resolver
            .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
            .await;
        if status == 401 {
            assert!(matches!(live, Error::Unauthorized { .. }));
            assert!(live.to_string().contains("refresh the token"));
            assert!(matches!(result, Err(Error::Unauthorized { .. })));
        } else {
            assert!(matches!(live, Error::Network { .. }));
            assert_eq!(result.unwrap().source, ResolutionSource::Cache);
        }
    }
}

#[tokio::test]
async fn connection_failure_and_idle_timeout_allow_fallback_and_name_the_cause() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    cache.save_pin(NAME, &pin()).unwrap();
    let mut refused = config(&server);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    refused.firestore_url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let resolver = Resolver::builder(refused, NoToken)
        .connect_timeout(Duration::from_millis(100))
        .read_timeout(Duration::from_millis(100))
        .build()
        .unwrap();
    let error = resolver
        .resolve(NAME, "*", AssetType::Model)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, Error::Network { detail, .. } if detail.to_lowercase().contains("connection refused")),
        "{error}"
    );
    assert_eq!(
        resolver
            .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
            .await
            .unwrap()
            .source,
        ResolutionSource::Cache
    );
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(250)))
        .mount(&server)
        .await;
    let resolver = Resolver::builder(config(&server), NoToken)
        .read_timeout(Duration::from_millis(20))
        .build()
        .unwrap();
    let error = resolver
        .resolve(NAME, "*", AssetType::Model)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, Error::Network { detail, .. } if detail.contains("timed out")),
        "{error}"
    );
    assert_eq!(
        resolver
            .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
            .await
            .unwrap()
            .source,
        ResolutionSource::Cache
    );
}

#[tokio::test]
async fn response_caps_apply_to_firestore_and_storage_and_do_not_fallback() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    cache.save_pin(NAME, &pin()).unwrap();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b' '; 16 * 1024 * 1024 + 1]))
        .mount(&server)
        .await;
    let resolver = Resolver::builder(config(&server), NoToken)
        .storage_limit(BYTES.len() - 1)
        .build()
        .unwrap();
    let error = resolver
        .resolve_with_fallback(NAME, "*", AssetType::Model, &cache)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        Error::ResponseTooLarge {
            limit: 16_777_216,
            ..
        }
    ));
    assert!(error.to_string().contains("16777216"));
    server.reset().await;
    storage(&server, BYTES, 200).await;
    let error = resolver.fetch(&pin()).await.unwrap_err();
    assert!(matches!(error, Error::ResponseTooLarge { limit, .. } if limit == BYTES.len()-1));
    let resolver = Resolver::builder(config(&server), NoToken)
        .storage_limit(BYTES.len())
        .build()
        .unwrap();
    assert_eq!(resolver.fetch(&pin()).await.unwrap().bytes, BYTES);
}

#[tokio::test]
async fn selected_release_errors_never_choose_an_older_release() {
    let server = MockServer::start().await;
    for (field, value) in [
        ("contentHash", Some(json!({"stringValue": "bad"}))),
        ("contentHash", Some(json!({"stringValue": "A".repeat(64)}))),
        ("storage", None),
        ("storage", Some(json!({"stringValue": "wrong type"}))),
        ("storage", Some(json!({"mapValue": {"fields": {}}}))),
        (
            "storage",
            Some(json!({"mapValue": {"fields": {"fileBucket": {"stringValue": ""}}}})),
        ),
        (
            "storage",
            Some(json!({"mapValue": {"fields": {"fileBucket": {"stringValue": "."}}}})),
        ),
        (
            "storage",
            Some(json!({"mapValue": {"fields": {"fileBucket": {"stringValue": ".."}}}})),
        ),
    ] {
        server.reset().await;
        let mut newer = release("newer", "2.0.0");
        let primary = newer["fields"]["artifacts"]["mapValue"]["fields"]["primary"]["mapValue"]
            ["fields"]
            .as_object_mut()
            .unwrap();
        match value {
            Some(value) => {
                primary.insert(field.into(), value);
            }
            None => {
                primary.remove(field);
            }
        }
        listing(&server, vec![release("older", "1.0.0"), newer]).await;
        let error = Resolver::new(config(&server), NoToken)
            .unwrap()
            .resolve(NAME, "*", AssetType::Model)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::UnexpectedResponse { .. }));
        assert!(
            error.to_string().contains("newer") && error.to_string().contains(NAME),
            "{error}"
        );
    }
}

#[tokio::test]
async fn non_string_family_warns_and_keeps_the_newest_release() {
    capture_warnings();
    let server = MockServer::start().await;
    for (i, family) in [
        json!({"integerValue": "42"}),
        json!({"nullValue": null}),
        json!({"booleanValue": false}),
    ]
    .into_iter()
    .enumerate()
    {
        server.reset().await;
        let id = format!("family-not-string-{i}");
        let mut newer = release(&id, "2.0.0");
        newer["fields"]["family"] = family;
        listing(&server, vec![release("older", "1.0.0"), newer]).await;
        let pin = Resolver::new(config(&server), NoToken)
            .unwrap()
            .resolve(NAME, "*", AssetType::Model)
            .await
            .unwrap();
        assert_eq!(pin.release_id, id);
        assert_eq!(pin.family, None);
        assert!(lock(&WARNINGS.0)
            .iter()
            .any(|m| m.contains(&id) && m.contains("family") && m.contains("absent")));
    }
}

#[tokio::test]
async fn empty_or_non_string_hashes_are_not_candidates() {
    let server = MockServer::start().await;
    let mut releases = vec![];
    for hash in [json!({"stringValue": ""}), json!({"integerValue": "123"})] {
        let mut newer = release("no-hash", "2.0.0");
        newer["fields"]["artifacts"]["mapValue"]["fields"]["primary"]["mapValue"]["fields"]
            ["contentHash"] = hash;
        releases.push(newer);
    }
    releases.push(release("valid", "1.0.0"));
    listing(&server, releases).await;
    let pin = Resolver::new(config(&server), NoToken)
        .unwrap()
        .resolve(NAME, "*", AssetType::Model)
        .await
        .unwrap();
    assert_eq!(pin.release_id, "valid");
}

#[tokio::test]
async fn fetch_rejects_invalid_storage_paths_before_http() {
    let server = MockServer::start().await;
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    for path in ["", ".", ".."] {
        let mut pin = pin();
        pin.storage_path = Some(path.into());
        let error = resolver.fetch(&pin).await.unwrap_err();
        assert!(matches!(error, Error::UnexpectedResponse { .. }));
        assert!(error.to_string().contains(&pin.release_id));
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn local_hashing_runs_off_the_executor() {
    capture_warnings();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model");
    let bytes = vec![42; 1231];
    fs::write(&path, &bytes).unwrap();
    let since = lock(&HASH_THREADS).len();
    let fetched = fetch_local(path).await.unwrap();
    assert_eq!(fetched.bytes, bytes);
    assert_hash_off_executor(bytes.len(), since);
}

#[tokio::test(flavor = "current_thread")]
async fn storage_verification_runs_off_the_executor() {
    capture_warnings();
    let server = MockServer::start().await;
    let bytes = vec![43; 1237];
    storage(&server, &bytes, 200).await;
    let mut pin = pin();
    pin.content_hash = content_hash(&bytes);
    let since = lock(&HASH_THREADS).len();
    let fetched = Resolver::new(config(&server), NoToken)
        .unwrap()
        .fetch(&pin)
        .await
        .unwrap();
    assert_eq!(fetched.bytes, bytes);
    assert_hash_off_executor(bytes.len(), since);
}
