//! The Semio Studio connection: what [`crate::run_with_hal`] wires around the
//! runtime loop when the `studio-bridge` feature selects Studio as the bridge
//! — Firebase auth, token rotation, the Zenoh connection, device registration.
//!
//! This is what makes `arora` Studio-capable (not just its binary):
//! the full run — read Firebase config + Zenoh endpoints from the environment,
//! load/save an encrypted refresh token from a local file, build the real
//! `ZenohDeviceClient` (the studio-bridge Zenoh connector), and drive it via
//! [`crate::run_with_frontend`] over the caller's [`DataStore`]
//! (`arora_types::data::DataStore` — the entrypoints default it to a fresh
//! `SimpleDataStore` for devices that do not care).
//!
//! The binary (`src/main.rs`) is a thin wrapper that just calls [`crate::run`].
//!
//! # Whether a run connects
//!
//! Studio is opt-in: a run contacts it only when it asks for it. What a run
//! asks is a [`Choice`], which a runner's command line gives ([`StudioArgs`]:
//! `--studio`, `--no-studio`) and
//! [`AroraBuilder::with_studio`](crate::AroraBuilder::with_studio) hands to
//! [`AroraBuilder::run`](crate::AroraBuilder::run):
//!
//! | the run asks | the default bridge |
//! |---|---|
//! | `--studio`, or `ARORA_STUDIO=1` | Studio; a device holding no saved credential registers as a new Studio device |
//! | nothing, or `--no-studio` | the open local bridge, whatever the device holds |
//!
//! `--no-studio` wins over `--studio` and `ARORA_STUDIO`: a run told both
//! declines, and a run passing it overrides the variable.
//!
//! The saved credential is the refresh token of the device's Studio account,
//! in its device directory's `studio/` subdirectory ([`crate::device_dir`]).
//! It does not connect a run that does not ask: a saved credential is not
//! proof of an opt-in, as a headless run of arora 13.4 or earlier signed the
//! device up unasked. A run says what it decided in one line of its log; one
//! that does not ask looks the credential up only to say so, pointing a
//! device holding one to `--studio`, and serves the local bridge whatever the
//! lookup finds.
//!
//! The `connect` functions below are themselves the ask: an embedder calling
//! one connects, a device holding no credential registering.
//!
//! The public Firebase config and the production bridge endpoint are baked into
//! `arora-studio-bridge-client`, so this is zero-config for production. These env
//! vars override at runtime:
//!   - `FIREBASE_*` — Firebase project options (see
//!     `FirebaseOptions::from_env`).
//!   - `FIREBASE_*_EMULATOR_HOST` — Firebase emulator overrides (see
//!     `FirebaseEmulatorOptions::from_env`).
//!   - `STUDIO_BRIDGE_ENDPOINT` — override the baked bridge endpoint (e.g.
//!     `tcp/localhost:7447`) to target a local/preprod bridge without a rebuild.
//!     Over a TLS endpoint (`tls/…`), the bridge knows the device by a
//!     certificate Studio issues it, kept beside the refresh token.
//!   - `DEVICE_LOCAL_ID` — the device's local id, which names its directory
//!     `<data_local_dir>/semio/arora/devices/<local id>` ([`crate::device_dir`]);
//!     `default` when unset. Two devices on one host set different ids. The
//!     Studio connection keeps its credentials (the refresh token of the
//!     device's Studio account and the key encrypting it, and its bridge key
//!     and certificate) in the directory's `studio/` subdirectory.
//!   - `DEVICE_DIR` — the device directory itself, replacing the per-user
//!     path. An embedder passes it to [`connect_with_device_dir`] instead.
//!   - `IDENTITY_FILE` — deprecated: the identity it names migrates to
//!     `<IDENTITY_FILE>_dir`, which the run uses as its device directory.
//!   - `DEVICE_OWNERS` (comma-separated), `DEVICE_NAME`, `MODEL_FAMILY`,
//!     `DEVICE_DESCRIPTION`, `HARDWARE_VERSION`, `SOFTWARE_VERSION` — the device
//!     info registered with Studio. A set env var wins over every other source
//!     of its field; a blank one counts as unset. See
//!     [Registration](#registration).
//!   - `RUST_LOG` — log filter (`env_logger`).
//!
//! # Registration
//!
//! What the device registers with Studio as is resolved field by field, the
//! first source that gives a field winning:
//!
//! | field | sources, in order |
//! |---|---|
//! | owners | `DEVICE_OWNERS`, the saved registration, the operator |
//! | name | `DEVICE_NAME`, the saved registration, the operator (generated when left blank) |
//! | model family | `MODEL_FAMILY`, the HAL's description, the saved registration, the operator |
//! | hardware / software version | `HARDWARE_VERSION` / `SOFTWARE_VERSION`, the HAL's description |
//! | description | `DEVICE_DESCRIPTION` |
//!
//! The operator is asked only under the terminal UI of a run that connects,
//! and only for a field no other source gives; a saved registration answers
//! the model family too (with none when the operator typed none), so a device
//! registered once is asked nothing again. Leaving the owner empty declines
//! Studio: the run serves the open local bridge instead, and nothing is saved.
//!
//! The **saved registration** is `device.json` in the device directory's
//! `studio/` subdirectory: the owners and the name a run
//! that asked the operator anything resolved, and the model family the operator
//! typed, if any. It is written by such a run, before it connects, and read by
//! every later one, interactive or headless — so a device set up once under the
//! terminal UI can run headless. Delete it to be asked again.
//!
//! Headless, nobody is asked: the device registers with the info the
//! environment and the saved registration give, completed by the HAL's
//! description, and registers nothing — leaving an existing registration
//! untouched — when neither the environment nor a saved registration gives any.
//! [`crate::AroraBuilder::run`] passes the description of the device's HAL.

