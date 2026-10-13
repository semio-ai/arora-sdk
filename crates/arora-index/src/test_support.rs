#![allow(dead_code)]
use crate::{content_hash, AssetType, ResolvedRef};
use serde_json::{json, Value};
pub(crate) const NAME: &str = "@test/old";
pub(crate) const CURRENT: &str = "@test/current";
pub(crate) const BYTES: &[u8] = b"fixture model bytes";
pub(crate) const OBJECT: &str = "publishedAssets/asset/releases/release/artifacts/primary";
pub(crate) const ROOT: &str = "/v1/projects/demo-test/databases/(default)/documents";

pub(crate) fn asset(private: bool, asset_type: &str) -> Value {
    json!({"name": "publishedAssets/asset", "fields": {
        "scopedName": {"stringValue": CURRENT}, "assetType": {"stringValue": asset_type},
        "publicRole": if private { json!({"nullValue": null}) } else { json!({"stringValue": "viewer"}) },
        "latestVersion": {"stringValue": "99.0.0"}, "deprecated": {"booleanValue": false}
    }})
}

pub(crate) fn release(id: &str, version: &str) -> Value {
    json!({"name": format!("publishedAssets/asset/releases/{id}"), "fields": {
        "version": {"stringValue": version}, "family": {"stringValue": "test"},
        "artifacts": {"mapValue": {"fields": {"primary": {"mapValue": {"fields": {
            "contentHash": {"stringValue": content_hash(BYTES)},
            "storage": {"mapValue": {"fields": {"fileBucket": {"stringValue": OBJECT}}}},
            "role": {"stringValue": "model"}, "minRole": {"stringValue": "viewer"}
        }}}}}}
    }})
}

pub(crate) fn pin() -> ResolvedRef {
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

pub(crate) fn model_pin(private: bool) -> ResolvedRef {
    ResolvedRef {
        scoped_name: CURRENT.into(),
        published_asset_id: "asset".into(),
        version: "1.2.3".into(),
        release_id: "release".into(),
        artifact_key: "primary".into(),
        content_hash: content_hash(BYTES),
        storage_path: Some(OBJECT.into()),
        asset_type: AssetType::Model,
        family: Some("test".into()),
        private,
        resolved_at: 123,
    }
}

pub(crate) struct Warnings(pub(crate) std::sync::Mutex<Vec<String>>);
pub(crate) static HASH_THREADS: std::sync::Mutex<Vec<(String, std::thread::ThreadId)>> =
    std::sync::Mutex::new(Vec::new());
pub(crate) static WARNINGS: Warnings = Warnings(std::sync::Mutex::new(Vec::new()));
impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "arora-assets"
    }
    fn log(&self, record: &log::Record<'_>) {
        if record.level() <= log::Level::Warn {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record.args().to_string());
        }
        if record.level() == log::Level::Trace && record.target() == "arora-assets" {
            lock(&HASH_THREADS).push((record.args().to_string(), std::thread::current().id()));
        }
    }
    fn flush(&self) {}
}
/// Locks a shared test record even after a test panicked while holding it, so
/// one failing test does not make every later one fail too.
pub(crate) fn lock<T>(mutex: &'static std::sync::Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether file modes restrict this process. They do not restrict root, so a
/// test that needs a read-only file or directory skips when they do not apply.
#[cfg(unix)]
pub(crate) fn file_modes_apply() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let applies = std::fs::write(dir.path().join("probe"), b"").is_err();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    applies
}

pub(crate) fn capture_warnings() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        log::set_logger(&WARNINGS).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
}

pub(crate) fn assert_hash_off_executor(size: usize, since: usize) {
    let messages = lock(&HASH_THREADS);
    let threads: Vec<_> = messages[since..]
        .iter()
        .filter(|(message, _)| message == &format!("computing SHA-256 for {size} bytes"))
        .map(|(_, thread)| *thread)
        .collect();
    assert!(!threads.is_empty(), "hashing must be observed");
    assert!(
        threads
            .iter()
            .all(|thread| *thread != std::thread::current().id()),
        "hashing must run off the async executor"
    );
}

pub(crate) async fn bounded<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(std::time::Duration::from_secs(2), future)
        .await
        .expect("test deadline")
}
