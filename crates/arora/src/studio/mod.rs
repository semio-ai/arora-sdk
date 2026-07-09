//! The Semio Studio connection: what [`crate::run_with_hal`] wires around the
//! runtime loop when the `studio-bridge` feature selects Studio as the bridge
//! — Firebase auth, token rotation, the Zenoh connection, device registration.
//!
//! This is what makes `arora` Studio-capable (not just its binary):
//! the full run — read Firebase config + Zenoh endpoints from the environment,
//! load/save an encrypted refresh token from a local file, build the real
//! [`ZenohDeviceClient`] (the studio-bridge Zenoh connector), and drive it via
//! [`crate::run_with_bridge_builder`] over a fresh [`SimpleDataStore`].
//!
//! The binary ([`crate::main`]) is a thin
//! wrapper that just calls [`crate::run`].
//!
//! Configuration is environment-only for now:
//!   - `FIREBASE_*` — Firebase project options (see
//!     [`FirebaseOptions::from_env`]).
//!   - `FIREBASE_*_EMULATOR_HOST` — Firebase emulator overrides (see
//!     [`FirebaseEmulatorOptions::from_env`]).
//!   - `ZENOH_ENDPOINTS` — comma-separated Zenoh router endpoints (e.g.
//!     `tcp/localhost:7447`). Empty/unset falls back to LAN multicast scouting.
//!   - `IDENTITY_FILE` — path to the refresh-token file; defaults to
//!     `<app_data_dir>/refresh_token`. Use it to run several devices on one host.
//!   - `RUST_LOG` — log filter (`env_logger`).

mod app_data_files;
mod token_storage;

use std::sync::Arc;

use anyhow::{Context, Result};
use arora_bridge::Bridge;
use arora_simple_data_store::SimpleDataStore;
use arora_studio_bridge_client::firestore_support::options::{
    FirebaseEmulatorOptions, FirebaseOptions,
};
use arora_studio_bridge_client::zenoh::ZenohDeviceClient;
use log::{info, warn};

use app_data_files::ensure_app_data_dir;

/// Run the Studio-connected device to completion (until the device is
/// unregistered or the process is interrupted).
///
/// Configuration is environment-only for now (a CLI / bridge-config file is a
/// follow-up); it registers the device with Studio from the configured device
/// info (`DEVICE_NAME`, `MODEL_FAMILY`, `HARDWARE_VERSION`, …) when any is set.
/// The Studio-connected run, over the caller's HAL: the whole Studio side (Firebase
/// auth, token rotation, Zenoh connection, device registration) is identical
/// for every device — only the hardware behind it differs. A device build
/// (e.g. a Vizij rig) injects its HAL here and is a Studio device.
pub(crate) fn run_with_hal(hal: Arc<dyn arora_hal::Hal>) -> Result<()> {
    // Pick the operator front end (terminal UI when interactive, headless
    // otherwise) first: doing so installs the matching log sink, so the startup
    // logging below is captured by whichever front end was chosen.
    let frontend = crate::run::select_frontend();

    // Read the Firebase options and Zenoh endpoints from the environment.
    let firebase_options = FirebaseOptions::from_env();
    let firebase_emulator_options = FirebaseEmulatorOptions::from_env();
    let endpoints: Vec<String> = std::env::var("ZENOH_ENDPOINTS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // Look for the refresh token that might have been saved. It is encrypted
    // with a key stored in the same app-data directory.
    let app_data_dir = ensure_app_data_dir().context("could not create app data directory")?;
    let key_path = app_data_dir.join("key");
    let token_path = match std::env::var("IDENTITY_FILE") {
        Ok(identity_file) => std::path::PathBuf::from(identity_file),
        Err(_) => app_data_dir.join("refresh_token"),
    };

    // Install the rustls ring crypto provider (the Zenoh/Firebase TLS stacks
    // need a default provider).
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    // Load and decrypt the refresh token, if one was saved.
    let refresh_token = token_storage::load_token(&key_path, &token_path)
        .ok()
        .flatten();
    if refresh_token.is_some() {
        info!("Refresh token found");
    } else {
        warn!("No refresh token found");
    }

    // Persist future refresh tokens as the client rotates them.
    let save_cb: Box<dyn FnMut(String) + Send + Sync> = {
        let key_path = key_path.clone();
        let token_path = token_path.clone();
        Box::new(move |token: String| {
            if let Err(e) = token_storage::save_token(&key_path, &token_path, &token) {
                warn!("Failed to save refresh token to {:?}: {:?}", token_path, e);
            }
        })
    };

    // Device info to register with Studio, from the environment. `None` when
    // nothing is configured, so we don't clear an already-registered device.
    let device_info = device_info_from_env();

    info!("Connecting via Zenoh (endpoints: {:?})", endpoints);

    crate::run_with_frontend(
        hal,
        Arc::new(SimpleDataStore::new()),
        frontend,
        move || async move {
        let client = ZenohDeviceClient::new(
            &firebase_options,
            Some(&firebase_emulator_options),
            refresh_token,
            Some(save_cb),
            endpoints,
        )
        .await
        // `arora_studio_bridge_client::error::Error` has no `Display` impl,
        // so format it with `{e:?}`.
        .map_err(|e| anyhow::anyhow!("failed to connect to Semio Studio via Zenoh: {e:?}"))?;
        let client: Arc<dyn Bridge> = Arc::new(client);

        // Register this device with Studio from the configured device info.
        if let Some(info) = device_info {
            client
                .update_device_info(Some(info))
                .await
                .context("failed to register device info with Studio")?;
            info!("Registered device info with Studio");
        }
        Ok(client)
    })
}

/// Build the device info to register from the environment, or `None` if nothing
/// is configured (so registration is skipped and an existing registration is
/// left untouched). `DEVICE_OWNERS` is a comma-separated list.
fn device_info_from_env() -> Option<arora_bridge::DeviceInfo> {
    let owners: Vec<String> = std::env::var("DEVICE_OWNERS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|o| o.trim().to_string())
                .filter(|o| !o.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let info = arora_bridge::DeviceInfo {
        name: std::env::var("DEVICE_NAME").ok(),
        description: std::env::var("DEVICE_DESCRIPTION").ok(),
        model_family: std::env::var("MODEL_FAMILY").ok(),
        hardware_version: std::env::var("HARDWARE_VERSION").ok(),
        software_version: std::env::var("SOFTWARE_VERSION").ok(),
        owners,
    };
    let configured = info.name.is_some()
        || info.description.is_some()
        || info.model_family.is_some()
        || info.hardware_version.is_some()
        || info.software_version.is_some()
        || !info.owners.is_empty();
    configured.then_some(info)
}
