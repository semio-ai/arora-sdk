mod support;
use arora_index::{content_hash, Cache, Error, NoToken, ResolvedRef, Resolver};
use std::fs;
use support::*;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pin(private: bool) -> ResolvedRef {
    ResolvedRef {
        scoped_name: CURRENT.into(),
        published_asset_id: "asset".into(),
        version: "1.2.3".into(),
        release_id: "release".into(),
        artifact_key: "primary".into(),
        content_hash: content_hash(BYTES),
        storage_path: OBJECT.into(),
        family: Some("test".into()),
        private,
        resolved_at: 123,
    }
}

#[test]
fn verified_hits_corruption_and_visibility_are_separate() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let hash = content_hash(BYTES);
    assert!(cache.read(&hash, false).unwrap().is_none());
    cache.store(&hash, BYTES, false).unwrap();
    assert_eq!(cache.read(&hash, false).unwrap().unwrap(), BYTES);
    assert!(cache.read(&hash, true).unwrap().is_none());
    fs::write(cache.public_path(&hash).unwrap(), b"corrupt").unwrap();
    assert!(cache.read(&hash, false).unwrap().is_none());
    assert!(!cache.public_path(&hash).unwrap().exists());
    assert!(cache.store(&hash, b"wrong", false).is_err());
    assert!(cache.read("../key", false).is_err());
}

fn assert_no_plaintext(dir: &std::path::Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            assert_no_plaintext(&path);
        } else {
            assert!(!fs::read(path)
                .unwrap()
                .windows(BYTES.len())
                .any(|w| w == BYTES));
        }
    }
}

#[test]
fn private_bytes_stay_sealed_even_after_reading() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let pin = pin(true);
    cache.store(&pin.content_hash, BYTES, true).unwrap();
    let sealed_path = dir
        .path()
        .join("blobs")
        .join(format!("{}.sealed", pin.content_hash));
    assert_eq!(fs::read(&sealed_path).unwrap().len(), BYTES.len() + 24 + 16);
    let fetched = cache.fetch(&pin).unwrap().unwrap();
    assert_eq!(fetched.bytes, BYTES);
    assert_eq!(fetched.resolved, Some(pin.clone()));
    assert_no_plaintext(dir.path());
    assert!(!cache.public_path(&pin.content_hash).unwrap().exists());
    assert!(cache.read(&pin.content_hash, false).unwrap().is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(dir.path().join("key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    fs::write(&sealed_path, b"invalid ciphertext").unwrap();
    assert!(cache.read(&pin.content_hash, true).unwrap().is_none());
    assert!(!sealed_path.exists());
}

#[test]
fn changed_key_makes_private_blob_a_miss() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let hash = content_hash(BYTES);
    cache.store(&hash, BYTES, true).unwrap();
    fs::write(dir.path().join("key"), [0u8; 32]).unwrap();
    assert!(cache.read(&hash, true).unwrap().is_none());
    assert_eq!(fs::read_dir(dir.path().join("blobs")).unwrap().count(), 0);
}

#[test]
fn pins_use_requested_names_and_studio_field_names() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let pin = pin(false);
    cache.save_pin(NAME, &pin).unwrap();
    cache.save_pin("@test/other", &pin).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("resolved.json")).unwrap()).unwrap();
    assert!(value.get(CURRENT).is_none());
    let entry = value[NAME].as_object().unwrap();
    let mut keys: Vec<_> = entry.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "artifactKey",
            "contentHash",
            "family",
            "private",
            "publishedAssetId",
            "releaseId",
            "resolvedAt",
            "scopedName",
            "version"
        ]
    );
    assert_eq!(entry["scopedName"], CURRENT);
    assert_eq!(entry["resolvedAt"], 123);
    let mut offline_pin = pin.clone();
    offline_pin.storage_path.clear();
    assert_eq!(cache.last_pin(NAME).unwrap(), Some(offline_pin));
    assert_eq!(serde_json::to_value(pin).unwrap()["storagePath"], OBJECT);
}

#[test]
fn independent_cache_handles_preserve_keys_and_all_pins() {
    let dir = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        for i in 0..12 {
            let path = dir.path();
            scope.spawn(move || {
                let cache = Cache::new(path).unwrap();
                let mut pin = pin(true);
                let bytes = format!("private bytes {i}");
                pin.content_hash = content_hash(bytes.as_bytes());
                cache
                    .store(&pin.content_hash, bytes.as_bytes(), true)
                    .unwrap();
                cache.save_pin(&format!("@test/{i}"), &pin).unwrap();
            });
        }
    });
    let cache = Cache::new(dir.path()).unwrap();
    for i in 0..12 {
        let pin = cache.last_pin(&format!("@test/{i}")).unwrap().unwrap();
        assert_eq!(
            cache.fetch(&pin).unwrap().unwrap().bytes,
            format!("private bytes {i}").as_bytes()
        );
    }
}

#[tokio::test]
async fn only_network_errors_allow_a_compatible_last_pin() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let resolver = Resolver::new(config(&server), NoToken).unwrap();
    chain(&server, false).await;
    let online = resolver
        .resolve_with_fallback(NAME, "*", &cache)
        .await
        .unwrap();
    assert_eq!(
        cache.last_pin(NAME).unwrap().unwrap().version,
        online.version
    );
    for status in [503, 403, 404, 400, 200] {
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_string("not JSON"))
            .mount(&server)
            .await;
        let result = resolver.resolve_with_fallback(NAME, "*", &cache).await;
        if status == 503 {
            assert_eq!(result.unwrap().version, "1.2.3");
            assert!(matches!(
                resolver.resolve_with_fallback(NAME, "2.0.0", &cache).await,
                Err(Error::Network { .. })
            ));
        } else {
            assert!(result.is_err());
        }
    }
    server.reset().await;
    documents(&server, false, "face").await;
    assert!(matches!(
        resolver.resolve_with_fallback(NAME, "*", &cache).await,
        Err(Error::WrongAssetType { .. })
    ));
    server.reset().await;
    chain(&server, false).await;
    assert!(matches!(
        resolver.resolve_with_fallback(NAME, "2.0.0", &cache).await,
        Err(Error::NoRelease { .. })
    ));
}

#[test]
fn decrypted_hash_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let wrong = content_hash(b"different bytes");
    let expected = content_hash(BYTES);
    cache.store(&wrong, b"different bytes", true).unwrap();
    let source = dir.path().join("blobs").join(format!("{wrong}.sealed"));
    let destination = dir.path().join("blobs").join(format!("{expected}.sealed"));
    fs::rename(source, &destination).unwrap();
    assert!(cache.read(&expected, true).unwrap().is_none());
    assert!(!destination.exists());
}
