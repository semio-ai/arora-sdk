mod fake_studio;
use arora_index::{build, content_hash};
use fake_studio::*;
use std::collections::BTreeMap;
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

/// Exercises manifest pins, public downloads, Cargo directives and an offline rebuild.
#[tokio::test(flavor = "multi_thread")]
async fn pinned_download_is_exposed_and_then_works_offline_without_configuration() {
    bounded(async {
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
    })
    .await;
}
