//! The Semio Studio connection: what [`crate::run_with_hal`] wires around the
//! runtime loop when the `studio-bridge` feature selects Studio as the bridge
//! — Firebase auth, token rotation, the Zenoh connection, device registration.
//!
//! This is what makes `arora` Studio-capable (not just its binary):
//! the full run — read Firebase config + Zenoh endpoints from the environment,
//! load/save an encrypted refresh token from a local file, build the real
//! [`ZenohDeviceClient`] (the studio-bridge Zenoh connector), and drive it via
//! [`crate::run_with_frontend`] over a fresh [`SimpleDataStore`].
//!
//! The binary ([`crate::main`]) is a thin
//! wrapper that just calls [`crate::run`].
//!
//! The public Firebase config and the production bridge endpoint are baked into
//! `arora-studio-bridge-client`, so this is zero-config for production. These env
//! vars override at runtime:
//!   - `FIREBASE_*` — Firebase project options (see
//!     [`FirebaseOptions::from_env`]).
//!   - `FIREBASE_*_EMULATOR_HOST` — Firebase emulator overrides (see
//!     [`FirebaseEmulatorOptions::from_env`]).
//!   - `STUDIO_BRIDGE_ENDPOINT` — override the baked bridge endpoint (e.g.
//!     `tcp/localhost:7447`) to target a local/preprod bridge without a rebuild.
//!   - `IDENTITY_FILE` — path to the refresh-token file; defaults to
//!     `<app_data_dir>/refresh_token`. Use it to run several devices on one host.
//!   - `DEVICE_OWNERS` (comma-separated), `DEVICE_NAME`, `MODEL_FAMILY`,
//!     `DEVICE_DESCRIPTION`, `HARDWARE_VERSION`, `SOFTWARE_VERSION` — the device
//!     info registered with Studio. Under the terminal UI the operator is
//!     prompted for the owner, name, and model family at startup; a set env var
//!     pre-fills (and skips the prompt for) its field. An empty owner disables
//!     Studio. Headless, only these env vars are read (no prompt).
//!   - `RUST_LOG` — log filter (`env_logger`).

mod app_data_files;
mod token_storage;

use anyhow::{Context, Result};
use arora_bridge::{Bridge, DeviceInfo};
use arora_simple_data_store::SimpleDataStore;
use arora_studio_bridge_client::firestore_support::options::{
    FirebaseEmulatorOptions, FirebaseOptions,
};
use arora_studio_bridge_client::zenoh::ZenohDeviceClient;
use log::{info, warn};

use crate::operator::Operator;

use app_data_files::ensure_app_data_dir;

/// The prompt shown to the operator for the Studio owner UID(s). Empty is
/// allowed and meaningful: no owner means "do not connect to Studio".
const OWNER_LABEL: &str =
    "Semio Studio owner UID(s), comma-separated — leave empty to skip (disables Studio support)";
/// The prompt for the device name. Empty falls back to a generated
/// `arora-device-<random>` name, so the device is never nameless.
const NAME_LABEL: &str = "Device name — leave empty for a generated name";
/// The prompt for the model family. Empty is fine (the device registers with no
/// model family).
const MODEL_FAMILY_LABEL: &str = "Model family — optional, leave empty to skip";

