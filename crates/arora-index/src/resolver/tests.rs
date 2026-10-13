use super::*;
use crate::content_hash;
use crate::test_support::*;
use serde_json::json;
use std::fs;

/// Asserts selection is stable and skips unusable releases.
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

/// Asserts builder timeouts have no total deadline.
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

/// Asserts cap checks each chunk before extending the buffer.
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

/// Asserts oversized stream stops before polling the rest.
#[tokio::test]
async fn oversized_stream_stops_before_polling_the_rest() {
    bounded(async {
        let mut polled = 0;
        let stream =
            futures_util::stream::iter((0..10).map(|_| Ok(vec![0; 3]))).inspect(|_| polled += 1);
        let error = read_body(stream, 5, "@test/model", "bytes")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::ResponseTooLarge { limit: 5, .. }));
        assert_eq!(polled, 2);
    })
    .await;
}

/// Asserts environment configuration uses injected lookup.
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

fn resolved(documents: &[Value], range: &str) -> Result<ResolvedRef, Error> {
    resolved_release(
        documents,
        NAME,
        range,
        &Range::parse(NAME, range)?,
        CURRENT,
        "asset",
        false,
        AssetType::Model,
    )
}

/// Missing fields and invalid typed values are not candidates; only boolean true deprecates.
#[test]
fn malformed_releases_are_skipped_and_deprecation_is_tolerant() {
    let mut missing_version = release("no-version", "9.0.0");
    missing_version["fields"]
        .as_object_mut()
        .unwrap()
        .remove("version");
    let mut non_string = release("number", "9.0.0");
    non_string["fields"]["version"] = json!({"integerValue":"9"});
    let mut artifacts = release("bad-map", "9.0.0");
    artifacts["fields"]["artifacts"] = json!({"stringValue":"bad"});
    for deprecated in [
        None,
        Some(json!({"nullValue":null})),
        Some(json!({"stringValue":"true"})),
    ] {
        let mut chosen = release("chosen", "2.0.0");
        if let Some(value) = deprecated {
            chosen["fields"]["deprecated"] = value;
        }
        let documents = vec![
            json!({"name":"no-fields"}),
            missing_version.clone(),
            non_string.clone(),
            artifacts.clone(),
            release("invalid", "bad"),
            chosen,
        ];
        assert_eq!(resolved(&documents, "*").unwrap().release_id, "chosen");
    }
}

/// Distinguishes nonempty malformed hashes (selected, then rejected) from absent candidates.
#[test]
fn selected_release_errors_never_choose_an_older_release() {
    for (field, value) in [
        ("contentHash", Some(json!({"stringValue":"bad"}))),
        ("contentHash", Some(json!({"stringValue":"A".repeat(64)}))),
        ("storage", None),
        ("storage", Some(json!({"stringValue":"bad"}))),
        ("storage", Some(json!({"mapValue":{"fields":{}}}))),
        (
            "storage",
            Some(json!({"mapValue":{"fields":{"fileBucket":{"stringValue":""}}}})),
        ),
        (
            "storage",
            Some(json!({"mapValue":{"fields":{"fileBucket":{"stringValue":"."}}}})),
        ),
        (
            "storage",
            Some(json!({"mapValue":{"fields":{"fileBucket":{"stringValue":".."}}}})),
        ),
    ] {
        let mut newer = release("newer", "2.0.0");
        let primary = newer["fields"]["artifacts"]["mapValue"]["fields"]["primary"]["mapValue"]
            ["fields"]
            .as_object_mut()
            .unwrap();
        if let Some(value) = value {
            primary.insert(field.into(), value);
        } else {
            primary.remove(field);
        }
        let error = resolved(&[release("older", "1.0.0"), newer], "*").unwrap_err();
        assert!(matches!(error, Error::UnexpectedResponse { .. }));
        assert!(error.to_string().contains("newer") && error.to_string().contains(NAME));
    }
}

