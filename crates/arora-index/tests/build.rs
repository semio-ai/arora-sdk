mod support;
use arora_index::{build, content_hash, Cache};
use std::collections::BTreeMap;
use support::*;
use wiremock::MockServer;

fn environment(
    dir: &std::path::Path,
    server: Option<&MockServer>,
    version: &str,
    hash: &str,
) -> BTreeMap<String, String> {
    let manifest = format!("[package]\nname = 'fixture'\nversion = '0.1.0'\n[package.metadata.arora.models]\nrobot-model = {{ scoped_name = '{NAME}', version = '{version}', content_hash = '{hash}' }}\n");
    std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    let mut env = BTreeMap::from([
        ("CARGO_MANIFEST_DIR".into(), dir.to_str().unwrap().into()),
        (
            "ARORA_ASSETS_CACHE_DIR".into(),
            dir.join("cache").to_str().unwrap().into(),
        ),
    ]);
    if let Some(server) = server {
        env.insert("FIREBASE_PROJECT_ID".into(), "demo-test".into());
        env.insert("FIREBASE_STORAGE_BUCKET".into(), "demo-test.bucket".into());
        for key in [
            "FIREBASE_FIRESTORE_EMULATOR_HOST",
            "FIREBASE_STORAGE_EMULATOR_HOST",
        ] {
            env.insert(
                key.into(),
                server.uri().strip_prefix("http://").unwrap().into(),
            );
        }
    }
    env
}