/// Run the Studio-connected device to completion (until the device is
/// unregistered or the process is interrupted).
///
/// Under the terminal UI, the operator is prompted for the device info (owner,
/// name, model family) at startup, each field pre-filled by its env var when
/// set (`DEVICE_OWNERS`, `DEVICE_NAME`, `MODEL_FAMILY`, …). Leaving the owner
/// empty disables Studio: the device then runs over the open local bridge
/// instead. Headless, there is nobody to prompt, so the device info comes from
/// the environment only. The whole Studio side (Firebase auth, token rotation,
/// Zenoh connection, device registration) is identical for every device — only
/// the hardware behind it differs. A device build (e.g. a Vizij rig) injects its
/// HAL here and is a Studio device.
pub(crate) async fn run_with_hal(hal: Box<dyn arora_hal::Hal>) -> Result<()> {
    // Pick the operator front end (terminal UI when interactive, headless
    // otherwise) first: doing so installs the matching log sink, so the startup
    // logging below is captured by whichever front end was chosen.
    let frontend = crate::run::select_frontend();

    // Decide the Studio connection.
    //
    // Interactive front end (the terminal UI): ask the operator for the device
    // info (owner, name, model family), letting env vars pre-fill any field.
    // An empty owner means "disable Studio", in which case we do not connect at
    // all and fall through to the open local bridge (see below).
    //
    // Headless / unattended: there is nobody to prompt, so keep today's
    // behavior exactly — `connect()` builds the bridge and registers device info
    // from the environment only (skipping registration when nothing is set).
    let client = if frontend.interactive {
        connect_with_operator(&*frontend.operator).await?
    } else {
        Some(connect().await?)
    };

    match client {
        Some(client) => {
            crate::run_with_frontend(hal, client, Box::new(SimpleDataStore::new()), frontend).await
        }
        None => {
            // Skip-Studio fallback: the operator left the owner empty, so there
            // is no Studio to connect to. Rather than run with no remote at all,
            // fall through to the same open local bridge the non-Studio build
            // serves — local editors still reach the device, just without Semio
            // Studio.
            info!("studio-bridge: skipped by operator (no owner) — running without Studio");
            let bridge = crate::run::local_ws_bridge().await?;
            crate::run_with_frontend(hal, bridge, Box::new(SimpleDataStore::new()), frontend).await
        }
    }
}

/// Build the Studio bridge for an interactive run, resolving the device info
/// through `operator`: each field is taken from its env var when set, otherwise
/// the operator is prompted (on the terminal UI's prompt line). Returns
/// `Ok(None)` when the operator declines Studio by leaving the owner empty (and
/// `DEVICE_OWNERS` is unset) — the caller then runs without a Studio bridge.
///
/// This is the operator-driven counterpart to [`connect`] (which is env-only,
/// for embedders that have no operator). The bridge itself is built the same
/// way; only how the device info is gathered differs.
pub async fn connect_with_operator(operator: &dyn Operator) -> Result<Option<Box<dyn Bridge>>> {
    // Owner first: `DEVICE_OWNERS` wins if set; otherwise prompt (not required).
    // An empty result disables Studio, so short-circuit before building anything.
    let owners = resolve_owners(operator).await;
    if owners.is_empty() {
        return Ok(None);
    }

    // Owner given, so this device registers with Studio. Resolve the rest:
    // the name always ends up non-empty (generated when blank); model family and
    // the version fields may stay empty.
    let name = resolve_field(operator, "DEVICE_NAME", NAME_LABEL)
        .await
        .unwrap_or_else(generated_device_name);
    let model_family = resolve_field(operator, "MODEL_FAMILY", MODEL_FAMILY_LABEL).await;
    let info = DeviceInfo {
        name: Some(name),
        description: env_nonempty("DEVICE_DESCRIPTION"),
        model_family,
        hardware_version: env_nonempty("HARDWARE_VERSION"),
        // Software version stays env/auto — not something an operator types.
        software_version: env_nonempty("SOFTWARE_VERSION"),
        owners,
    };

    let client = build_studio_bridge().await?;
    client
        .update_device_info(Some(info))
        .await
        .context("failed to register device info with Studio")?;
    info!("Registered device info with Studio");
    Ok(Some(client))
}

/// Build the Semio Studio bridge — a ready-to-inject [`Bridge`] endpoint — from
/// the environment, register the device from any configured device info, and
/// return it.
///
/// This is the injectable counterpart to [`run_with_hal`]: where `run_with_hal`
/// builds the bridge *and* owns the run loop, `connect` returns just the
/// finished, registered bridge so an embedder can attach it to a device with
/// [`AroraBuilder::with_bridge`](crate::AroraBuilder::with_bridge) — the
/// producer side of "let a Studio see this runtime's live data". A host that
/// already runs its own Arora (e.g. the Vizij standalone) opts into a Studio
/// connection with:
///
/// ```ignore
/// let studio_bridge = arora::studio::connect().await?;
/// let device = arora::Arora::builder().with_bridge(studio_bridge).build()?;
/// ```
///
/// The whole Studio side (Firebase auth, token rotation, the Zenoh connection,
/// device registration) is identical for every device; only the runtime it is
/// attached to differs. Configuration is environment-only — see the
/// [module docs](self) for the variables read.
pub async fn connect() -> Result<Box<dyn Bridge>> {
    let client = build_studio_bridge().await?;

    // Register this device with Studio from the configured device info. `None`
    // when nothing is set, so an already-registered device is left untouched.
    if let Some(info) = device_info_from_env() {
        client
            .update_device_info(Some(info))
            .await
            .context("failed to register device info with Studio")?;
        info!("Registered device info with Studio");
    }

    Ok(client)
}

