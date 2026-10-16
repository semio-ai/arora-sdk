#![cfg(feature = "build")]
use arora_index::{build, content_hash, DirSource};
use serde_json::json;
use std::collections::BTreeMap;

/// Connects manifest pins to a local source and Cargo output, then rebuilds offline from the blob.
#[test]
fn build_models_through_dir_source() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("source/demo/robot/1.2.3");
    std::fs::create_dir_all(&release).unwrap();
    std::fs::write(release.join("model.glb"), b"models").unwrap();
    let hash = content_hash(b"models");
    std::fs::write(
        release.join("release.json"),
        serde_json::to_vec(&json!({
            "version": "1.2.3", "kind": "model", "deprecated": false,
            "artifacts": {"primary": {"file": "model.glb", "contentHash": hash}}
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), format!("[package.metadata.arora.models.robot-model]\nscoped_name = '@demo/robot'\nversion = '1.2.3'\ncontent_hash = '{hash}'\n")).unwrap();
    let mut env = BTreeMap::from([
        (
            "CARGO_MANIFEST_DIR",
            dir.path().to_str().unwrap().to_owned(),
        ),
        (
            "ARORA_ASSETS_CACHE_DIR",
            dir.path().join("assets").to_str().unwrap().to_owned(),
        ),
    ]);
    let source = DirSource::new(dir.path().join("source"));
    let mut lines = Vec::new();
    build::models_with_env(
        &source,
        |key| env.get(key).cloned(),
        |line| lines.push(line.to_owned()),
    )
    .unwrap();
    let path = lines
        .iter()
        .find_map(|line| line.strip_prefix("cargo:rustc-env=ARORA_MODEL_ROBOT_MODEL="))
        .unwrap();
    assert!(std::path::Path::new(path).is_absolute());
    assert_eq!(std::fs::read(path).unwrap(), b"models");
    assert!(lines.contains(&format!("cargo:rerun-if-changed={path}")));
    assert!(lines.contains(&"cargo:rerun-if-changed=Cargo.toml".into()));
    for name in [
        "CARGO_MANIFEST_DIR",
        "ARORA_ASSETS_CACHE_DIR",
        "CARGO_HOME",
        "HOME",
        "ARORA_ASSETS_OFFLINE",
    ] {
        assert!(lines.contains(&format!("cargo:rerun-if-env-changed={name}")));
    }
    std::fs::remove_dir_all(source.directory()).unwrap();
    env.insert("ARORA_ASSETS_OFFLINE", "1".into());
    build::models_with_env(&source, |key| env.get(key).cloned(), |_| {}).unwrap();
}