mod credentials;
mod registration;

use std::path::{Path, PathBuf};

use anyhow::Result;
use arora_bridge::{Bridge, DeviceInfo};
use arora_hal::HalDescription;
use arora_studio_bridge_client::firestore_support::options::{
    FirebaseEmulatorOptions, FirebaseOptions,
};
use arora_studio_bridge_client::zenoh::ZenohDeviceClient;
use log::{info, warn};

use crate::device_dir;
use crate::operator::Operator;

use arora_studio_bridge_client::credentials::DeviceCredentials;

use registration::Registration;

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

/// What a run asks of Semio Studio: the [module docs](self#whether-a-run-connects)
/// say what each gives. A runner's command line gives it with
/// [`StudioArgs::choice`]; [`AroraBuilder::with_studio`](crate::AroraBuilder::with_studio)
/// hands it to the run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    /// `--studio`: Studio, a device holding no credential registering as a
    /// new Studio device.
    Studio,
    /// Nothing, or `--no-studio`: the open local bridge, whatever the device
    /// holds.
    #[default]
    Local,
}

/// The variable that asks for Studio as `--studio` does.
const STUDIO_VAR: &str = "ARORA_STUDIO";

/// The Studio options of a runner's command line, as a clap helper that
/// [`RunnerArgs`](crate::RunnerArgs) and [`DeviceCli`](crate::DeviceCli)
/// flatten: `--studio` and `--no-studio`. [`choice`](Self::choice) reads
/// `ARORA_STUDIO` beside them.
#[derive(Clone, Debug, Default, clap::Args)]
pub struct StudioArgs {
    /// Connect to Semio Studio, registering this device with it when it holds
    /// no saved Studio credential; ARORA_STUDIO=1 asks the same. Without
    /// either, the run serves the open local bridge.
    #[arg(long)]
    studio: bool,
    /// Serve the open local bridge, even when --studio or ARORA_STUDIO asks
    /// for Studio.
    #[arg(long)]
    no_studio: bool,
}

impl StudioArgs {
    /// What these options and `ARORA_STUDIO` ask of Studio. `--no-studio`
    /// wins over the other two, and `--studio` over the variable. The variable
    /// is a boolean — `1`, `true`, `yes`, `on`, or `0`, `false`, `no`, `off`,
    /// in any case — and blank, like unset, asks nothing; any other value is
    /// an error naming it.
    pub fn choice(&self) -> Result<Choice> {
        let studio_var =
            std::env::var_os(STUDIO_VAR).map(|value| value.to_string_lossy().into_owned());
        self.choice_with(studio_var.as_deref())
    }

    /// [`choice`](Self::choice), with `studio_var` the value of
    /// `ARORA_STUDIO`, if set.
    fn choice_with(&self, studio_var: Option<&str>) -> Result<Choice> {
        if self.no_studio {
            return Ok(Choice::Local);
        }
        if self.studio {
            return Ok(Choice::Studio);
        }
        let value = studio_var.map(str::trim).unwrap_or_default();
        match value.to_ascii_lowercase().as_str() {
            "" | "0" | "false" | "no" | "off" => Ok(Choice::Local),
            "1" | "true" | "yes" | "on" => Ok(Choice::Studio),
            _ => anyhow::bail!(
                "{STUDIO_VAR}={value:?} is not a boolean: set it to 1, true, yes or on to ask \
                 for Semio Studio, or leave it unset"
            ),
        }
    }
}

/// What a device holds of a Studio credential: the refresh token of its
/// Studio account, which a connection signs it in with.
#[derive(Debug)]
enum Credential {
    /// One that reads.
    Saved,
    /// None: a connection would sign the device up as a new Studio device.
    Absent,
    /// One that does not read (its key missing or another, the file damaged):
    /// a connection would sign the device up anew, as another Studio device.
    Unreadable(String),
}

/// Where a run's default bridge goes, and why; the directory is where the
/// device keeps its Studio credential.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    /// Studio, as the run asks.
    Asked,
    /// The local bridge, as the run does not ask, and the device holds no
    /// saved credential.
    Unregistered(PathBuf),
    /// The local bridge, as the run does not ask, although the device holds a
    /// saved credential: only `--studio` connects with it.
    Saved(PathBuf),
    /// The local bridge, as the run does not ask; the saved credential does
    /// not read, for the reason given.
    Unreadable(PathBuf, String),
    /// The local bridge, as the run does not ask; what the device holds could
    /// not be looked up, for the reason given.
    Unknown(String),
}

impl Decision {
    fn connects(&self) -> bool {
        matches!(self, Decision::Asked)
    }

    /// Says in one line of the log what was decided.
    fn log(&self) {
        match self {
            Decision::Asked => info!(
                "Studio: connecting, as --studio asks; a device with no saved credential \
                 registers with Semio Studio."
            ),
            Decision::Unregistered(dir) => info!(
                "Studio: no saved credential in {}; serving the local bridge. Pass --studio to \
                 register this device with Semio Studio.",
                dir.display()
            ),
            Decision::Saved(dir) => info!(
                "Studio: serving the local bridge, as the run does not ask for Studio. Pass \
                 --studio to connect with the saved credential in {}.",
                dir.display()
            ),
            Decision::Unreadable(dir, error) => warn!(
                "Studio: the saved credential in {} does not read ({error}); serving the local \
                 bridge. Pass --studio to register this device with Semio Studio anew.",
                dir.display()
            ),
            Decision::Unknown(error) => warn!(
                "Studio: the saved credential cannot be looked up ({error}); serving the local \
                 bridge."
            ),
        }
    }
}

