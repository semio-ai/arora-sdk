use super::*;
use crate::content_hash;
use crate::test_support::*;
use std::collections::BTreeMap;

fn environment(dir: &std::path::Path, version: &str, hash: &str) -> BTreeMap<String, String> {
    let manifest = format!("[package]\nname = 'fixture'\nversion = '0.1.0'\n[package.metadata.arora.models]\nrobot-model = {{ scoped_name = '{NAME}', version = '{version}', content_hash = '{hash}' }}\n");
    std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    BTreeMap::from([
        ("CARGO_MANIFEST_DIR".into(), dir.to_str().unwrap().into()),
        (
            "ARORA_ASSETS_CACHE_DIR".into(),
            dir.join("cache").to_str().unwrap().into(),
        ),
    ])
}

/// Offline misses name the entry and remedy; online misses name the missing Firebase variable.
#[test]
fn offline_misses_and_missing_configuration_are_actionable() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = environment(dir.path(), "1.2.3", &content_hash(BYTES));
    env.insert("ARORA_ASSETS_OFFLINE".into(), "1".into());
    let err = models_with(|key| env.get(key).cloned(), |_| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("robot-model") && err.contains("fill the cache"));
    env.remove("ARORA_ASSETS_OFFLINE");
    assert!(models_with(|key| env.get(key).cloned(), |_| {})
        .unwrap_err()
        .to_string()
        .contains("FIREBASE_PROJECT_ID"));
    env.insert("FIREBASE_PROJECT_ID".into(), "demo-test".into());
    assert!(models_with(|key| env.get(key).cloned(), |_| {})
        .unwrap_err()
        .to_string()
        .contains("FIREBASE_STORAGE_BUCKET"));
}

/// Asserts default cache paths and sealed only miss.
#[test]
fn default_cache_paths_and_sealed_only_miss() {
    for cargo_home in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let mut env = environment(dir.path(), "1.2.3", &content_hash(BYTES));
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
        assert!(models_with(|key| env.get(key).cloned(), |_| {})
            .unwrap_err()
            .to_string()
            .contains("offline cache miss"));
        cache.store(&content_hash(BYTES), BYTES, false).unwrap();
        models_with(|key| env.get(key).cloned(), |_| {}).unwrap();
    }
}

/// Asserts online cache hit without firebase and empty overrides.
#[test]
fn online_cache_hit_without_firebase_and_empty_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = environment(dir.path(), "1.2.3", &content_hash(BYTES));
    env.insert("ARORA_ASSETS_CACHE_DIR".into(), String::new());
    env.insert("CARGO_HOME".into(), String::new());
    env.insert("ARORA_ASSETS_OFFLINE".into(), String::new());
    env.insert("HOME".into(), dir.path().to_str().unwrap().into());
    let cache = Cache::new(dir.path().join(".cargo/arora-assets")).unwrap();
    cache.store(&content_hash(BYTES), BYTES, false).unwrap();
    let mut lines = Vec::new();
    models_with(
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

/// All entries are validated together before the caller can look up or expose any cached blob.
#[test]
fn every_pin_is_validated_before_any_cache_hit() {
    for (name, version, hash, detail) in [
        (
            "bad/name",
            "1.2.3",
            content_hash(BYTES),
            "invalid scoped name",
        ),
        (
            NAME,
            "^1.2.3",
            content_hash(BYTES),
            "exact canonical version",
        ),
        (
            NAME,
            "v1.2.3",
            content_hash(BYTES),
            "exact canonical version",
        ),
        (NAME, "1.2.3", "BAD".into(), "64 lowercase hex"),
    ] {
        let entries:toml::Table=toml::from_str(&format!("first = {{ scoped_name = '{NAME}', version = '1.2.3', content_hash = '{}' }}\nz-invalid = {{ scoped_name = '{name}', version = '{version}', content_hash = '{hash}' }}",content_hash(BYTES))).unwrap();
        let error = validate_pins(&entries).err().unwrap().to_string();
        assert!(error.contains("z-invalid") && error.contains(detail));
    }
    for version in ["~1.2.3", "*", "1.2.3-beta", "0.9.9"] {
        let entries: toml::Table = toml::from_str(&format!(
            "robot = {{ scoped_name = '{NAME}', version = '{version}', content_hash = '{}' }}",
            content_hash(BYTES)
        ))
        .unwrap();
        assert!(validate_pins(&entries)
            .err()
            .unwrap()
            .to_string()
            .contains("exact canonical version"));
    }
    for name in ["UPPER", "bad.name", ""] {
        let mut entries = toml::Table::new();
        entries.insert(name.into(), toml::Value::Table(toml::Table::new()));
        assert!(validate_pins(&entries).is_err());
    }
    let entries:toml::Table=toml::from_str(&format!("a-b = {{scoped_name='{NAME}',version='1.2.3',content_hash='{}'}}\na_b = {{scoped_name='{NAME}',version='1.2.3',content_hash='{}'}}",content_hash(BYTES),content_hash(BYTES))).unwrap();
    assert!(validate_pins(&entries)
        .err()
        .unwrap()
        .to_string()
        .contains("duplicate output"));
}

/// Asserts empty environment values are unset and cache order is explicit.
#[test]
fn empty_environment_values_are_unset_and_cache_order_is_explicit() {
    assert_eq!(env_value(Some(String::new())), None);
    let cfg = crate::StudioConfig::from_lookup(|key| {
        Some(
            match key {
                "FIREBASE_PROJECT_ID" => "demo-test",
                "FIREBASE_STORAGE_BUCKET" => "fixture",
                _ => "",
            }
            .into(),
        )
    })
    .unwrap();
    assert_eq!(cfg.firestore_url, "https://firestore.googleapis.com");
    assert_eq!(cfg.storage_url, "https://firebasestorage.googleapis.com");
    for missing in ["FIREBASE_PROJECT_ID", "FIREBASE_STORAGE_BUCKET"] {
        assert!(crate::StudioConfig::from_lookup(|key| Some(
            if key == missing { "" } else { "fixture" }.into()
        ))
        .unwrap_err()
        .to_string()
        .contains(missing));
    }
    for (explicit, cargo, expected) in [
        ("chosen", "cargo", "chosen"),
        ("", "cargo", "cargo/arora-assets"),
        ("", "", "home/.cargo/arora-assets"),
    ] {
        let selected = cache_directory(|key| {
            Some(
                match key {
                    "ARORA_ASSETS_CACHE_DIR" => explicit,
                    "CARGO_HOME" => cargo,
                    "HOME" => "home",
                    _ => "",
                }
                .into(),
            )
        })
        .unwrap();
        assert_eq!(selected, std::path::PathBuf::from(expected));
    }
    assert!(cache_directory(|_| Some(String::new())).is_err());
}

/// Resolving an exact version is insufficient: builds also require a public asset and the pinned hash.
#[test]
fn private_assets_and_mismatched_pins_are_refused_before_storage() {
    let mut resolved = pin();
    let expected = Pin {
        scoped_name: NAME.into(),
        version: "1.2.3".into(),
        content_hash: content_hash(BYTES),
    };
    assert!(validate_resolved("robot", &expected, &resolved)
        .unwrap_err()
        .to_string()
        .contains("must be public"));
    resolved.private = false;
    resolved.content_hash = content_hash(b"different");
    assert!(matches!(
        validate_resolved("robot", &expected, &resolved),
        Err(Error::HashMismatch { .. })
    ));
    resolved.content_hash = expected.content_hash.clone();
    validate_resolved("robot", &expected, &resolved).unwrap();
}