/// Typed null and other non-string family values warn without changing the selected release.
#[test]
fn non_string_family_warns_and_keeps_the_newest_release() {
    capture_warnings();
    for (i, value) in [
        json!({"integerValue":"42"}),
        json!({"nullValue":null}),
        json!({"booleanValue":false}),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("bad-family-{i}");
        let mut newer = release(&id, "2.0.0");
        newer["fields"]["family"] = value;
        let pin = resolved(&[release("older", "1.0.0"), newer], "*").unwrap();
        assert_eq!(pin.release_id, id);
        assert_eq!(pin.family, None);
        assert!(lock(&WARNINGS.0)
            .iter()
            .any(|m| m.contains(&id) && m.contains("absent")));
    }
}

/// Empty and non-string hashes are skipped, unlike malformed nonempty strings.
#[test]
fn empty_or_non_string_hashes_are_not_candidates() {
    for hash in [json!({"stringValue":""}), json!({"integerValue":"123"})] {
        let mut newer = release("no-hash", "2.0.0");
        newer["fields"]["artifacts"]["mapValue"]["fields"]["primary"]["mapValue"]["fields"]
            ["contentHash"] = hash;
        assert_eq!(
            resolved(&[newer, release("valid", "1.0.0")], "*")
                .unwrap()
                .release_id,
            "valid"
        );
    }
    assert!(matches!(
        resolved(&[release("old", "1.0.0")], "2.0.0"),
        Err(Error::NoRelease { .. })
    ));
}

/// Each invalid segment is rejected before URL construction, and errors retain the requested name.
#[test]
fn invalid_names_ids_and_ranges_name_the_asset() {
    for name in [
        "",
        "scope/name",
        "@/name",
        "@scope/",
        "@scope/name/other",
        "@scope:other/name",
    ] {
        assert!(
            matches!(validate_name(name), Err(Error::InvalidName {scoped_name}) if scoped_name == name)
        );
    }
    validate_name(NAME).unwrap();
    for id in ["", ".", "..", "one/two"] {
        let error = validate_id(id, NAME, "name lookup").unwrap_err();
        assert!(error.to_string().contains(NAME));
    }
    for name in [None, Some("releases/"), Some("releases/.."), Some(".")] {
        let mut doc = release("invalid", "bad");
        if let Some(name) = name {
            doc["name"] = json!(name);
        } else {
            doc.as_object_mut().unwrap().remove("name");
        }
        assert!(matches!(
            resolved(&[doc], "*"),
            Err(Error::UnexpectedResponse { .. })
        ));
    }
    assert!(
        matches!(Range::parse(NAME,"bad"),Err(Error::InvalidRange{scoped_name,range}) if scoped_name==NAME && range=="bad")
    );
}

/// Pins with absent paths explain offline retrieval; dot segments cannot reach the URL builder.
#[test]
fn storage_paths_and_offline_pins_are_validated() {
    let cfg = config();
    for path in [Some(""), Some("."), Some(".."), None] {
        let mut pin = pin();
        pin.storage_path = path.map(str::to_owned);
        let error = storage_endpoint(&cfg, &pin).unwrap_err();
        if path.is_none() {
            assert!(error.to_string().contains("cache by hash"));
        } else {
            assert!(matches!(error, Error::UnexpectedResponse { .. }));
            assert!(error.to_string().contains(&pin.release_id));
        }
    }
}

/// The typed Firestore contract distinguishes absent values, wrong types and empty maps.
#[test]
fn firestore_values_and_asset_identity() {
    let value = json!({"fields":{
        "text":{"stringValue":"ok"}, "null":{"nullValue":null}, "number":{"integerValue":"3"},
        "empty":{"mapValue":{}}, "map":{"mapValue":{"fields":{"text":{"stringValue":"nested"}}}},
        "badMap":{"mapValue":{"fields":false}},"missingMapFields":{"mapValue":{"other":1}}
    }});
    let fields = Fields::document(&value, NAME, "parse").unwrap();
    assert_eq!(fields.string("text").unwrap(), "ok");
    for key in ["null", "missing"] {
        assert_eq!(fields.optional_string(key).unwrap(), None);
        assert!(fields.string(key).is_err());
    }
    assert!(fields.string("number").is_err());
    assert!(fields.map("missing").unwrap().is_none());
    assert!(fields.map("empty").unwrap().unwrap().fields.is_empty());
    assert_eq!(
        fields.map("map").unwrap().unwrap().string("text").unwrap(),
        "nested"
    );
    for key in ["text", "badMap", "missingMapFields"] {
        assert!(fields.map(key).is_err());
    }
    assert!(Fields::document(&json!({}), NAME, "parse").is_err());
    assert!(matches!(
        asset_identity(&asset(false, "face"), NAME, AssetType::Model),
        Err(Error::WrongAssetType { expected, actual, .. }) if expected == "model" && actual == "face"
    ));
    let mut face = asset(true, "face");
    face["fields"].as_object_mut().unwrap().remove("publicRole");
    assert_eq!(
        asset_identity(&face, NAME, AssetType::Face).unwrap(),
        (CURRENT.into(), true)
    );
}