/// Where a run asking `choice` goes: see the
/// [module docs](self#whether-a-run-connects). `credential` looks up what the
/// device holds and the directory it keeps it in; it is called only for a run
/// that does not ask, to say what the device holds, and its failure is said,
/// not returned: only the choice decides whether the run connects.
fn decide(choice: Choice, credential: impl FnOnce() -> Result<(PathBuf, Credential)>) -> Decision {
    match choice {
        Choice::Studio => Decision::Asked,
        Choice::Local => match credential() {
            Ok((dir, Credential::Saved)) => Decision::Saved(dir),
            Ok((dir, Credential::Absent)) => Decision::Unregistered(dir),
            Ok((dir, Credential::Unreadable(error))) => Decision::Unreadable(dir, error),
            Err(error) => Decision::Unknown(format!("{error:#}")),
        },
    }
}

/// What the device a run configured from the environment holds of a Studio
/// credential, and the directory it keeps it in. It is looked up where a
/// connection looks, after the same migrations ([`credentials::from_env`]),
/// so the two agree on whether the device holds one; like a connection, the
/// lookup creates the directory, empty, when it is missing.
///
/// A device directory that cannot be named fails the lookup, and credentials
/// that cannot be resolved there — a deprecated `IDENTITY_FILE` naming
/// nothing, a directory that cannot be created — do not read: either way the
/// run serves the local bridge and says why.
fn credential_from_env() -> Result<(PathBuf, Credential)> {
    let dir = device_dir::from_env()?.join(credentials::STUDIO_DIR);
    Ok((dir, resolved(credentials::from_env())))
}

/// What credentials resolved as `credentials` hold.
fn resolved(credentials: Result<DeviceCredentials>) -> Credential {
    match credentials {
        Ok(credentials) => held(&credentials),
        Err(error) => Credential::Unreadable(format!("{error:#}")),
    }
}

/// What `credentials` hold of a Studio credential. A token that reads is what
/// the client signs in with; it signs up only with none, and with one that
/// does not read (`arora-studio-bridge-client` 10).
fn held(credentials: &DeviceCredentials) -> Credential {
    match credentials.refresh_token() {
        Ok(Some(_)) => Credential::Saved,
        Ok(None) => Credential::Absent,
        Err(error) => Credential::Unreadable(error.to_string()),
    }
}

/// Resolve the default bridge of a `studio-bridge` run asking `choice`: the
/// Semio Studio connection, or the open local bridge — when the run does not
/// connect ([module docs](self#whether-a-run-connects)), or when the operator
/// declines Studio.
///
/// A run that connects resolves the device info as the
/// [module docs](self#registration) say: under the terminal UI, the operator
/// is asked for what neither the environment, the HAL's `description`, nor
/// the saved registration gives, and leaving the owner empty disables Studio
/// — the returned bridge is then the open local bridge instead. Headless,
/// nobody is asked. The whole Studio side (Firebase auth, token rotation,
/// Zenoh connection, device registration) is identical for every device —
/// only the hardware behind it differs.
pub(crate) async fn default_bridge(
    frontend: &crate::operator::Frontend,
    description: &HalDescription,
    choice: Choice,
) -> Result<Box<dyn Bridge>> {
    bridge_for(
        decide(choice, credential_from_env),
        connect_studio(frontend, description),
        crate::run::local_ws_bridge(),
    )
    .await
}

/// The bridge `decision` picks, once it is logged: `studio` when it connects,
/// `local` otherwise. Only the one picked runs.
async fn bridge_for(
    decision: Decision,
    studio: impl std::future::Future<Output = Result<Box<dyn Bridge>>>,
    local: impl std::future::Future<Output = Result<Box<dyn Bridge>>>,
) -> Result<Box<dyn Bridge>> {
    decision.log();
    if decision.connects() {
        studio.await
    } else {
        local.await
    }
}

/// The Studio connection of a run that connects, through the operator under
/// the terminal UI: the open local bridge when the operator declines.
async fn connect_studio(
    frontend: &crate::operator::Frontend,
    description: &HalDescription,
) -> Result<Box<dyn Bridge>> {
    // Interactive front end (the terminal UI): ask the operator for the device
    // info nothing else gives. An empty owner means "disable Studio", in which
    // case we do not connect at all and fall through to the open local bridge
    // (see below).
    //
    // Headless / unattended: there is nobody to prompt — the bridge registers
    // the device info the environment and the saved registration give
    // (skipping registration when neither gives any).
    let client = if frontend.interactive {
        connect_with_operator_and_description(&*frontend.operator, description).await?
    } else {
        Some(connect_with_description(description).await?)
    };

    match client {
        Some(client) => Ok(client),
        None => {
            // Skip-Studio fallback: the operator left the owner empty, so there
            // is no Studio to connect to. Rather than run with no remote at all,
            // fall through to the same open local bridge the non-Studio build
            // serves — local editors still reach the device, just without Semio
            // Studio.
            info!("studio-bridge: skipped by operator (no owner) — running without Studio");
            crate::run::local_ws_bridge().await
        }
    }
}

/// Build the Studio bridge for an interactive run, resolving the device info
/// through `operator` as the [module docs](self#registration) say, with no HAL
/// description: each field is taken from its env var when set, else from the
/// saved registration, otherwise the operator is prompted (on the terminal
/// UI's prompt line). Returns `Ok(None)` when the operator declines Studio by
/// leaving the owner empty (and neither `DEVICE_OWNERS` nor a saved
/// registration gives one) — the caller then runs without a Studio bridge.
///
/// This is the operator-driven counterpart to [`connect`] (which never asks,
/// for embedders that have no operator). The bridge itself is built the same
/// way; only how the device info is gathered differs.
pub async fn connect_with_operator(operator: &dyn Operator) -> Result<Option<Box<dyn Bridge>>> {
    connect_with_operator_and_description(operator, &HalDescription::default()).await
}

