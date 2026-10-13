use super::*;
use crate::test_support::*;
use crate::{content_hash, AssetType};
use serde_json::{json, Value};
use std::time::Duration;

/// Asserts verified hits corruption and visibility are separate.
#[test]
fn verified_hits_corruption_and_visibility_are_separate() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let hash = content_hash(BYTES);
    assert!(cache.read(&hash, false).unwrap().is_none());
    cache.store(&hash, BYTES, false).unwrap();
    assert_eq!(cache.read(&hash, false).unwrap().unwrap(), BYTES);
    assert!(cache.read(&hash, true).unwrap().is_none());
    assert_eq!(cache.read(&hash, false).unwrap().unwrap(), BYTES);
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

/// Asserts private bytes stay sealed even after reading.
#[test]
fn private_bytes_stay_sealed_even_after_reading() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let pin = model_pin(true);
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

/// Asserts changed key makes private blob a miss.
#[test]
fn changed_key_makes_private_blob_a_miss() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let hash = content_hash(BYTES);
    cache.store(&hash, BYTES, true).unwrap();
    assert_eq!(cache.read(&hash, true).unwrap().unwrap(), BYTES);
    fs::write(dir.path().join("key"), [0u8; 32]).unwrap();
    assert!(cache.read(&hash, true).unwrap().is_none());
    assert_eq!(fs::read_dir(dir.path().join("blobs")).unwrap().count(), 0);
}

/// Asserts pins use requested names and studio field names.
#[test]
fn pins_use_requested_names_and_studio_field_names() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let pin = model_pin(false);
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
            "assetType",
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
    offline_pin.storage_path = None;
    assert_eq!(cache.last_pin(NAME).unwrap(), Some(offline_pin));
    assert_eq!(serde_json::to_value(pin).unwrap()["storagePath"], OBJECT);
}

/// Asserts decrypted hash is verified.
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

/// Asserts pins recover valid entries and discard bad files.
#[test]
fn pins_recover_valid_entries_and_discard_bad_files() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let path = dir.path().join("resolved.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({NAME: pin(), "@test/bad": {"version": 123}})).unwrap(),
    )
    .unwrap();
    assert_eq!(cache.last_pin(NAME).unwrap().unwrap().storage_path, None);
    assert!(cache.last_pin("@test/bad").unwrap().is_none());
    cache.save_pin("@test/new", &pin()).unwrap();
    let map: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(map.get(NAME).is_some() && map.get("@test/new").is_some());
    assert_eq!(map["@test/bad"], json!({"version": 123}));
    for bytes in [b"{truncated".as_slice(), b"[]"] {
        fs::write(&path, bytes).unwrap();
        assert!(cache.last_pin(NAME).unwrap().is_none());
        cache.save_pin(NAME, &pin()).unwrap();
        assert!(cache.last_pin(NAME).unwrap().is_some());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(cache.last_pin(NAME).unwrap().is_none());
}

/// Asserts invalid key is replaced and old ciphertext becomes a miss.
#[test]
fn invalid_key_is_replaced_and_old_ciphertext_becomes_a_miss() {
    capture_warnings();
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let hash = content_hash(BYTES);
    cache.store(&hash, BYTES, true).unwrap();
    assert_eq!(cache.read(&hash, true).unwrap().unwrap(), BYTES);
    fs::write(dir.path().join("key"), b"partial").unwrap();
    let other = content_hash(b"new bytes");
    cache.store(&other, b"new bytes", true).unwrap();
    assert_eq!(fs::read(dir.path().join("key")).unwrap().len(), 32);
    assert!(cache.read(&hash, true).unwrap().is_none());
    assert_eq!(cache.read(&other, true).unwrap().unwrap(), b"new bytes");
    assert!(lock(&WARNINGS.0)
        .iter()
        .any(|m| m.contains("replacing key with invalid length")
            && m.contains(dir.path().to_str().unwrap())));
}

