use super::*;
use crate::test_support::*;
use std::sync::atomic::Ordering;

/// Every source category drives policy through resolve, preserving unrelated pins and bytes.
#[tokio::test]
async fn fallback_categories() {
    bounded(async {
        for error in [
            SourceError::Network("timeout / refused / 408 / 429".into()),
            SourceError::NotFound("definitive no".into()),
            SourceError::Unauthorized("401".into()),
            SourceError::Malformed("bad data".into()),
            SourceError::AccessDenied("anonymous 403".into()),
            SourceError::TooLarge { limit: 1 },
        ] {
            let dir = tempfile::tempdir().unwrap();
            let cache = Cache::new(dir.path()).unwrap();
            cache.save_pin(NAME, &pin()).unwrap();
            let mut other = pin();
            other.content_hash = crate::content_hash(b"other");
            cache.save_pin("@test/other", &other).unwrap();
            cache.store(&pin().content_hash, BYTES, true).unwrap();
            cache.store(&other.content_hash, b"other", true).unwrap();
            let source = FakeSource {
                error: Some(error.clone()),
                ..Default::default()
            };
            let resolver = Resolver::new(&source as &dyn Source, cache.clone());
            let result = resolver
                .resolve_with_fallback(NAME, "^1.0.0", Kind::MODEL, "primary")
                .await;
            match error {
                SourceError::Network(_) => {
                    assert_eq!(result.unwrap().source, ResolutionSource::Cache)
                }
                SourceError::NotFound(_) => {
                    assert!(matches!(
                        result,
                        Err(Error::Source(SourceError::NotFound(_)))
                    ));
                    assert!(cache.last_pin(NAME).unwrap().is_none());
                    assert!(cache.read(&pin().content_hash, true).unwrap().is_none());
                }
                _ => {
                    assert_eq!(
                        result.unwrap_err().to_string(),
                        Error::Source(error).to_string()
                    );
                    assert!(cache.last_pin(NAME).unwrap().is_some());
                    assert!(cache.read(&pin().content_hash, true).unwrap().is_some());
                }
            }
            assert!(cache.last_pin("@test/other").unwrap().is_some());
            assert_eq!(
                cache.read(&other.content_hash, true).unwrap().unwrap(),
                b"other"
            );
        }
    })
    .await;
}

/// Offline pins must match all three request constraints; a corrupt pin cannot supply content identity.
#[tokio::test]
async fn fallback_requires_compatible_pin() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let source = FakeSource {
            error: Some(SourceError::Network("offline".into())),
            ..Default::default()
        };
        let resolver = Resolver::new(&source, cache.clone());
        for variant in 0..5 {
            let mut pin = pin();
            match variant {
                0 => pin.asset_type = Kind::FACE,
                1 => pin.artifact_key = "interface".into(),
                2 => pin.version = "2.0.0".into(),
                3 => pin.version = "broken".into(),
                _ => pin.content_hash = "bad".into(),
            }
            cache.save_pin(NAME, &pin).unwrap();
            assert!(matches!(
                resolver
                    .resolve_with_fallback(NAME, "^1.0.0", Kind::MODEL, "primary")
                    .await,
                Err(Error::Source(SourceError::Network(_)))
            ));
        }
        cache.remove_pin(NAME).unwrap();
        assert!(resolver
            .resolve_with_fallback(NAME, "*", Kind::MODEL, "primary")
            .await
            .is_err());
    })
    .await;
}

/// An exact pin checks source metadata before downloading, independently of release selection.
#[tokio::test]
async fn exact_pin_is_checked_before_download() {
    bounded(async {
        for variant in 0..8 {
            let dir = tempfile::tempdir().unwrap();
            let cache = Cache::new(dir.path()).unwrap();
            let mut source = FakeSource::default();
            let release = &mut source.releases[0];
            match variant {
                0 => {
                    release.artifacts.get_mut("primary").unwrap().content_hash =
                        crate::content_hash(b"different")
                }
                1 => release.release_id = "other-release".into(),
                2 => release.version = "1.2.4".into(),
                3 => release.scoped_name = "@test/different".into(),
                4 => release.artifacts.clear(),
                5 => release.kind = Kind::FACE,
                6 => release.private = false,
                _ => release.artifacts.get_mut("primary").unwrap().size = Some(u64::MAX),
            }
            let result = Resolver::new(&source, cache).fetch(&pin()).await;
            assert!(result.is_err(), "variant {variant}");
            if variant == 0 {
                assert!(matches!(result, Err(Error::HashMismatch { .. })));
            }
            assert_eq!(
                source.downloads.load(Ordering::SeqCst),
                0,
                "variant {variant}"
            );
            assert!(!dir.path().join("resolved.json").exists());
        }
    })
    .await;
}

/// Exact fetch stores sealed bytes, returns a cache hit without source I/O and never changes resolution pins.
#[tokio::test]
async fn fetch_never_writes_resolved_json_and_cached_fetch_never_calls_source() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let source = FakeSource::default();
        let resolver = Resolver::new(&source, cache.clone());
        assert_eq!(resolver.fetch(&pin()).await.unwrap().bytes, BYTES);
        assert!(!dir.path().join("resolved.json").exists());
        assert_eq!(
            cache.read(&pin().content_hash, true).unwrap().unwrap(),
            BYTES
        );
        let mut different = pin();
        different.release_id = "another-party".into();
        cache.save_pin(NAME, &different).unwrap();
        let before = std::fs::read(dir.path().join("resolved.json")).unwrap();
        assert_eq!(resolver.fetch(&pin()).await.unwrap().bytes, BYTES);
        assert_eq!(source.listings.load(Ordering::SeqCst), 1);
        assert_eq!(source.downloads.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read(dir.path().join("resolved.json")).unwrap(),
            before
        );
        assert!(!cache.public_path(&pin().content_hash).unwrap().exists());
    })
    .await;
}

