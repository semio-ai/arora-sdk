use super::*;
use crate::test_support::bounded;
use serde_json::json;

fn fixture(root: &Path) -> PathBuf {
    let dir = root.join("test/model/1.0.0");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("artifact"), b"bytes").unwrap();
    fs::write(dir.join("release.json"), serde_json::to_vec(&json!({
        "version": "1.0.0", "kind": "model", "deprecated": false,
        "artifacts": {"primary": {"file": "artifact", "contentHash": content_hash(b"bytes"), "size": 5}}
    })).unwrap()).unwrap();
    dir
}

/// The local source bounds allocation, verifies bytes and distinguishes absence from malformed metadata.
#[tokio::test]
async fn local_errors_and_caps() {
    bounded(async {
        let root = tempfile::tempdir().unwrap();
        let dir = fixture(root.path());
        let source = DirSource::new(root.path());
        fs::write(root.path().join("test/model/README"), b"not a release").unwrap();
        assert!(matches!(
            source
                .releases("@missing/model", &Kind::MODEL, SourceOptions::default())
                .await,
            Err(SourceError::NotFound(_))
        ));
        assert!(source
            .releases("@test/model", &Kind::FACE, SourceOptions::default())
            .await
            .unwrap()
            .is_empty());
        let release = source
            .releases("@test/model", &Kind::MODEL, SourceOptions::default())
            .await
            .unwrap()
            .remove(0);
        assert_eq!(release.artifacts["primary"].size, Some(5));
        let options = SourceOptions {
            storage_limit: 4,
            ..Default::default()
        };
        assert!(matches!(
            source.fetch(&release, "primary", options).await,
            Err(SourceError::TooLarge { limit: 4 })
        ));
        fs::write(dir.join("artifact"), b"wrong").unwrap();
        assert!(matches!(
            source
                .fetch(&release, "primary", SourceOptions::default())
                .await,
            Err(SourceError::Malformed(_))
        ));
        fs::write(dir.join("release.json"), b"{bad").unwrap();
        assert!(matches!(
            source
                .releases("@test/model", &Kind::MODEL, SourceOptions::default())
                .await,
            Err(SourceError::Malformed(_))
        ));
        fs::remove_file(dir.join("release.json")).unwrap();
        assert!(matches!(
            source
                .releases("@test/model", &Kind::MODEL, SourceOptions::default())
                .await,
            Err(SourceError::Malformed(_))
        ));
    })
    .await;
}

/// Files cannot escape their release, and stale source metadata cannot redirect a fetch.
#[tokio::test]
async fn paths_and_changed_metadata_are_rejected() {
    bounded(async {
        for variant in 0..9 {
            let root = tempfile::tempdir().unwrap();
            let dir = fixture(root.path());
            let source = DirSource::new(root.path());
            let mut release = source
                .releases("@test/model", &Kind::MODEL, SourceOptions::default())
                .await
                .unwrap()
                .remove(0);
            let path = dir.join("release.json");
            let mut document: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            match variant {
                0 => {
                    document["artifacts"]["primary"]["file"] = json!("../outside");
                    fs::write(dir.parent().unwrap().join("outside"), b"bytes").unwrap();
                }
                1 => document["version"] = json!("2.0.0"),
                2 => document["kind"] = json!("face"),
                3 => document["private"] = json!(true),
                4 => {
                    document["artifacts"]["primary"]["contentHash"] = json!(content_hash(b"other"))
                }
                5 => {
                    document["artifacts"] = json!({});
                }
                6 => release.release_id = "../1.0.0".into(),
                7 => release.scoped_name = "@../model".into(),
                _ => {
                    let nested = dir.join("nested");
                    fs::create_dir(&nested).unwrap();
                    fs::copy(dir.join("artifact"), nested.join("artifact")).unwrap();
                    fs::copy(dir.join("release.json"), nested.join("release.json")).unwrap();
                    release.release_id = "1.0.0/nested".into();
                }
            }
            fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
            let error = source
                .fetch(&release, "primary", SourceOptions::default())
                .await
                .unwrap_err();
            let detail = error.to_string();
            let expected = match variant {
                0 => "relative path",
                1..=3 => "release metadata changed",
                4 => "artifact metadata changed",
                5 => "artifact primary",
                6 | 8 => "invalid directory release id",
                _ => "invalid scoped name",
            };
            assert!(detail.contains(expected), "variant {variant}: {detail}");
            if variant == 1 {
                assert!(matches!(
                    source
                        .releases("@test/model", &Kind::MODEL, SourceOptions::default())
                        .await,
                    Err(SourceError::Malformed(_))
                ));
            }
        }
    })
    .await;
}

/// Canonical containment also rejects a relative symlink that points outside the release.
#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_escape() {
    bounded(async {
        let root = tempfile::tempdir().unwrap();
        let dir = fixture(root.path());
        let source = DirSource::new(root.path());
        let release = source
            .releases("@test/model", &Kind::MODEL, SourceOptions::default())
            .await
            .unwrap()
            .remove(0);
        let outside = root.path().join("outside");
        fs::write(&outside, b"bytes").unwrap();
        fs::remove_file(dir.join("artifact")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("artifact")).unwrap();
        assert!(matches!(
            source
                .fetch(&release, "primary", SourceOptions::default())
                .await,
            Err(SourceError::Malformed(_))
        ));
    })
    .await;
}