/// Asserts verified public private and pin reads need no writable directory or lock.
#[cfg(unix)]
#[test]
fn verified_public_private_and_pin_reads_need_no_writable_directory_or_lock() {
    if !file_modes_apply() {
        eprintln!("skipped: file modes do not restrict this user");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let pin = pin();
    cache.store(&pin.content_hash, BYTES, false).unwrap();
    cache.store(&pin.content_hash, BYTES, true).unwrap();
    cache.save_pin(NAME, &pin).unwrap();
    fs::remove_file(dir.path().join(".lock")).unwrap();
    fs::create_dir(dir.path().join(".lock")).unwrap();
    let mut paths = vec![dir.path().to_owned(), dir.path().join("blobs")];
    paths.extend(
        fs::read_dir(dir.path().join("blobs"))
            .unwrap()
            .map(|e| e.unwrap().path()),
    );
    paths.extend([dir.path().join("key"), dir.path().join("resolved.json")]);
    for path in &paths {
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if path.is_dir() { 0o500 } else { 0o400 }),
        )
        .unwrap();
    }
    let public = cache.read(&pin.content_hash, false);
    let private = cache.read(&pin.content_hash, true);
    let saved = cache.last_pin(NAME);
    for path in &paths {
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if path.is_dir() { 0o700 } else { 0o600 }),
        )
        .unwrap();
    }
    assert_eq!(public.unwrap().unwrap(), BYTES);
    assert_eq!(private.unwrap().unwrap(), BYTES);
    assert!(saved.unwrap().is_some());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4);
    assert_eq!(fs::read_dir(dir.path().join("blobs")).unwrap().count(), 2);
}

/// Asserts a busy cache lock does not block the async executor.
#[tokio::test(flavor = "current_thread")]
async fn a_busy_cache_lock_does_not_block_the_async_executor() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.path().join(".lock"))
            .unwrap();
        lock.lock().unwrap();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        // The timeout only releases a regressed synchronous writer so the test cannot hang.
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let executor_ran = release_rx.recv_timeout(Duration::from_millis(500)).is_ok();
            drop(lock);
            done_tx.send(executor_ran).unwrap();
        });
        let asset = crate::FetchedAsset {
            content_hash: content_hash(BYTES),
            bytes: BYTES.to_vec(),
            resolved: None,
        };
        let (stored, ()) = tokio::join!(biased;
            cache.store_async(asset),
            async { let _ = release_tx.send(()); },
        );
        stored.unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(750)).unwrap(),
            "the executor must run while the cache writer waits for its lock"
        );
    })
    .await;
}

/// Asserts corrupt read only blobs are misses even when cleanup fails.
#[cfg(unix)]
#[test]
fn corrupt_read_only_blobs_are_misses_even_when_cleanup_fails() {
    if !file_modes_apply() {
        eprintln!("skipped: file modes do not restrict this user");
        return;
    }
    capture_warnings();
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let hash = content_hash(BYTES);
    cache.store(&hash, BYTES, true).unwrap();
    cache.store(&hash, BYTES, false).unwrap();
    fs::write(cache.public_path(&hash).unwrap(), b"wrong").unwrap();
    fs::write(
        dir.path().join("blobs").join(format!("{hash}.sealed")),
        b"wrong",
    )
    .unwrap();
    let blobs = dir.path().join("blobs");
    fs::set_permissions(&blobs, fs::Permissions::from_mode(0o500)).unwrap();
    let public = cache.read(&hash, false);
    let private = cache.read(&hash, true);
    fs::set_permissions(&blobs, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(public.unwrap().is_none());
    assert!(private.unwrap().is_none());
    assert!(lock(&WARNINGS.0).iter().any(
        |m| m.contains("could not remove corrupt blob") && m.contains(blobs.to_str().unwrap())
    ));
}

/// Asserts old pins without asset type are models.
#[test]
fn old_pins_without_asset_type_are_models() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let mut value = serde_json::to_value(pin()).unwrap();
    value.as_object_mut().unwrap().remove("assetType");
    fs::write(
        dir.path().join("resolved.json"),
        serde_json::to_vec(&json!({NAME: value})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        cache.last_pin(NAME).unwrap().unwrap().asset_type,
        AssetType::Model
    );
}

/// Asserts async cache helpers preserve private storage.
#[tokio::test]
async fn async_cache_helpers_preserve_private_storage() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let pin = pin();
        let asset = crate::FetchedAsset {
            content_hash: pin.content_hash.clone(),
            bytes: BYTES.to_vec(),
            resolved: Some(pin.clone()),
        };
        cache.store_async(asset).await.unwrap();
        assert_eq!(cache.fetch_async(&pin).await.unwrap().unwrap().bytes, BYTES);
        assert!(!cache.public_path(&pin.content_hash).unwrap().exists());
    })
    .await;
}

