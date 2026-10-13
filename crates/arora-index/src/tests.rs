use super::*;
use crate::test_support::*;
use std::fs;

/// Asserts hashes match known answers.
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

/// Asserts local file returns bytes hash and no release.
#[tokio::test]
async fn local_file_returns_bytes_hash_and_no_release() {
    bounded(async {
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
    })
    .await;
}

/// Asserts local hashing runs off the executor.
#[tokio::test(flavor = "current_thread")]
async fn local_hashing_runs_off_the_executor() {
    bounded(async {
        capture_warnings();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model");
        let bytes = vec![42; 1231];
        fs::write(&path, &bytes).unwrap();
        let since = lock(&HASH_THREADS).len();
        let fetched = fetch_local(path).await.unwrap();
        assert_eq!(fetched.bytes, bytes);
        assert_hash_off_executor(bytes.len(), since);
    })
    .await;
}
