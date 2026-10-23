#![cfg(feature = "client")]
use arora_index::{content_hash, Cache, DirSource, Kind, Resolver};
use serde_json::json;

/// Connects local release metadata, keyed selection, exact fetch, cache and persisted resolution.
#[tokio::test]
async fn dir_source_resolve_and_fetch() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let dir = tempfile::tempdir().unwrap(); let release = dir.path().join("source/demo/robot/1.2.3");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(release.join("model.glb"), b"local model").unwrap();
        std::fs::write(release.join("release.json"), serde_json::to_vec(&json!({
            "version": "1.2.3", "kind": "model", "deprecated": false,
            "artifacts": {"primary": {"file": "model.glb", "contentHash": content_hash(b"local model"), "size": 11}}
        })).unwrap()).unwrap();
        let cache = Cache::new(dir.path().join("assets")).unwrap();
        let resolver = Resolver::new(DirSource::new(dir.path().join("source")), cache.clone());
        let pin = resolver.resolve("@demo/robot", "^1.0.0", Kind::MODEL, "primary").await.unwrap();
        assert_eq!(cache.last_pin("@demo/robot").unwrap(), Some(pin.clone()));
        let fetched = resolver.fetch(&pin).await.unwrap();
        assert_eq!(fetched.bytes, b"local model"); assert_eq!(fetched.resolved, Some(pin.clone()));
        std::fs::remove_dir_all(dir.path().join("source")).unwrap();
        assert_eq!(resolver.fetch(&pin).await.unwrap().bytes, b"local model");
        assert_eq!(std::fs::read(cache.public_path(&pin.content_hash).unwrap()).unwrap(), b"local model");
    }).await.expect("test deadline");
}
