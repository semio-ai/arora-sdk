use arora_index::{content_hash, AssetType, Error, NoToken, Resolver, StudioConfig};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

/// Requires an explicitly selected local demo project and both running emulators.
#[tokio::test]
#[ignore = "requires local Firebase emulators and ARORA_ASSETS_EMULATOR_TEST=1"]
async fn public_and_private_models() {
    if !emulator_enabled(|name| std::env::var(name).ok()) {
        eprintln!("Skipping emulator test: ARORA_ASSETS_EMULATOR_TEST is unset or not 1");
        return;
    }
    for name in [
        "FIREBASE_FIRESTORE_EMULATOR_HOST",
        "FIREBASE_STORAGE_EMULATOR_HOST",
    ] {
        let host = env_value(std::env::var(name).ok()).unwrap_or_else(|| panic!("missing {name}"));
        let url = reqwest::Url::parse(&format!("http://{host}")).unwrap();
        assert!(
            matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
            "{name} must address localhost"
        );
        assert!(url.port().is_some(), "{name} must include a port");
    }
    let config = StudioConfig::from_env().unwrap();
    assert!(
        config.project_id.starts_with("demo-"),
        "emulator project must start with demo-"
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let root = format!(
        "{}/v1/projects/{}/databases/(default)/documents",
        config.firestore_url, config.project_id
    );
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let bytes = b"local emulator model fixture";
    let mut public_name = String::new();
    let mut private_name = String::new();
    for private in [false, true] {
        let id = format!("fixture-{suffix}-{private}");
        let name = format!("@fixture/{id}");
        let object = format!("publishedAssets/{id}/releases/release/artifacts/primary");
        let fields = json!({
            "scopedName": {"stringValue": name}, "assetType": {"stringValue": "model"},
            "ownerId": {"stringValue": "fixture-owner"}, "deprecated": {"booleanValue": false},
            "publicRole": if private { json!({"nullValue": null}) } else { json!({"stringValue": "viewer"}) },
            "latestVersion": {"stringValue": "1.0.0"}
        });
        let release = json!({
            "version": {"stringValue": "1.0.0"}, "deprecated": {"booleanValue": false},
            "publishedAt": {"timestampValue": "2026-01-01T00:00:00Z"},
            "artifacts": {"mapValue": {"fields": {"primary": {"mapValue": {"fields": {
                "contentHash": {"stringValue": content_hash(bytes)}, "role": {"stringValue": "model"},
                "minRole": {"stringValue": "viewer"},
                "storage": {"mapValue": {"fields": {"fileBucket": {"stringValue": object}}}}
            }}}}}}
        });
        let named = json!({"publishedAssetId": {"stringValue": id}, "kind": {"stringValue": "claim"},
            "scopedName": {"stringValue": name}, "ownerId": {"stringValue": "fixture-owner"}});
        for (path, fields) in [
            (format!("publishedAssets/{id}"), fields),
            (format!("publishedAssets/{id}/releases/release"), release),
            (format!("scopedNames/{}", name.replace('/', ":")), named),
        ] {
            client
                .patch(format!("{root}/{path}"))
                .header("Authorization", "Bearer owner")
                .json(&json!({"fields": fields}))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
        let mut upload = reqwest::Url::parse(&format!(
            "{}/v0/b/{}/o",
            config.storage_url, config.storage_bucket
        ))
        .unwrap();
        upload
            .query_pairs_mut()
            .append_pair("uploadType", "media")
            .append_pair("name", &object);
        client
            .post(upload)
            .header("Authorization", "Bearer owner")
            .header("Content-Type", "application/octet-stream")
            .body(bytes.to_vec())
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        if private {
            private_name = name;
        } else {
            public_name = name;
        }
    }
    let resolver = Resolver::new(config, NoToken).unwrap();
    let pin = resolver
        .resolve(&public_name, "*", AssetType::Model)
        .await
        .unwrap();
    assert_eq!(resolver.fetch(&pin).await.unwrap().bytes, bytes);
    let error = resolver
        .resolve(&private_name, "*", AssetType::Model)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NotFound { .. }));
    assert!(error.to_string().contains("not found, or not public"));
}

fn emulator_enabled(mut lookup: impl FnMut(&str) -> Option<String>) -> bool {
    env_value(lookup("ARORA_ASSETS_EMULATOR_TEST")).as_deref() == Some("1")
}

#[test]
fn emulator_opt_in_requires_one() {
    for value in [None, Some(""), Some("0")] {
        assert!(!emulator_enabled(|_| value.map(str::to_owned)));
    }
    assert!(emulator_enabled(|_| Some("1".into())));
}

fn env_value(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}
