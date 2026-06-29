//! The headless device runner.
//!
//! This is what makes `arora` the headless-capable crate (not just its binary):
//! the full run — read Firebase config + Zenoh endpoints from the environment,
//! load/save an encrypted refresh token from a local file, build the real
//! [`ZenohDeviceClient`] (the studio-bridge Zenoh connector), and drive it via
//! [`crate::launch_with`] over a [`FakeHal`] and a fresh [`SimpleDataStore`].
//!
//! Ported from `studio-bridge/headless`. The binary ([`crate::main`]) is a thin
//! wrapper that just calls [`launch`].
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
use arora_hal::FakeHal;
use arora_simple_data_store::SimpleDataStore;
use firestore_stream::options::{FirebaseEmulatorOptions, FirebaseOptions};
use log::{info, warn};
use studio_bridge_device_client::zenoh::ZenohDeviceClient;

use app_data_files::ensure_app_data_dir;

/// Run the headless device runner to completion (until the device is
/// unregistered or the process is interrupted).
///
/// TODO: device-info registration + CLI args (grow launch). For now this is
/// env-only and does not register device info, load a bridge config, or prompt.
pub fn launch() -> Result<()> {
    env_logger::init();

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

    info!("Connecting via Zenoh (endpoints: {:?})", endpoints);

    let hal = Arc::new(FakeHal::new());
    crate::launch_with(hal, SimpleDataStore::new(), move || async move {
        let client = ZenohDeviceClient::new(
            &firebase_options,
            Some(&firebase_emulator_options),
            refresh_token,
            Some(save_cb),
            endpoints,
        )
        .await
        // `studio_bridge_device_client::error::Error` has no `Display` impl,
        // so format it with `{e:?}`.
        .map_err(|e| anyhow::anyhow!("failed to connect to Semio Studio via Zenoh: {e:?}"))?;
        Ok(Arc::new(client) as Arc<dyn Bridge>)
    })
}