/// Asserts pin updates preserve raw entries and only remove the named blob.
#[test]
fn pin_updates_preserve_raw_entries_and_only_remove_the_named_blob() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let pin = pin();
    let path = dir.path().join("resolved.json");
    let raw = format!(
        r#"{{ "contentHash" : "{}", "unknown": 1e2, "version": false }}"#,
        pin.content_hash
    );
    fs::write(
        &path,
        format!(
            r#"{{"{NAME}": {}, "@test/raw": {raw}}}"#,
            serde_json::to_string(&pin).unwrap()
        ),
    )
    .unwrap();
    cache.store(&pin.content_hash, BYTES, true).unwrap();
    let pending = content_hash(b"stored before pin");
    cache.store(&pending, b"stored before pin", true).unwrap();
    cache.save_pin("@test/new", &pin).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains(&raw));
    cache.remove_pin("@test/new").unwrap();
    cache.remove_pin(NAME).unwrap();
    assert!(cache.last_pin(NAME).unwrap().is_none());
    assert!(fs::read_to_string(&path).unwrap().contains(&raw));
    assert_eq!(cache.read(&pin.content_hash, true).unwrap().unwrap(), BYTES);
    assert_eq!(
        cache.read(&pending, true).unwrap().unwrap(),
        b"stored before pin"
    );
    let before = fs::read(&path).unwrap();
    for absent in ["@test/missing", "@test/raw"] {
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&path).unwrap().ino()
        };
        cache.remove_pin(absent).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        }
    }
}

/// Asserts unreadable pins are never overwritten and warn.
#[cfg(unix)]
#[test]
fn unreadable_pins_are_never_overwritten_and_warn() {
    if !file_modes_apply() {
        eprintln!("skipped: file modes do not restrict this user");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    capture_warnings();
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let path = dir.path().join("resolved.json");
    cache.save_pin(NAME, &pin()).unwrap();
    let before = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let read = cache.last_pin(NAME);
    let saved = cache.save_pin("@test/new", &pin());
    let removed = cache.remove_pin(NAME);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(read.unwrap().is_none());
    saved.unwrap();
    removed.unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(lock(&WARNINGS.0)
        .iter()
        .any(|m| m.contains("could not save resolved pin") && m.contains(path.to_str().unwrap())));
    assert!(lock(&WARNINGS.0)
        .iter()
        .any(|m| m.contains("could not clear resolved pin") && m.contains(path.to_str().unwrap())));
}

/// Asserts store verification runs off the executor with shared bytes.
#[tokio::test(flavor = "current_thread")]
async fn store_verification_runs_off_the_executor_with_shared_bytes() {
    bounded(async {
        capture_warnings();
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let bytes = vec![44; 1249];
        let asset = std::sync::Arc::new(crate::FetchedAsset {
            content_hash: content_hash(&bytes),
            bytes,
            resolved: None,
        });
        let since = lock(&HASH_THREADS).len();
        cache.store_async(asset.clone()).await.unwrap();
        assert_hash_off_executor(asset.bytes.len(), since);
        assert_eq!(
            cache.read(&asset.content_hash, false).unwrap().unwrap(),
            asset.bytes
        );
    })
    .await;
}

mod allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static MIN_SIZE: Cell<usize> = const { Cell::new(0) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    struct Allocator;
    #[global_allocator]
    static ALLOCATOR: Allocator = Allocator;

    fn record(size: usize) {
        let _ = MIN_SIZE.try_with(|min| {
            if min.get() != 0 && size >= min.get() {
                let _ = COUNT.try_with(|count| count.set(count.get() + 1));
            }
        });
    }

    // Observe allocations on the polling thread; System owns every allocation.
    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            record(layout.size());
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            record(layout.size());
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            record(size);
            unsafe { System.realloc(ptr, layout, size) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub(super) async fn count_large_allocations<F: std::future::Future>(
        future: F,
        minimum: usize,
    ) -> (F::Output, usize) {
        let mut future = std::pin::pin!(future);
        COUNT.set(0);
        let result = std::future::poll_fn(|cx| {
            MIN_SIZE.set(minimum);
            let result = future.as_mut().poll(cx);
            MIN_SIZE.set(0);
            result
        })
        .await;
        (result, COUNT.get())
    }
}

/// Asserts async store does not copy the body on the executor.
#[tokio::test(flavor = "current_thread")]
async fn async_store_does_not_copy_the_body_on_the_executor() {
    bounded(async {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let bytes = vec![45; 1024 * 1024];
        let asset = std::sync::Arc::new(crate::FetchedAsset {
            content_hash: content_hash(&bytes),
            bytes,
            resolved: None,
        });
        let (result, allocations) = allocations::count_large_allocations(
            cache.store_async(asset.clone()),
            asset.bytes.len(),
        )
        .await;
        result.unwrap();
        assert_eq!(allocations, 0, "the executor must not allocate a body copy");
        assert_eq!(
            cache.read(&asset.content_hash, false).unwrap().unwrap(),
            asset.bytes
        );
    })
    .await;
}

/// Asserts key publication is complete and never clobbers a winner.
#[test]
fn key_publication_is_complete_and_never_clobbers_a_winner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key");
    let first = Key::from([1; 32]);
    let second = Key::from([2; 32]);
    let temp = private_temp(&path, &first).unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read(temp.path()).unwrap(), first.as_slice());
    drop(temp);
    assert_eq!(publish_key(&path, &first).unwrap(), first);
    assert_eq!(publish_key(&path, &second).unwrap(), first);
    assert_eq!(fs::read(path).unwrap(), first.as_slice());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// Four concurrent writers must preserve every pin and share one complete encryption key.
#[test]
fn independent_cache_handles_preserve_keys_and_all_pins() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..4 {
        let cache = cache.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut pin = pin();
            let bytes = format!("private {i}");
            pin.content_hash = content_hash(bytes.as_bytes());
            let result = cache
                .store(&pin.content_hash, bytes.as_bytes(), true)
                .and_then(|()| cache.save_pin(&format!("@test/{i}"), &pin));
            tx.send(result).unwrap();
        });
    }
    for _ in 0..4 {
        rx.recv_timeout(Duration::from_millis(750))
            .unwrap()
            .unwrap();
    }
    for i in 0..4 {
        let pin = cache.last_pin(&format!("@test/{i}")).unwrap().unwrap();
        assert_eq!(
            cache.fetch(&pin).unwrap().unwrap().bytes,
            format!("private {i}").as_bytes()
        );
    }
}