/// Build the Studio bridge itself — Firebase auth, token load/rotate, the Zenoh
/// connection — without registering any device info. Both [`connect`] (env-only)
/// and [`connect_with_operator`] (operator-driven) build the bridge this way and
/// then register their own resolved [`DeviceInfo`].
async fn build_studio_bridge() -> Result<Box<dyn Bridge>> {
    // Read the Firebase options and Zenoh endpoints from the environment.
    let firebase_options = FirebaseOptions::from_env();
    let firebase_emulator_options = FirebaseEmulatorOptions::from_env();
    // The bridge endpoint is baked into `arora-studio-bridge-client` (v3);
    // `STUDIO_BRIDGE_ENDPOINT` overrides it at runtime to target a local/preprod
    // bridge (e.g. `tcp/localhost:7447`) without a rebuild.
    let endpoint_override = std::env::var("STUDIO_BRIDGE_ENDPOINT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

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

    // Build the Zenoh bridge here (awaiting its async construction on the caller's
    // runtime) and hand the finished bridge to the run loop — no bridge factory.
    // Default to the endpoint baked into the client crate; a non-empty
    // `STUDIO_BRIDGE_ENDPOINT` (captured above as `endpoint_override`) routes to
    // `new_endpoint` instead, targeting a local/preprod bridge.
    let client = match endpoint_override {
        Some(endpoint) => {
            info!("Connecting to Semio Studio via Zenoh (endpoint: {endpoint})");
            ZenohDeviceClient::new_endpoint(
                &firebase_options,
                Some(&firebase_emulator_options),
                refresh_token,
                Some(save_cb),
                endpoint,
            )
            .await
        }
        None => {
            info!("Connecting to Semio Studio via the baked-in bridge endpoint");
            ZenohDeviceClient::new(
                &firebase_options,
                Some(&firebase_emulator_options),
                refresh_token,
                Some(save_cb),
            )
            .await
        }
    }
    // `arora_studio_bridge_client::error::Error` has no `Display` impl,
    // so format it with `{e:?}`.
    .map_err(|e| anyhow::anyhow!("failed to connect to Semio Studio via Zenoh: {e:?}"))?;
    let client: Box<dyn Bridge> = Box::new(client);
    Ok(client)
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

/// Resolve the Studio owner UID(s): `DEVICE_OWNERS` wins when set (env overrides
/// the prompt); otherwise ask the operator (not required — an empty answer is
/// meaningful, it disables Studio). Returns the parsed, trimmed, non-empty UIDs.
async fn resolve_owners(operator: &dyn Operator) -> Vec<String> {
    let raw = match env_nonempty("DEVICE_OWNERS") {
        Some(value) => Some(value),
        None => operator.ask_text(OWNER_LABEL, false).await,
    };
    raw.map(|s| {
        s.split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Resolve one free-text device-info field: the env var wins when set (env
/// overrides the prompt); otherwise ask the operator. A blank answer becomes
/// `None`. Never required — the caller supplies any default.
async fn resolve_field(operator: &dyn Operator, env_key: &str, label: &str) -> Option<String> {
    if let Some(value) = env_nonempty(env_key) {
        return Some(value);
    }
    operator
        .ask_text(label, false)
        .await
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// An environment variable's value, trimmed, or `None` if unset or blank.
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A generated device name, `arora-device-<random>`, for when the operator does
/// not provide one — so a registered device is never nameless.
fn generated_device_name() -> String {
    format!("arora-device-{}", random_device_suffix())
}

/// A short random lowercase-alphanumeric suffix. Seeded from the OS via
/// `RandomState` (no extra dependency), the same technique vizij-standalone uses
/// for its generated device names.
fn random_device_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(std::process::id() as u64);
    let mut value = hasher.finish();
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut suffix = String::with_capacity(8);
    for _ in 0..8 {
        suffix.push(ALPHABET[(value % 36) as usize] as char);
        value /= 36;
    }
    suffix
}