/// Empty pages are valid; malformed documents and cyclic page tokens are not.
#[test]
fn release_listing_shapes_and_page_tokens() {
    let mut docs = vec![];
    let mut seen = HashSet::new();
    assert_eq!(
        release_page(&json!({}), NAME, &mut docs, &mut seen).unwrap(),
        None
    );
    for value in [
        json!([]),
        json!({"documents":false}),
        json!({"nextPageToken":""}),
        json!({"nextPageToken":1}),
    ] {
        assert!(release_page(&value, NAME, &mut docs, &mut seen).is_err());
    }
    let page = json!({"documents":[release("one","1.0.0")],"nextPageToken":"next"});
    assert_eq!(
        release_page(&page, NAME, &mut docs, &mut seen).unwrap(),
        Some("next".into())
    );
    assert!(release_page(&page, NAME, &mut docs, &mut seen).is_err());
}

fn status(code: u16, token: bool) -> Result<(), Error> {
    classify_status(
        reqwest::StatusCode::from_u16(code).unwrap(),
        token,
        NAME,
        "document",
    )
}

/// Pins the status-to-error boundary independently of HTTP transport and fallback policy.
#[test]
fn status_classification_preserves_identity_and_transience() {
    for token in [false, true] {
        for code in [401, 403, 404, 408, 429, 500, 503] {
            let error = status(code, token).unwrap_err();
            assert!(error.to_string().contains(NAME));
            assert!(error.to_string().contains("document"));
        }
        for step in ["name lookup", "asset document", "Storage bytes"] {
            let error =
                classify_status(reqwest::StatusCode::FORBIDDEN, false, NAME, step).unwrap_err();
            assert!(error
                .to_string()
                .contains(&format!("{step}: not found, or not public")));
        }
        assert!(matches!(
            status(404, token),
            Err(Error::NotFound {
                http_status: 404,
                ..
            })
        ));
        let error = status(403, token).unwrap_err();
        if token {
            assert!(matches!(error, Error::Refused { .. }));
        } else {
            assert!(matches!(
                error,
                Error::NotFound {
                    http_status: 403,
                    ..
                }
            ));
            assert!(error.to_string().contains("not found, or not public"));
        }
        assert!(matches!(
            status(401, token),
            Err(Error::Unauthorized { .. })
        ));
        for code in [408, 429, 500, 503] {
            assert!(matches!(status(code, token), Err(Error::Network { .. })));
        }
    }
    assert!(status(200, false).is_ok());
    assert!(matches!(
        status(400, false),
        Err(Error::UnexpectedResponse { .. })
    ));
}

async fn finish(result: Result<ResolvedRef, Error>, cache: &Cache) -> Result<Resolution, Error> {
    finish_resolution(
        result,
        NAME,
        &Range::parse(NAME, "*").unwrap(),
        AssetType::Model,
        cache,
    )
    .await
}

