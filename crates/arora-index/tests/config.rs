use arora_index::{build, StudioConfig};

fn env_value(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

#[test]
fn environment_configuration_uses_injected_lookup() {
    let config = StudioConfig::from_lookup(|name| match name {
        "FIREBASE_PROJECT_ID" => Some("project".into()),
        "FIREBASE_STORAGE_BUCKET" => Some("bucket".into()),
        _ => None,
    })
    .unwrap();
    assert_eq!(config.firestore_url, "https://firestore.googleapis.com");
    assert_eq!(config.storage_url, "https://firebasestorage.googleapis.com");
    let config = StudioConfig::from_lookup(|name| {
        Some(
            match name {
                "FIREBASE_FIRESTORE_EMULATOR_HOST" => "localhost:8080",
                "FIREBASE_STORAGE_EMULATOR_HOST" => "127.0.0.1:9199",
                _ => "fixture",
            }
            .into(),
        )
    })
    .unwrap();
    assert_eq!(config.firestore_url, "http://localhost:8080");
    assert_eq!(config.storage_url, "http://127.0.0.1:9199");
    assert!(StudioConfig::from_lookup(|_| None)
        .unwrap_err()
        .to_string()
        .contains("FIREBASE_PROJECT_ID"));
    assert!(StudioConfig::from_lookup(
        |name| (name == "FIREBASE_PROJECT_ID").then(|| "fixture".into())
    )
    .unwrap_err()
    .to_string()
    .contains("FIREBASE_STORAGE_BUCKET"));
}

#[test]
fn empty_environment_values_are_unset_and_cache_order_is_explicit() {
    assert_eq!(env_value(Some(String::new())), None);
    let cfg = StudioConfig::from_lookup(|key| {
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
        assert!(StudioConfig::from_lookup(|key| Some(
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
        let selected = build::cache_directory(|key| {
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
    assert!(build::cache_directory(|_| Some(String::new())).is_err());
}