#[tokio::test(flavor = "multi_thread")]
async fn pinned_download_is_exposed_and_then_works_offline_without_configuration() {
    let server = MockServer::start().await;
    chain(&server, false).await;
    storage(&server, BYTES, 200).await;
    let dir = tempfile::tempdir().unwrap();
    let env = environment(dir.path(), Some(&server), "1.2.3", &content_hash(BYTES));
    let lines = tokio::task::spawn_blocking(move || {
        let mut lines = Vec::new();
        build::models_with(
            |key| env.get(key).cloned(),
            |line| lines.push(line.to_owned()),
        )
        .unwrap();
        lines
    })
    .await
    .unwrap();
    let directive = lines
        .iter()
        .find(|l| l.starts_with("cargo:rustc-env=ARORA_MODEL_ROBOT_MODEL="))
        .unwrap();
    let path = directive
        .strip_prefix("cargo:rustc-env=ARORA_MODEL_ROBOT_MODEL=")
        .unwrap();
    assert!(std::path::Path::new(path).is_absolute());
    assert!(lines.contains(&format!("cargo:rerun-if-changed={path}")));
    assert_eq!(std::fs::read(path).unwrap(), BYTES);
    for var in [
        "CARGO_MANIFEST_DIR",
        "ARORA_ASSETS_CACHE_DIR",
        "CARGO_HOME",
        "HOME",
        "ARORA_ASSETS_OFFLINE",
        "FIREBASE_PROJECT_ID",
        "FIREBASE_STORAGE_BUCKET",
        "FIREBASE_FIRESTORE_EMULATOR_HOST",
        "FIREBASE_STORAGE_EMULATOR_HOST",
    ] {
        assert!(lines.contains(&format!("cargo:rerun-if-env-changed={var}")));
    }
    assert!(lines.contains(&"cargo:rerun-if-changed=Cargo.toml".into()));
    let before = server.received_requests().await.unwrap().len();
    let mut env = environment(dir.path(), None, "1.2.3", &content_hash(BYTES));
    env.insert("ARORA_ASSETS_OFFLINE".into(), "1".into());
    tokio::task::spawn_blocking(move || {
        build::models_with(|key| env.get(key).cloned(), |_| {}).unwrap()
    })
    .await
    .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[test]
fn invalid_pins_offline_misses_and_missing_configuration_are_actionable() {
    let dir = tempfile::tempdir().unwrap();
    for version in ["^1.2.3", "~1.2.3", "*", "v1.2.3", "1.2.3-beta", "0.9.9"] {
        let env = environment(dir.path(), None, version, &content_hash(BYTES));
        let error = build::models_with(|key| env.get(key).cloned(), |_| {}).unwrap_err();
        assert!(error.to_string().contains("robot-model"));
        assert!(error.to_string().contains("exact canonical version"));
    }
    let mut env = environment(dir.path(), None, "1.2.3", &content_hash(BYTES));
    env.insert("ARORA_ASSETS_OFFLINE".into(), "1".into());
    let err = build::models_with(|key| env.get(key).cloned(), |_| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("robot-model") && err.contains("fill the cache"));
    env.remove("ARORA_ASSETS_OFFLINE");
    assert!(build::models_with(|key| env.get(key).cloned(), |_| {})
        .unwrap_err()
        .to_string()
        .contains("FIREBASE_PROJECT_ID"));
    env.insert("FIREBASE_PROJECT_ID".into(), "demo-test".into());
    assert!(build::models_with(|key| env.get(key).cloned(), |_| {})
        .unwrap_err()
        .to_string()
        .contains("FIREBASE_STORAGE_BUCKET"));
}

#[tokio::test(flavor = "multi_thread")]
async fn private_assets_and_mismatched_pins_are_refused_before_storage() {
    for private in [true, false] {
        let server = MockServer::start().await;
        chain(&server, private).await;
        let dir = tempfile::tempdir().unwrap();
        let env = environment(
            dir.path(),
            Some(&server),
            "1.2.3",
            &content_hash(b"different pin"),
        );
        let err = tokio::task::spawn_blocking(move || {
            build::models_with(|key| env.get(key).cloned(), |_| {}).unwrap_err()
        })
        .await
        .unwrap();
        if private {
            assert!(err.to_string().contains("must be public"));
        } else {
            assert!(err.to_string().contains("SHA-256 mismatch"));
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
        assert_eq!(
            std::fs::read_dir(dir.path().join("cache/blobs"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn default_cache_paths_and_sealed_only_miss() {
    for cargo_home in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let mut env = environment(dir.path(), None, "1.2.3", &content_hash(BYTES));
        env.remove("ARORA_ASSETS_CACHE_DIR");
        env.insert(
            if cargo_home { "CARGO_HOME" } else { "HOME" }.into(),
            dir.path().to_str().unwrap().into(),
        );
        env.insert("ARORA_ASSETS_OFFLINE".into(), "1".into());
        let path = dir.path().join(if cargo_home {
            "arora-assets"
        } else {
            ".cargo/arora-assets"
        });
        let cache = Cache::new(path).unwrap();
        cache.store(&content_hash(BYTES), BYTES, true).unwrap();
        assert!(build::models_with(|key| env.get(key).cloned(), |_| {})
            .unwrap_err()
            .to_string()
            .contains("offline cache miss"));
        cache.store(&content_hash(BYTES), BYTES, false).unwrap();
        build::models_with(|key| env.get(key).cloned(), |_| {}).unwrap();
    }
}

#[test]
fn online_cache_hit_without_firebase_and_empty_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = environment(dir.path(), None, "1.2.3", &content_hash(BYTES));
    env.insert("ARORA_ASSETS_CACHE_DIR".into(), String::new());
    env.insert("CARGO_HOME".into(), String::new());
    env.insert("ARORA_ASSETS_OFFLINE".into(), String::new());
    env.insert("HOME".into(), dir.path().to_str().unwrap().into());
    let cache = Cache::new(dir.path().join(".cargo/arora-assets")).unwrap();
    cache.store(&content_hash(BYTES), BYTES, false).unwrap();
    let mut lines = Vec::new();
    build::models_with(
        |key| env.get(key).cloned(),
        |line| lines.push(line.to_owned()),
    )
    .unwrap();
    let path = cache
        .public_path(&content_hash(BYTES))
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(lines.contains(&format!("cargo:rerun-if-changed={}", path.display())));
}