/// Only Network allows a compatible pin; errors retain their category and definitive answers clear it.
#[tokio::test]
async fn fallback_policy_and_pin_compatibility() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        for (code, token) in [
            (403, false),
            (403, true),
            (404, false),
            (401, true),
            (408, false),
            (429, false),
            (503, false),
            (400, false),
        ] {
            cache.save_pin(NAME, &pin()).unwrap();
            let result = finish(Err(status(code, token).unwrap_err()), &cache).await;
            if [408, 429, 503].contains(&code) {
                assert_eq!(result.unwrap().source, ResolutionSource::Cache);
            } else {
                assert!(result.is_err());
            }
            assert_eq!(
                cache.last_pin(NAME).unwrap().is_none(),
                code == 404 || (code == 403 && token)
            );
        }
        for (kind, range) in [(AssetType::Face, "*"), (AssetType::Model, "2.0.0")] {
            cache.save_pin(NAME, &pin()).unwrap();
            let result = finish_resolution(
                Err(status(503, false).unwrap_err()),
                NAME,
                &Range::parse(NAME, range).unwrap(),
                kind,
                &cache,
            )
            .await;
            assert!(matches!(result, Err(Error::Network { .. })));
        }
        for error in [
            Error::NoRelease {
                scoped_name: NAME.into(),
                range: "*".into(),
            },
            unexpected(NAME, "parse", "bad"),
            Error::ResponseTooLarge {
                scoped_name: NAME.into(),
                step: "body".into(),
                limit: 1,
            },
            Error::WrongAssetType {
                scoped_name: NAME.into(),
                expected: "model".into(),
                actual: "face".into(),
            },
        ] {
            assert!(finish(Err(error), &cache).await.is_err());
        }
    })
    .await;
}

/// Advisory cache failures cannot replace either a live success or the original network/asset error.
#[tokio::test]
async fn cache_failures_never_replace_live_results() {
    bounded(async {
        capture_warnings();
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        fs::create_dir(dir.path().join(".lock")).unwrap();
        let result = finish(Ok(pin()), &cache).await.unwrap();
        assert_eq!(result.source, ResolutionSource::Server);
        assert!(lock(&WARNINGS.0)
            .iter()
            .any(|m| m.contains(NAME) && m.contains("could not save resolved pin")));
        for code in [404, 403] {
            assert!(matches!(
                finish(Err(status(code, true).unwrap_err()), &cache).await,
                Err(Error::NotFound { .. } | Error::Refused { .. })
            ));
        }
        assert!(lock(&WARNINGS.0)
            .iter()
            .any(|m| m.contains(dir.path().to_str().unwrap())
                && m.contains("could not clear resolved pin")));
        fs::create_dir(dir.path().join("resolved.json")).unwrap();
        assert!(matches!(
            finish(Err(status(503, false).unwrap_err()), &cache).await,
            Err(Error::Network { .. })
        ));
        fs::remove_dir(dir.path().join("resolved.json")).unwrap();
        fs::write(dir.path().join("resolved.json"), "{").unwrap();
        assert!(matches!(
            finish(Err(status(503, false).unwrap_err()), &cache).await,
            Err(Error::Network { .. })
        ));
    })
    .await;
}

/// Pins the persisted source/path distinction, millisecond clock, and Studio enum spellings.
#[tokio::test]
async fn resolution_reports_source_type_and_millisecond_timestamp() {
    bounded(async {
        let millis = || {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
        };
        let before = millis();
        let pin = resolved(&[release("release", "1.2.3")], "*").unwrap();
        let after = millis();
        assert!((before..=after).contains(&pin.resolved_at));
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let live = finish(Ok(pin), &cache).await.unwrap();
        assert_eq!(live.source, ResolutionSource::Server);
        assert_eq!(live.resolved.storage_path.as_deref(), Some(OBJECT));
        let offline = finish(Err(status(503, false).unwrap_err()), &cache)
            .await
            .unwrap();
        assert_eq!(offline.resolved.storage_path, None);
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
    })
    .await;
}

fn config() -> StudioConfig {
    StudioConfig {
        project_id: "demo-test".into(),
        storage_bucket: "fixture".into(),
        firestore_url: "http://127.0.0.1:1".into(),
        storage_url: "http://127.0.0.1:1".into(),
    }
}

/// The same worker verifies downloaded bytes and rejects a hash mismatch without any transport.
#[tokio::test(flavor = "current_thread")]
async fn storage_verification_runs_off_the_executor() {
    bounded(async {
        capture_warnings();
        let bytes = vec![43; 1237];
        let mut pin = pin();
        pin.content_hash = content_hash(&bytes);
        let since = lock(&HASH_THREADS).len();
        let fetched = verified_asset(&pin, bytes.clone()).await.unwrap();
        assert_eq!(fetched.bytes, bytes);
        assert_hash_off_executor(bytes.len(), since);
        assert!(matches!(
            verified_asset(&pin, b"wrong".to_vec()).await,
            Err(Error::HashMismatch { expected, actual, .. }) if expected == content_hash(&bytes) && actual == content_hash(b"wrong")
        ));
    })
    .await;
}