/// [`connect_with_operator`], completing the device info with what the
/// device's HAL says of itself: its model family, unless `MODEL_FAMILY` is set,
/// is the HAL's (and the operator is not asked for one), and so are its
/// hardware and software versions, unless `HARDWARE_VERSION` /
/// `SOFTWARE_VERSION` are set. Pass `hal.describe().await`.
///
/// When the operator was asked anything, the owners and name resolved — and
/// the model family the operator typed, if any — are saved in the device
/// directory before connecting, and the next run asks nothing.
pub async fn connect_with_operator_and_description(
    operator: &dyn Operator,
    description: &HalDescription,
) -> Result<Option<Box<dyn Bridge>>> {
    let saved_at = registration::path_in(&device_dir::from_env()?);
    let saved = registration::load(&saved_at);
    let Some(resolved) =
        resolve_with_operator(operator, &env_nonempty, saved.as_ref(), description).await
    else {
        return Ok(None);
    };
    if resolved.from_saved {
        log_saved_use(&resolved.info, &saved_at);
    }
    // The credentials first: under the deprecated `IDENTITY_FILE`, resolving
    // them migrates the identity into a device directory that must not exist
    // before, so nothing is written there until they are.
    let credentials = credentials::from_env()?;
    if let Some(answered) = &resolved.answered {
        match registration::save(&saved_at, answered) {
            Ok(()) => info!(
                "studio: saved the registration in {}; the next run asks nothing (delete it to \
                 answer again)",
                saved_at.display()
            ),
            Err(e) => warn!(
                "studio: could not save the registration in {}: {e}; the next run asks again",
                saved_at.display()
            ),
        }
    }
    Ok(Some(
        build_studio_bridge(credentials, Some(resolved.info)).await?,
    ))
}

/// Build the Semio Studio bridge — a ready-to-inject [`Bridge`] endpoint — from
/// the environment, register the device from any configured device info, and
/// return it.
///
/// This is the injectable counterpart to [`crate::run_with_hal`]: where `run_with_hal`
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
/// attached to differs. Nobody is asked anything: the device info is the
/// environment's, completed by the registration an interactive run saved in
/// the device directory, if any — see the [module docs](self#registration).
pub async fn connect() -> Result<Box<dyn Bridge>> {
    connect_with_description(&HalDescription::default()).await
}

/// [`connect`], completing the device info with what the device's HAL says of
/// itself (`hal.describe().await`): its model family, unless `MODEL_FAMILY`
/// is set, and its hardware and software versions, unless `HARDWARE_VERSION`
/// / `SOFTWARE_VERSION` are. The description completes a registration, it
/// does not make one: with neither the environment nor a saved registration
/// giving any device info, nothing is registered, as with [`connect`].
pub async fn connect_with_description(description: &HalDescription) -> Result<Box<dyn Bridge>> {
    let info = unattended_info(&device_dir::from_env()?, description);
    build_studio_bridge(credentials::from_env()?, info).await
}

/// [`connect`], with `device_dir` as the device directory rather than the one
/// the environment names; the Studio credentials go in its `studio/`
/// subdirectory. An app whose platform gives it a data directory (Android, a
/// Tauri app) passes that; [`crate::device_dir::of`] gives the per-user one
/// of a local id. The directories are created if missing, and the
/// credentials on first connection.
///
/// The device info is the environment's, completed by the registration saved
/// in `device_dir` — which only an interactive run of the same device
/// directory writes; an app that never runs one registers exactly what the
/// environment gives, as before.
pub async fn connect_with_device_dir(device_dir: &Path) -> Result<Box<dyn Bridge>> {
    build_studio_bridge(
        credentials::in_device_dir(device_dir)?,
        unattended_info(device_dir, &HalDescription::default()),
    )
    .await
}