/// Only the cleared pin's own hash is eligible; shared, pending and public blobs remain.
#[test]
fn clearing_only_removes_its_own_unshared_sealed_blob() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::new(dir.path()).unwrap();
    let pin = pin();
    cache.save_pin(NAME, &pin).unwrap();
    cache.save_pin("@test/other", &pin).unwrap();
    cache.store(&pin.content_hash, BYTES, true).unwrap();
    cache.store(&pin.content_hash, BYTES, false).unwrap();
    let pending = content_hash(b"pending");
    cache.store(&pending, b"pending", true).unwrap();
    cache.remove_pin(NAME).unwrap();
    assert!(cache.read(&pin.content_hash, true).unwrap().is_some());
    cache.remove_pin("@test/other").unwrap();
    assert!(cache.read(&pin.content_hash, true).unwrap().is_none());
    assert!(cache.read(&pin.content_hash, false).unwrap().is_some());
    assert!(cache.read(&pending, true).unwrap().is_some());
}

/// Each filesystem failure warns; even failed blob deletion is advisory after pin removal.
#[test]
fn cleanup_failures_warn_without_failing() {
    capture_warnings();
    for obstacle in [".lock", "resolved.json", "blob"] {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let pin = pin();
        cache.save_pin(NAME, &pin).unwrap();
        let path = if obstacle == "blob" {
            cache.blob_path(&pin.content_hash, true).unwrap()
        } else {
            let p = dir.path().join(obstacle);
            fs::remove_file(&p).unwrap();
            p
        };
        fs::create_dir(&path).unwrap();
        cache.remove_pin(NAME).unwrap();
        assert!(lock(&WARNINGS.0)
            .iter()
            .any(|m| m.contains("could not clear resolved pin")
                && m.contains(path.to_str().unwrap())));
    }
}

/// A failed initial verification cannot delete a valid replacement published before the recheck.
#[test]
fn corrupt_read_rechecks_a_concurrent_replacement_under_the_lock() {
    for private in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path()).unwrap();
        let hash = content_hash(BYTES);
        cache.store(&hash, BYTES, private).unwrap();
        let path = cache.blob_path(&hash, private).unwrap();
        let replacement = dir.path().join("replacement");
        fs::rename(&path, &replacement).unwrap();
        fs::write(&path, b"bad").unwrap();
        assert!(cache
            .verified_bytes(&hash, private, fs::read(&path).unwrap())
            .unwrap()
            .is_none());
        let lock = cache.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = cache.clone();
        let read_path = path.clone();
        let read_hash = hash.clone();
        std::thread::spawn(move || {
            let result = reader.remove_corrupt(&read_path, &read_hash, private);
            tx.send(result).unwrap();
        });
        let early = rx.recv_timeout(Duration::from_millis(30));
        fs::rename(replacement, &path).unwrap();
        drop(lock);
        assert!(early.is_err(), "cleanup must wait for the lock");
        rx.recv_timeout(Duration::from_millis(750))
            .unwrap()
            .unwrap();
        assert_eq!(cache.read(&hash, private).unwrap().unwrap(), BYTES);
    }
}
