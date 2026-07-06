//! Downloads the robots' GLB models (public Semio Studio assets) into
//! `models/`, once per missing file. Set `ARORA_HAL_ROS2_SKIP_MODELS=1` to
//! build offline; the built-in robot configs then need an explicit
//! `model_glb_path` (or a joint-id override) at runtime.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=ARORA_HAL_ROS2_SKIP_MODELS");
    if std::env::var("ARORA_HAL_ROS2_SKIP_MODELS").is_ok_and(|v| v == "1") {
        println!("cargo:warning=ARORA_HAL_ROS2_SKIP_MODELS=1: robot GLB models not downloaded");
        return;
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        const USER_ID: &str = "iFx0jWNTf7ePZanmD8fY0NQmxHp2";
        get_model("quori", USER_ID, "b5647c7f-474c-4b00-b7df-29602fd4dd61").await;
        get_model("nao", USER_ID, "2db815de-ad4c-48a8-93b4-37a7deacf91d").await;
        get_model("pepper", USER_ID, "3fddaf18-e36c-471e-bf65-2d09a7e25c95").await;
        get_model("ur3", USER_ID, "8afa3b4b-4f20-49d8-a1c2-b36de8c57dad").await;
        get_model("ur5", USER_ID, "160ba29a-bf84-464b-8ce7-13f57fc015a5").await;
        get_model("g1", USER_ID, "abddf81b-3e20-4589-8ed0-de8d0dc50f8b").await;
    });
}

async fn get_model(name: &str, user_id: &str, asset_id: &str) {
    let models_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(|dir| std::path::PathBuf::from(dir).join("models"))
        .unwrap();
    if !models_dir.exists() {
        std::fs::create_dir_all(&models_dir).unwrap();
    }
    let model_path = models_dir.join(format!("{name}.glb"));
    if model_path.exists() {
        println!("Model for {name} already exists, skipping download.");
        return;
    }
    println!("Downloading model for {name}...");
    let model = download_latest_public_robot_model(user_id, asset_id)
        .await
        .unwrap();
    std::fs::write(model_path, model).unwrap();
}

/// Downloads the latest version of a publicly-readable robot model from the
/// Semio Studio Firebase Storage bucket.
async fn download_latest_public_robot_model(
    user_id: &str,
    asset_id: &str,
) -> Result<bytes::Bytes, String> {
    let storage_host = "https://firebasestorage.googleapis.com";
    let bucket = "semio-studio-deployment.appspot.com";
    let version = "latest";
    https_client()
        .get(format!(
            "{storage_host}/v0/b/{bucket}/o/model%2F{user_id}%2F{asset_id}%2F{version}?alt=media"
        ))
        .send()
        .await
        .map_err(|e| format!("Network error when downloading robot model: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Error response when downloading robot model: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Error while downloading robot model: {e}"))
}

/// A reqwest client that trusts only the bundled webpki roots (no OS trust
/// store — this runs in a build script, possibly on a headless device), with
/// the ring provider selected explicitly since no process-default provider is
/// installed here.
fn https_client() -> reqwest::Client {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports the default protocol versions")
    .with_root_certificates(roots)
    .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .build()
        .expect("reqwest client")
}