/// Build the Studio bridge itself — Firebase auth, token load/rotate, device
/// registration, the Zenoh connection — as the device whose Studio credentials
/// `credentials` holds. With `info`, the device is registered with it before
/// the client connects, which a bridge that certifies devices requires of a
/// new one; without, an existing registration is left untouched. [`connect`],
/// [`connect_with_device_dir`] and [`connect_with_operator`] each pass the
/// [`DeviceInfo`] they resolved.
async fn build_studio_bridge(
    credentials: DeviceCredentials,
    info: Option<DeviceInfo>,
) -> Result<Box<dyn Bridge>> {
    // Read the Firebase options and Zenoh endpoints from the environment.
    let firebase_options = FirebaseOptions::from_env();
    let firebase_emulator_options = FirebaseEmulatorOptions::from_env();
    // The bridge endpoint is baked into `arora-studio-bridge-client`;
    // `STUDIO_BRIDGE_ENDPOINT` overrides it at runtime to target a local/preprod
    // bridge (e.g. `tcp/localhost:7447`) without a rebuild.
    let endpoint_override = std::env::var("STUDIO_BRIDGE_ENDPOINT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Install the rustls ring crypto provider (the Zenoh/Firebase TLS stacks
    // need a default provider).
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    // The client signs in with the refresh token `credentials` holds and saves
    // each rotation there; over a TLS endpoint it also keeps there the key and
    // certificate the bridge knows the device by, obtained from Studio and
    // renewed while it runs.
    let client = match endpoint_override {
        Some(endpoint) => {
            info!("Connecting to Semio Studio via Zenoh (endpoint: {endpoint})");
            ZenohDeviceClient::new_endpoint_with_credentials(
                &firebase_options,
                Some(&firebase_emulator_options),
                &credentials,
                info,
                endpoint,
            )
            .await
        }
        None => {
            info!("Connecting to Semio Studio via the baked-in bridge endpoint");
            ZenohDeviceClient::new_with_credentials(
                &firebase_options,
                Some(&firebase_emulator_options),
                &credentials,
                info,
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

/// The device info a run resolved, and how.
#[derive(Debug, PartialEq)]
struct Resolved {
    /// What the device registers with.
    info: DeviceInfo,
    /// Whether the saved registration gave any field.
    from_saved: bool,
    /// What to save, when the operator was asked anything.
    answered: Option<Registration>,
}

/// The device info of an interactive run, `env` reading the environment: for
/// each field, the first source that gives it wins, as the
/// [module docs](self#registration) list them; the operator is asked only for
/// what none gives. `None` when the owner resolves empty: Studio is declined.
async fn resolve_with_operator(
    operator: &dyn Operator,
    env: &dyn Fn(&str) -> Option<String>,
    saved: Option<&Registration>,
    description: &HalDescription,
) -> Option<Resolved> {
    let mut asked = false;
    let mut from_saved = false;

    // Owner first. An empty result disables Studio, so short-circuit before
    // asking anything else.
    let owners = match (env("DEVICE_OWNERS"), saved) {
        (Some(owners), _) => parse_owners(&owners),
        (None, Some(saved)) => {
            from_saved = true;
            saved.owners.clone()
        }
        (None, None) => {
            asked = true;
            operator
                .ask_text(OWNER_LABEL, false)
                .await
                .map(|owners| parse_owners(&owners))
                .unwrap_or_default()
        }
    };
    if owners.is_empty() {
        return None;
    }

    // The name always ends up non-empty: generated when left blank.
    let name = match (env("DEVICE_NAME"), saved) {
        (Some(name), _) => name,
        (None, Some(saved)) => {
            from_saved = true;
            saved.name.clone()
        }
        (None, None) => {
            asked = true;
            ask_nonblank(operator, NAME_LABEL)
                .await
                .unwrap_or_else(generated_device_name)
        }
    };

    // The model family: the HAL knows it better than a saved answer. A saved
    // registration answers it even when it holds none: the operator typed none.
    let mut typed_model_family = None;
    let model_family = match (env("MODEL_FAMILY"), hal_field(&description.model_family)) {
        (Some(model_family), _) => Some(model_family),
        (None, Some(model_family)) => {
            info!("studio: model family {model_family}, as the HAL describes the device");
            Some(model_family)
        }
        (None, None) => match saved {
            Some(saved) => {
                from_saved |= saved.model_family.is_some();
                saved.model_family.clone()
            }
            None => {
                asked = true;
                typed_model_family = ask_nonblank(operator, MODEL_FAMILY_LABEL).await;
                typed_model_family.clone()
            }
        },
    };

    // The operator is asked only for what no saved registration gave, so a
    // run that asked anything had none: what it saves is all it resolved.
    let answered = asked.then(|| Registration {
        owners: owners.clone(),
        name: name.clone(),
        model_family: typed_model_family,
    });
    Some(Resolved {
        info: DeviceInfo {
            name: Some(name),
            description: env("DEVICE_DESCRIPTION"),
            model_family,
            hardware_version: env("HARDWARE_VERSION")
                .or_else(|| hal_field(&description.hardware_version)),
            // Software version: env or the HAL — not something an operator types.
            software_version: env("SOFTWARE_VERSION")
                .or_else(|| hal_field(&description.software_version)),
            owners,
        },
        from_saved,
        answered,
    })
}

/// The device info of a run nobody answers for, `env` reading the
/// environment: the environment's, completed by the saved registration and
/// the HAL's `description`. `None` when neither the environment nor a saved
/// registration gives any — the description alone registers nothing, so an
/// existing registration is left untouched.
fn resolve_unattended(
    env: &dyn Fn(&str) -> Option<String>,
    saved: Option<&Registration>,
    description: &HalDescription,
) -> Option<Resolved> {
    let from_env = DeviceInfo {
        name: env("DEVICE_NAME"),
        description: env("DEVICE_DESCRIPTION"),
        model_family: env("MODEL_FAMILY"),
        hardware_version: env("HARDWARE_VERSION"),
        software_version: env("SOFTWARE_VERSION"),
        owners: env("DEVICE_OWNERS")
            .map(|owners| parse_owners(&owners))
            .unwrap_or_default(),
    };
    if from_env == DeviceInfo::default() && saved.is_none() {
        return None;
    }
    let mut from_saved = false;
    let owners = match (from_env.owners.is_empty(), saved) {
        (true, Some(saved)) => {
            from_saved = true;
            saved.owners.clone()
        }
        _ => from_env.owners,
    };
    let name = or_saved(
        from_env.name,
        saved.map(|saved| saved.name.clone()),
        &mut from_saved,
    );
    let model_family = or_saved(
        from_env
            .model_family
            .or_else(|| hal_field(&description.model_family)),
        saved.and_then(|saved| saved.model_family.clone()),
        &mut from_saved,
    );
    Some(Resolved {
        info: DeviceInfo {
            name,
            description: from_env.description,
            model_family,
            hardware_version: from_env
                .hardware_version
                .or_else(|| hal_field(&description.hardware_version)),
            software_version: from_env
                .software_version
                .or_else(|| hal_field(&description.software_version)),
            owners,
        },
        from_saved,
        answered: None,
    })
}

/// `value`, else `saved`, noting in `from_saved` when `saved` gave it.
fn or_saved(value: Option<String>, saved: Option<String>, from_saved: &mut bool) -> Option<String> {
    value.or_else(|| {
        *from_saved |= saved.is_some();
        saved
    })
}

/// The device info a run nobody answers for registers, for the device whose
/// directory is `device_dir`: see [`resolve_unattended`].
fn unattended_info(device_dir: &Path, description: &HalDescription) -> Option<DeviceInfo> {
    let saved_at = registration::path_in(device_dir);
    let saved = registration::load(&saved_at);
    let resolved = resolve_unattended(&env_nonempty, saved.as_ref(), description)?;
    if resolved.from_saved {
        log_saved_use(&resolved.info, &saved_at);
    }
    Some(resolved.info)
}

/// Says the device registers with values saved at `saved_at`, and how to be
/// asked again.
fn log_saved_use(info: &DeviceInfo, saved_at: &Path) {
    info!(
        "studio: registering as {} for owner(s) {} (saved in {}; delete it to answer again)",
        info.name.as_deref().unwrap_or("(no name)"),
        info.owners.join(", "),
        saved_at.display()
    );
}

/// The owner UIDs a comma-separated list names: trimmed, blanks dropped.
fn parse_owners(owners: &str) -> Vec<String> {
    owners
        .split(',')
        .map(|owner| owner.trim().to_string())
        .filter(|owner| !owner.is_empty())
        .collect()
}

/// Ask the operator for one free-text field, not required. A blank answer
/// becomes `None`; the caller supplies any default.
async fn ask_nonblank(operator: &dyn Operator, label: &str) -> Option<String> {
    operator
        .ask_text(label, false)
        .await
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A field of the HAL's description, trimmed, or `None` if absent or blank.
fn hal_field(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use arora_bridge::AccessDecision;

    use super::*;
    use crate::operator::{AccessRequestSummary, AccessRuling};

    /// An operator answering each prompt from a script, keyed by its label,
    /// and recording the labels it was asked.
    #[derive(Default)]
    struct Scripted {
        answers: HashMap<&'static str, &'static str>,
        asked: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn answering(answers: &[(&'static str, &'static str)]) -> Self {
            Scripted {
                answers: answers.iter().copied().collect(),
                ..Scripted::default()
            }
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Operator for Scripted {
        async fn ask_text(&self, label: &str, _required: bool) -> Option<String> {
            self.asked.lock().unwrap().push(label.to_string());
            self.answers.get(label).map(|answer| answer.to_string())
        }

        async fn decide_access(&self, _request: &AccessRequestSummary) -> AccessRuling {
            AccessRuling {
                decision: AccessDecision::Rejected,
                remember: false,
            }
        }
    }

    /// An environment setting `vars`, and nothing else.
    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |key| vars.get(key).cloned()
    }

    fn description(model_family: Option<&str>, version: Option<&str>) -> HalDescription {
        HalDescription {
            model_family: model_family.map(str::to_string),
            hardware_version: version.map(str::to_string),
            software_version: version.map(str::to_string),
        }
    }

    fn saved(model_family: Option<&str>) -> Registration {
        Registration {
            owners: vec!["saved-uid".to_string()],
            name: "saved-name".to_string(),
            model_family: model_family.map(str::to_string),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[tokio::test]
    async fn a_device_registered_once_is_asked_nothing_again() {
        let device =
            std::env::temp_dir().join(format!("arora-studio-asked-once-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&device);
        let saved_at = registration::path_in(&device);
        let no_hal = HalDescription::default();

        // First run: nothing saved, nothing set — the operator answers all.
        let operator = Scripted::answering(&[
            (OWNER_LABEL, " uid-1, uid-2 "),
            (NAME_LABEL, "pepper-lab"),
            (MODEL_FAMILY_LABEL, "pepper"),
        ]);
        let first = resolve_with_operator(&operator, &env(&[]), None, &no_hal)
            .await
            .expect("an owner was given");
        assert_eq!(
            operator.asked(),
            [OWNER_LABEL, NAME_LABEL, MODEL_FAMILY_LABEL]
        );
        assert!(!first.from_saved);
        let answered = first.answered.expect("the answers are to be saved");
        registration::save(&saved_at, &answered).unwrap();

        // Second run: the saved registration answers every question.
        let operator = Scripted::default();
        let saved = registration::load(&saved_at);
        let second = resolve_with_operator(&operator, &env(&[]), saved.as_ref(), &no_hal)
            .await
            .expect("the saved owner registers the device");
        assert!(operator.asked().is_empty(), "asked {:?}", operator.asked());
        assert_eq!(second.info, first.info);
        assert!(second.from_saved);
        assert_eq!(second.answered, None, "nothing new to save");
        std::fs::remove_dir_all(&device).unwrap();
    }

    #[tokio::test]
    async fn a_blank_model_family_answer_is_remembered_as_none() {
        let operator = Scripted::answering(&[(OWNER_LABEL, "uid"), (NAME_LABEL, "")]);
        let first = resolve_with_operator(&operator, &env(&[]), None, &HalDescription::default())
            .await
            .unwrap();
        let answered = first.answered.unwrap();
        assert!(
            answered.name.starts_with("arora-device-"),
            "a blank name is generated, and the generated one saved: {}",
            answered.name
        );
        assert_eq!(answered.model_family, None);

        let operator = Scripted::default();
        let second = resolve_with_operator(
            &operator,
            &env(&[]),
            Some(&answered),
            &HalDescription::default(),
        )
        .await
        .unwrap();
        assert!(operator.asked().is_empty(), "asked {:?}", operator.asked());
        assert_eq!(second.info.name, Some(answered.name));
        assert_eq!(second.info.model_family, None);
    }

    #[tokio::test]
    async fn an_empty_owner_declines_studio_and_saves_nothing() {
        let operator = Scripted::answering(&[(OWNER_LABEL, " , ")]);
        let resolved =
            resolve_with_operator(&operator, &env(&[]), None, &HalDescription::default()).await;
        assert_eq!(resolved, None);
        assert_eq!(operator.asked(), [OWNER_LABEL], "nothing else is asked");
    }

    #[tokio::test]
    async fn the_environment_wins_over_the_saved_registration() {
        let operator = Scripted::default();
        let resolved = resolve_with_operator(
            &operator,
            &env(&[
                ("DEVICE_OWNERS", "env-uid"),
                ("DEVICE_NAME", "env-name"),
                ("MODEL_FAMILY", "env-model"),
            ]),
            Some(&saved(Some("saved-model"))),
            &description(Some("hal-model"), None),
        )
        .await
        .unwrap();
        assert!(operator.asked().is_empty());
        assert_eq!(resolved.info.owners, strings(&["env-uid"]));
        assert_eq!(resolved.info.name.as_deref(), Some("env-name"));
        assert_eq!(resolved.info.model_family.as_deref(), Some("env-model"));
        assert!(!resolved.from_saved);
        assert_eq!(resolved.answered, None);
    }

    #[tokio::test]
    async fn the_hal_gives_the_model_family_and_versions_unless_the_environment_does() {
        // Nothing saved: the HAL's model family spares the operator the
        // question, and is not saved — the HAL gives it again next run.
        let operator = Scripted::answering(&[(OWNER_LABEL, "uid"), (NAME_LABEL, "nao")]);
        let resolved = resolve_with_operator(
            &operator,
            &env(&[("SOFTWARE_VERSION", "env-sw")]),
            None,
            &description(Some("nao"), Some("hal-v")),
        )
        .await
        .unwrap();
        assert_eq!(operator.asked(), [OWNER_LABEL, NAME_LABEL]);
        assert_eq!(resolved.info.model_family.as_deref(), Some("nao"));
        assert_eq!(resolved.info.hardware_version.as_deref(), Some("hal-v"));
        assert_eq!(resolved.info.software_version.as_deref(), Some("env-sw"));
        assert_eq!(resolved.answered.unwrap().model_family, None);

        // Saved: the HAL's model family wins over the saved one.
        let operator = Scripted::default();
        let resolved = resolve_with_operator(
            &operator,
            &env(&[]),
            Some(&saved(Some("saved-model"))),
            &description(Some("pepper"), None),
        )
        .await
        .unwrap();
        assert!(operator.asked().is_empty());
        assert_eq!(resolved.info.model_family.as_deref(), Some("pepper"));
        assert_eq!(resolved.info.owners, strings(&["saved-uid"]));
    }

    #[tokio::test]
    async fn the_operator_is_asked_only_for_what_nothing_gives() {
        let operator = Scripted::answering(&[(NAME_LABEL, "typed-name")]);
        let resolved = resolve_with_operator(
            &operator,
            &env(&[("DEVICE_OWNERS", "env-uid"), ("MODEL_FAMILY", "env-model")]),
            None,
            &HalDescription::default(),
        )
        .await
        .unwrap();
        assert_eq!(operator.asked(), [NAME_LABEL]);
        assert_eq!(
            resolved.answered,
            Some(Registration {
                owners: strings(&["env-uid"]),
                name: "typed-name".to_string(),
                model_family: None,
            }),
            "the resolved owners and name are saved, an env model family is not"
        );
    }

    /// Where the decision tests' device keeps its credential.
    fn studio_dir() -> PathBuf {
        PathBuf::from("/devices/bench/studio")
    }

    /// A lookup finding `credential` in [`studio_dir`].
    fn finding(credential: Credential) -> impl FnOnce() -> Result<(PathBuf, Credential)> {
        move || Ok((studio_dir(), credential))
    }

    /// A lookup the decision must not make.
    fn not_looked_up() -> Result<(PathBuf, Credential)> {
        panic!("the credential is looked up although the run asks for Studio")
    }

    #[test]
    fn no_credential_and_no_flag_serve_the_local_bridge() {
        let decision = decide(Choice::Local, finding(Credential::Absent));
        assert_eq!(decision, Decision::Unregistered(studio_dir()));
        assert!(!decision.connects());
    }

    #[test]
    fn a_saved_credential_alone_serves_the_local_bridge() {
        // It is not proof of an opt-in: arora 13.4 or earlier signed a
        // headless device up unasked.
        let decision = decide(Choice::Local, finding(Credential::Saved));
        assert_eq!(decision, Decision::Saved(studio_dir()));
        assert!(!decision.connects());
    }

    #[test]
    fn a_credential_that_does_not_read_serves_the_local_bridge() {
        let decision = decide(
            Choice::Local,
            finding(Credential::Unreadable("damaged".to_string())),
        );
        assert_eq!(
            decision,
            Decision::Unreadable(studio_dir(), "damaged".to_string())
        );
        assert!(!decision.connects());
    }

    #[test]
    fn a_lookup_that_fails_serves_the_local_bridge() {
        let lookup = || Err(anyhow::anyhow!("no per-user data directory"));
        let decision = decide(Choice::Local, lookup);
        assert_eq!(
            decision,
            Decision::Unknown("no per-user data directory".to_string())
        );
        assert!(!decision.connects());
    }

    #[test]
    fn studio_connects_whatever_the_device_holds() {
        let decision = decide(Choice::Studio, not_looked_up);
        assert_eq!(decision, Decision::Asked);
        assert!(decision.connects());
    }

    #[test]
    fn a_saved_credential_is_a_refresh_token_that_reads() {
        let dir = std::env::temp_dir().join(format!("arora-studio-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let credentials = DeviceCredentials::in_dir(&dir).unwrap();
        assert!(matches!(held(&credentials), Credential::Absent));

        credentials.save_refresh_token("token").unwrap();
        assert!(matches!(held(&credentials), Credential::Saved));

        // The token without the key it is encrypted with does not read.
        std::fs::remove_file(dir.join("key")).unwrap();
        assert!(matches!(held(&credentials), Credential::Unreadable(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A runner's command line, with the Studio options only.
    #[derive(Debug, clap::Parser)]
    struct Runner {
        #[command(flatten)]
        studio: StudioArgs,
    }

    /// What `args` ask, with `ARORA_STUDIO` at `studio_var`.
    fn choice_of(args: &[&str], studio_var: Option<&str>) -> Result<Choice> {
        use clap::Parser;
        Runner::try_parse_from(std::iter::once("runner").chain(args.iter().copied()))
            .expect("the Studio options parse")
            .studio
            .choice_with(studio_var)
    }

    #[test]
    fn the_flags_say_what_the_run_asks_no_studio_winning() {
        let choice = |args: &[&str]| choice_of(args, None).unwrap();
        assert_eq!(choice(&[]), Choice::Local);
        assert_eq!(choice(&["--studio"]), Choice::Studio);
        assert_eq!(choice(&["--no-studio"]), Choice::Local);
        assert_eq!(choice(&["--studio", "--no-studio"]), Choice::Local);
        assert_eq!(choice(&["--no-studio", "--studio"]), Choice::Local);
    }

    #[test]
    fn arora_studio_is_a_boolean_blank_asking_nothing() {
        for yes in ["1", "true", "TRUE", "yes", "On", " 1 "] {
            assert_eq!(
                choice_of(&[], Some(yes)).unwrap(),
                Choice::Studio,
                "{yes:?}"
            );
        }
        for no in ["", "  ", "0", "false", "no", "OFF"] {
            assert_eq!(choice_of(&[], Some(no)).unwrap(), Choice::Local, "{no:?}");
        }
        let error = choice_of(&[], Some("maybe")).unwrap_err().to_string();
        assert!(error.contains("ARORA_STUDIO=\"maybe\""), "{error}");
    }

    #[test]
    fn the_flags_win_over_arora_studio() {
        assert_eq!(
            choice_of(&["--no-studio"], Some("1")).unwrap(),
            Choice::Local
        );
        assert_eq!(
            choice_of(&["--no-studio"], Some("maybe")).unwrap(),
            Choice::Local
        );
        assert_eq!(choice_of(&["--studio"], Some("0")).unwrap(), Choice::Studio);
    }

    #[test]
    fn a_run_asks_nothing_unless_told() {
        assert_eq!(Choice::default(), Choice::Local);
        assert_eq!(crate::Arora::builder().studio, Choice::Local);
        let builder = crate::Arora::builder().with_studio(Choice::Studio);
        assert_eq!(builder.studio, Choice::Studio);
    }

    #[test]
    fn credentials_that_cannot_be_resolved_do_not_read() {
        let credential = resolved(Err(anyhow::anyhow!("IDENTITY_FILE names nothing")));
        assert!(
            matches!(&credential, Credential::Unreadable(error) if error.contains("IDENTITY_FILE")),
            "{credential:?}"
        );
    }

    /// The bridge [`bridge_for`] serves for `decision`: `"studio"` or
    /// `"local"`, as the future it ran says.
    fn bridge_for_(decision: Decision) -> String {
        let studio = async { Err(anyhow::anyhow!("studio")) };
        let local = async { Err(anyhow::anyhow!("local")) };
        let ran = futures::executor::block_on(bridge_for(decision, studio, local));
        ran.err()
            .expect("the futures fail, naming themselves")
            .to_string()
    }

    #[test]
    fn only_a_decision_that_connects_runs_the_studio_connection() {
        assert_eq!(bridge_for_(Decision::Asked), "studio");
        assert_eq!(bridge_for_(Decision::Saved(studio_dir())), "local");
        assert_eq!(bridge_for_(Decision::Unregistered(studio_dir())), "local");
        assert_eq!(
            bridge_for_(Decision::Unknown("no dir".to_string())),
            "local"
        );
        assert_eq!(
            bridge_for_(Decision::Unreadable(studio_dir(), "damaged".to_string())),
            "local"
        );
    }

    #[test]
    fn unattended_nothing_set_nothing_saved_registers_nothing() {
        assert_eq!(
            resolve_unattended(&env(&[]), None, &description(Some("nao"), Some("v"))),
            None,
            "the HAL's description alone registers nothing"
        );
    }

    #[test]
    fn unattended_a_saved_registration_registers_the_device() {
        let resolved = resolve_unattended(
            &env(&[]),
            Some(&saved(Some("saved-model"))),
            &description(None, Some("hal-v")),
        )
        .unwrap();
        assert!(resolved.from_saved);
        assert_eq!(
            resolved.info,
            DeviceInfo {
                name: Some("saved-name".to_string()),
                description: None,
                model_family: Some("saved-model".to_string()),
                hardware_version: Some("hal-v".to_string()),
                software_version: Some("hal-v".to_string()),
                owners: strings(&["saved-uid"]),
            }
        );
    }

    #[test]
    fn unattended_the_environment_then_the_hal_then_the_saved_registration() {
        let resolved = resolve_unattended(
            &env(&[("DEVICE_NAME", "env-name"), ("HARDWARE_VERSION", "env-hw")]),
            Some(&saved(Some("saved-model"))),
            &description(Some("hal-model"), Some("hal-v")),
        )
        .unwrap();
        assert_eq!(
            resolved.info,
            DeviceInfo {
                name: Some("env-name".to_string()),
                description: None,
                model_family: Some("hal-model".to_string()),
                hardware_version: Some("env-hw".to_string()),
                software_version: Some("hal-v".to_string()),
                owners: strings(&["saved-uid"]),
            }
        );
    }

    #[test]
    fn unattended_without_a_saved_registration_is_the_environment_s() {
        let resolved = resolve_unattended(
            &env(&[("DEVICE_OWNERS", "a, b"), ("DEVICE_NAME", "n")]),
            None,
            &HalDescription::default(),
        )
        .unwrap();
        assert!(!resolved.from_saved);
        assert_eq!(
            resolved.info,
            DeviceInfo {
                name: Some("n".to_string()),
                owners: strings(&["a", "b"]),
                ..DeviceInfo::default()
            }
        );
    }
}