/// Save and clear must yield while a cache writer holds the lock; the watchdog bounds regressions.
#[tokio::test(flavor = "current_thread")]
async fn busy_pin_save_and_clear_leave_the_executor_free() {
    bounded(async {
        for clearing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let cache = Cache::new(dir.path()).unwrap();
            cache.save_pin(NAME, &pin()).unwrap();
            let lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(dir.path().join(".lock"))
                .unwrap();
            lock.lock().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let ran = rx.recv_timeout(Duration::from_millis(500)).is_ok();
                drop(lock);
                done_tx.send(ran).unwrap();
            });
            let result = if clearing {
                Err(status(404, false).unwrap_err())
            } else {
                Ok(pin())
            };
            let (result, ()) =
                tokio::join!(biased; finish(result,&cache),async {let _=tx.send(());});
            assert_eq!(result.is_err(), clearing);
            assert!(done_rx.recv_timeout(Duration::from_millis(750)).unwrap());
            assert_eq!(cache.last_pin(NAME).unwrap().is_none(), clearing);
        }
    })
    .await;
}

/// Limits are chosen before streaming: Firestore is fixed and only Storage is configurable.
#[test]
fn response_limits_choose_the_service_and_builder_override() {
    let resolver = Resolver::new(config(), NoToken).unwrap();
    assert_eq!(resolver.body_limit(false), 16 * 1024 * 1024);
    assert_eq!(resolver.body_limit(true), 1024 * 1024 * 1024);
    let resolver = Resolver::builder(config(), NoToken)
        .storage_limit(7)
        .build()
        .unwrap();
    assert_eq!(resolver.body_limit(true), 7);
    assert_eq!(resolver.body_limit(false), 16 * 1024 * 1024);
}

/// A real local TCP peer is needed to observe reqwest's idle timeout and its source chain.
#[tokio::test]
async fn idle_timeout_is_network_and_names_its_cause() {
    bounded(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let resolver = Resolver::builder(config(), NoToken)
            .read_timeout(Duration::from_millis(20))
            .build()
            .unwrap();
        let (result, ()) = tokio::join!(resolver.read(NAME, "timeout", url, false), async {
            let (_peer, _) = tokio::time::timeout(Duration::from_millis(500), listener.accept())
                .await
                .unwrap()
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        assert!(matches!(result,Err(Error::Network{detail,..}) if detail.contains("timed out")));
    })
    .await;
}

/// The HTTP call sites enforce download verification, token-aware status errors and the Storage cap.
#[tokio::test]
async fn fetch_and_read_enforce_verification_identity_and_size_limit() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Token;
    #[async_trait::async_trait]
    impl TokenSource for Token {
        async fn token(&self) -> Result<Option<String>, Error> {
            Ok(Some("test-token".into()))
        }
    }

    bounded(async {
        let server = MockServer::start().await;
        let mut cfg = config();
        cfg.firestore_url = server.uri();
        cfg.storage_url = server.uri();
        let url = Url::parse(&server.uri()).unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"wrong"))
            .mount(&server)
            .await;
        let resolver = Resolver::new(cfg.clone(), NoToken).unwrap();
        assert!(matches!(
            resolver.fetch(&pin()).await,
            Err(Error::HashMismatch { expected, actual, .. })
                if expected == content_hash(BYTES) && actual == content_hash(b"wrong")
        ));
        let limited = Resolver::builder(cfg.clone(), NoToken)
            .storage_limit(4)
            .build()
            .unwrap();
        assert!(matches!(
            limited.read(NAME, "Storage bytes", url.clone(), true).await,
            Err(Error::ResponseTooLarge { limit: 4, .. })
        ));

        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let error = resolver
            .read(NAME, "asset document", url.clone(), false)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::NotFound {
                http_status: 403,
                ..
            }
        ));
        assert!(error.to_string().contains("not found, or not public"));
        let authenticated = Resolver::new(cfg, Token).unwrap();
        assert!(matches!(
            authenticated.read(NAME, "asset document", url, false).await,
            Err(Error::Refused { .. })
        ));
    })
    .await;
}