/// Source limits reach both operations; oversized or corrupt downloads never enter the cache.
#[tokio::test]
async fn limits_and_download_verification() {
    bounded(async {
        let defaults = SourceOptions::default();
        assert_eq!(defaults.connect_timeout, Duration::from_secs(30));
        assert_eq!(defaults.read_timeout, Duration::from_secs(60));
        assert_eq!(defaults.storage_limit, 1024 * 1024 * 1024);
        for oversized in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let cache = Cache::new(dir.path()).unwrap();
            let source = FakeSource {
                bytes: b"bad bytes".to_vec(),
                ..Default::default()
            };
            let resolver = Resolver::builder(&source, cache.clone())
                .connect_timeout(Duration::from_millis(7))
                .read_timeout(Duration::from_millis(11))
                .storage_limit(if oversized { 1 } else { 100 })
                .build();
            let error = resolver.fetch(&pin()).await.unwrap_err();
            if oversized {
                assert!(matches!(error, Error::ResponseTooLarge { limit: 1, .. }));
            } else {
                assert!(matches!(error, Error::HashMismatch { .. }));
            }
            assert!(cache.read(&pin().content_hash, true).unwrap().is_none());
            let options = source.options.lock().unwrap();
            assert_eq!(options.len(), 2);
            for option in options.iter() {
                assert_eq!(option.connect_timeout, Duration::from_millis(7));
                assert_eq!(option.read_timeout, Duration::from_millis(11));
                assert_eq!(option.storage_limit, if oversized { 1 } else { 100 });
            }
        }
    })
    .await;
}

/// Resolve records the canonical pin under the requested alias, never under the current name.
#[tokio::test]
async fn live_resolution_saves_requested_name_and_validates_inputs() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let mut source = FakeSource::default();
        source.releases[0].version = "v1.2.3".into();
        let resolver = Resolver::new(&source, cache.clone());
        let result = resolver
            .resolve_with_fallback(NAME, "*", Kind::MODEL, "primary")
            .await
            .unwrap();
        assert_eq!(result.source, ResolutionSource::Source);
        assert_eq!(result.resolved.version, "1.2.3");
        assert_eq!(result.resolved.scoped_name, CURRENT);
        assert!(result.resolved.resolved_at > 0);
        assert_eq!(cache.last_pin(NAME).unwrap(), Some(result.resolved));
        assert!(cache.last_pin(CURRENT).unwrap().is_none());
        assert!(matches!(
            resolver.resolve("bad", "*", Kind::MODEL, "primary").await,
            Err(Error::InvalidName { .. })
        ));
        assert!(matches!(
            resolver.resolve(NAME, "bad", Kind::MODEL, "primary").await,
            Err(Error::InvalidRange { .. })
        ));
        assert_eq!(source.listings.load(Ordering::SeqCst), 1);
        drop(resolver);
        source.releases[0].kind = Kind::FACE;
        assert!(matches!(
            Resolver::new(&source, cache.clone())
                .resolve(NAME, "*", Kind::MODEL, "primary")
                .await,
            Err(Error::WrongAssetType { .. })
        ));
        source.releases[0].kind = Kind::MODEL;
        source.releases[0].release_id.clear();
        assert!(matches!(
            Resolver::new(&source, cache)
                .resolve(NAME, "*", Kind::MODEL, "primary")
                .await,
            Err(Error::Source(SourceError::Malformed(_)))
        ));
    })
    .await;
}

/// Save failures leave the live result usable and do not replace source errors during fallback.
#[tokio::test]
async fn cache_failures_are_advisory() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join(".lock")).unwrap();
        let mut source = FakeSource::default();
        Resolver::new(&source, cache.clone())
            .resolve(NAME, "*", Kind::MODEL, "primary")
            .await
            .unwrap();
        source.error = Some(SourceError::Network("offline".into()));
        assert!(matches!(
            Resolver::new(&source, cache)
                .resolve_with_fallback(NAME, "*", Kind::MODEL, "primary")
                .await,
            Err(Error::Source(SourceError::Network(_)))
        ));
    })
    .await;
}

/// A module pin fetches its named artifact, even when primary names different bytes.
#[tokio::test]
async fn fetch_uses_the_pinned_artifact_key() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let mut source = FakeSource::default();
        let interface = source.releases[0].artifacts["primary"].clone();
        source.releases[0]
            .artifacts
            .insert("interface".into(), interface);
        source.releases[0]
            .artifacts
            .get_mut("primary")
            .unwrap()
            .content_hash = crate::content_hash(b"other");
        let mut pin = pin();
        pin.artifact_key = "interface".into();
        let resolver = Resolver::new(&source, Cache::new(dir.path()).unwrap());
        assert_eq!(resolver.fetch(&pin).await.unwrap().bytes, BYTES);
        assert_eq!(*source.keys.lock().unwrap(), ["interface"]);
    })
    .await;
}
