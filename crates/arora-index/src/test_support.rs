#![allow(dead_code)]
use crate::{
    content_hash, ArtifactRef, Kind, Release, ResolvedRef, Source, SourceError, SourceOptions,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
pub(crate) const NAME: &str = "@test/old";
pub(crate) const CURRENT: &str = "@test/current";
pub(crate) const BYTES: &[u8] = b"fixture model bytes";
pub(crate) fn pin() -> ResolvedRef {
    model_pin(true)
}
pub(crate) fn model_pin(private: bool) -> ResolvedRef {
    ResolvedRef {
        scoped_name: CURRENT.into(),
        version: "1.2.3".into(),
        release_id: "release".into(),
        artifact_key: "primary".into(),
        content_hash: content_hash(BYTES),
        family: None,
        private,
        resolved_at: 123,
        asset_type: Kind::MODEL,
    }
}
pub(crate) fn release() -> Release {
    Release {
        scoped_name: CURRENT.into(),
        version: "1.2.3".into(),
        release_id: "release".into(),
        kind: Kind::MODEL,
        deprecated: false,
        private: true,
        family: None,
        artifacts: BTreeMap::from([(
            "primary".into(),
            ArtifactRef {
                content_hash: content_hash(BYTES),
                size: None,
            },
        )]),
    }
}
pub(crate) struct FakeSource {
    pub releases: Vec<Release>,
    pub error: Option<SourceError>,
    pub bytes: Vec<u8>,
    pub downloads: AtomicUsize,
    pub listings: AtomicUsize,
    pub options: Mutex<Vec<SourceOptions>>,
    pub keys: Mutex<Vec<String>>,
}
impl Default for FakeSource {
    fn default() -> Self {
        Self {
            releases: vec![release()],
            error: None,
            bytes: BYTES.to_vec(),
            downloads: AtomicUsize::new(0),
            listings: AtomicUsize::new(0),
            options: Mutex::new(Vec::new()),
            keys: Mutex::new(Vec::new()),
        }
    }
}
#[async_trait]
impl Source for FakeSource {
    async fn releases(
        &self,
        _: &str,
        _: &Kind,
        options: SourceOptions,
    ) -> Result<Vec<Release>, SourceError> {
        self.listings.fetch_add(1, Ordering::SeqCst);
        self.options.lock().unwrap().push(options);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(self.releases.clone())
    }
    async fn fetch(
        &self,
        _: &Release,
        key: &str,
        options: SourceOptions,
    ) -> Result<Vec<u8>, SourceError> {
        self.keys.lock().unwrap().push(key.into());
        self.downloads.fetch_add(1, Ordering::SeqCst);
        self.options.lock().unwrap().push(options);
        Ok(self.bytes.clone())
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
