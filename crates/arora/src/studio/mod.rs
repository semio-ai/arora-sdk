//! The Semio Studio connection: what [`crate::run_with_hal`] wires around the
//! runtime loop when the `studio-bridge` feature selects Studio as the bridge
//! — the device's enrollment with Studio, its sign-in and token rotation, the
//! Zenoh connection.
//!
//! This is what makes `arora` Studio-capable (not just its binary):
//! the full run — read Firebase config + Zenoh endpoints from the environment,
//! keep the device's encrypted Studio credential in its device directory,
//! enroll the device when it is not enrolled, build the real
//! `ZenohDeviceClient` (the studio-bridge Zenoh connector), and drive it via
//! [`crate::run_with_frontend`] over the caller's [`DataStore`]
//! (`arora_types::data::DataStore` — the entrypoints default it to a fresh
//! `SimpleDataStore` for devices that do not care).
//!
//! The binary (`src/main.rs`) is a thin wrapper that just calls [`crate::run`].
//!
//! # Whether a run connects
//!
//! Building with Studio is the opt-in: a runner built with it — arora's
//! `studio-bridge` feature for the `arora` binary, a HAL runner's own `studio`
//! feature — connects to Studio by default, and `--no-studio` opts a run out.
//! A build without it has no Studio option at all. What a run does is a
//! [`Choice`], which a runner's command line gives ([`StudioArgs::choice`])
//! and [`AroraBuilder::with_studio`](crate::AroraBuilder::with_studio) hands
//! to [`AroraBuilder::run`](crate::AroraBuilder::run):
//!
//! | the run | the default bridge |
//! |---|---|
//! | passes nothing | Studio; a device not enrolled with Studio [enrolls](#enrollment) first |
//! | passes `--no-studio` | the open local bridge, whatever the device holds |
//!
//! A builder no runner hands a choice serves the open local bridge. A feature
//! of `arora` is enabled for every crate a build links, so `studio-bridge`
//! being on does not tell that the runner at hand was built with Studio: a
//! runner built without it, beside one built with it, would otherwise connect
//! with no `--no-studio` to opt out.
//!
//! The saved credential is the refresh token of the device's Studio session,
//! in its device directory's `studio/` subdirectory ([`crate::device_dir`]).
//! A run says in one line of its log which bridge it serves.
//!
//! The `connect` functions below connect whoever calls them: an embedder
//! calling one connects, a device that is not enrolled enrolling first.
//!
//! # Enrollment
//!
//! Studio creates a device, under the owner a person chooses, when the device
//! enrolls; the device signs in with the credential its enrollment saved from
//! then on. A run that connects enrolls the device when the client finds no
//! enrolled device in its credentials: none, one that does not read, one
//! Studio refuses (a revoked or deleted device), or one that is no device's,
//! such as the anonymous account an earlier arora signed the device up as.
//! It enrolls:
//!
//! - with the enrollment token `STUDIO_ENROLLMENT_TOKEN` holds, when it holds
//!   one: a person created it in Studio (`semio_enroll_…`), and Studio creates
//!   the device under its owner. The token is a secret, read from the
//!   environment at run time only: no option takes it, since a command line
//!   is visible to the host's other users, and the run never logs it.
//! - otherwise by code: the run shows a code, which a person enters in Studio,
//!   choosing the device's owner. Each code is logged, and shown to the
//!   operator under the terminal UI ([`Operator::show_enrollment_code`]);
//!   when no operator shows it and the log shows no info lines — the
//!   headless log's default without `RUST_LOG` — it goes to standard error. It comes with
//!   the Studio page approving it when the client knows Studio's web address
//!   (`STUDIO_URL`, else the one baked into `arora-studio-bridge-client`). A
//!   code that expires undecided is replaced by another; the run waits for a
//!   person's decision for as long as it runs.
//!
//! Then the run connects. A person refusing the code, or Studio refusing the
//! token, fails the run: it connects to Studio, and Studio refused the device.
//! Only a device the client finds not enrolled enrolls: Studio failing
//! otherwise fails the run, the credential kept, and so does a credential of
//! another Firebase project than the run's, which says the two disagree on
//! which Studio the device belongs to.
//!
//! A device can also be enrolled before it runs, with Semio's own client:
//! `semio device enroll` saves the credential in the same device directory.
//!
//! Studio shows the person approving the device its description: its name,
//! `DEVICE_NAME` — under the terminal UI the operator is asked for one when
//! it is unset, and it is generated when left blank — and its model (below).
//! The device's record is Studio's from then on: its name and owner change
//! there, never from the device.
//!
//! # The device's model
//!
//! The device reports its model to its Studio record each time it connects,
//! and describes it when it enrolls: its family, and its hardware and software
//! versions, each from its variable — `MODEL_FAMILY`, `HARDWARE_VERSION`,
//! `SOFTWARE_VERSION` — else from the description of the device's HAL
//! ([`crate::AroraBuilder::run`] passes it). A report writes the fields it
//! gives, the record keeping the others. A device that enrolls with neither
//! giving its family is asked for one under the terminal UI: the family it
//! enrolls with stays its record's, as no later report gives another.
//!
//! # Configuration
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
//!   - `STUDIO_URL` — Studio's web address (`https://…`), overriding the one
//!     baked into `arora-studio-bridge-client`, whose approval page a run
//!     enrolling by code shows beside the code.
//!   - `STUDIO_ENROLLMENT_TOKEN` — the enrollment token a device that is not
//!     enrolled enrolls with, instead of a code.
//!   - `DEVICE_LOCAL_ID` — the device's local id, which names its directory
//!     `<data_local_dir>/semio/arora/devices/<local id>` ([`crate::device_dir`]);
//!     `default` when unset. Two devices on one host set different ids. The
//!     Studio connection keeps its credentials (the refresh token of the
//!     device's Studio session and the key encrypting it, and its bridge key
//!     and certificate) in the directory's `studio/` subdirectory.
//!   - `DEVICE_DIR` — the device directory itself, replacing the per-user
//!     path. An embedder passes it to [`connect_with_device_dir`] instead.
//!   - `IDENTITY_FILE` — deprecated: the identity it names migrates to
//!     `<IDENTITY_FILE>_dir`, which the run uses as its device directory.
//!   - `DEVICE_NAME` — the name a device enrolls with; `MODEL_FAMILY`,
//!     `HARDWARE_VERSION`, `SOFTWARE_VERSION` — its model. A set variable wins
//!     over every other source of its field; a blank one counts as unset.
//!   - `RUST_LOG` — log filter (`env_logger`).
//!
//! [`DataStore`]: arora_types::data::DataStore

mod credentials;

use std::future::Future;
use std::path::Path;

use anyhow::{Context, Result};
use arora_bridge::Bridge;
use arora_hal::HalDescription;
use arora_studio_bridge_client::auth::{DeviceDescription, Error as AuthError, PendingSignIn};
use arora_studio_bridge_client::credentials::DeviceCredentials;
use arora_studio_bridge_client::error::Error as ClientError;
use arora_studio_bridge_client::firestore_support::options::{
    FirebaseEmulatorOptions, FirebaseOptions,
};
use arora_studio_bridge_client::studio::{self as enrollment, EnrollWith, ENROLLMENT_TOKEN_VAR};
use arora_studio_bridge_client::zenoh::ZenohDeviceClient;
use arora_studio_bridge_client::{PartialDeviceModel, PartialDeviceModelVersion};
use log::info;

use crate::operator::{EnrollmentCode, Operator};

/// The prompt for the name of a device that enrolls. Empty falls back to a
/// generated `arora-device-<random>` name, so the device is never nameless.
const NAME_LABEL: &str = "Device name — leave empty for a generated name";

/// The prompt for the model family of a device that enrolls when neither
/// `MODEL_FAMILY` nor the HAL gives one. Empty is fine: the device enrolls
/// with no model family.
const MODEL_FAMILY_LABEL: &str = "Model family — optional, leave empty to skip";

/// What a run does about Semio Studio: the
/// [module docs](self#whether-a-run-connects) say what each gives. A
/// runner's command line gives it with [`StudioArgs::choice`];
/// [`AroraBuilder::with_studio`](crate::AroraBuilder::with_studio) hands it
/// to the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Studio, a device that is not enrolled enrolling first: what a run of a
    /// runner built with Studio does unless it passes `--no-studio`.
    Studio,
    /// `--no-studio`: the open local bridge, whatever the device holds.
    Local,
}

/// The Studio option of a runner built with Studio, as a clap helper that
/// [`DeviceCli`](crate::DeviceCli) and the robot runners flatten:
/// `--no-studio`.
#[derive(Clone, Debug, Default, clap::Args)]
pub struct StudioArgs {
    /// Serve the open local bridge instead of connecting to Semio Studio,
    /// which a run of this build otherwise connects to, enrolling this device
    /// with it when it is not enrolled.
    #[arg(long)]
    no_studio: bool,
}

impl StudioArgs {
    /// What the run does: [`Choice::Local`] with `--no-studio`, else
    /// [`Choice::Studio`].
    pub fn choice(&self) -> Choice {
        if self.no_studio {
            Choice::Local
        } else {
            Choice::Studio
        }
    }
}

/// Resolve the default bridge of a `studio-bridge` run doing `choice`: the
/// Semio Studio connection, or the open local bridge with [`Choice::Local`].
///
/// A device that is not enrolled enrolls as the [module docs](self#enrollment)
/// say: under the terminal UI, the operator names it when `DEVICE_NAME` does
/// not, and is shown each code; headless, nobody is asked, and the log shows
/// the codes. The whole Studio side (enrollment, sign-in, token rotation, the
/// Zenoh connection) is identical for every device — only the hardware behind
/// it, which `description` describes, differs.
pub(crate) async fn default_bridge(
    frontend: &crate::operator::Frontend,
    description: &HalDescription,
    choice: Choice,
) -> Result<Box<dyn Bridge>> {
    bridge_for(
        choice,
        connect_studio(frontend, description),
        crate::run::local_ws_bridge(),
    )
    .await
}

/// The bridge `choice` picks, once the log says which: `studio` for
/// [`Choice::Studio`], `local` for [`Choice::Local`]. Only the one picked
/// runs.
async fn bridge_for(
    choice: Choice,
    studio: impl Future<Output = Result<Box<dyn Bridge>>>,
    local: impl Future<Output = Result<Box<dyn Bridge>>>,
) -> Result<Box<dyn Bridge>> {
    match choice {
        Choice::Studio => {
            info!(
                "Studio: connecting to Semio Studio (--no-studio serves the open local bridge \
                 instead); a device not enrolled with it enrolls first."
            );
            studio.await
        }
        Choice::Local => {
            info!("Studio: serving the open local bridge instead of Semio Studio.");
            local.await
        }
    }
}

/// The Studio connection of a run that connects, through the operator only
/// under the terminal UI: headless, nobody answers.
async fn connect_studio(
    frontend: &crate::operator::Frontend,
    description: &HalDescription,
) -> Result<Box<dyn Bridge>> {
    let operator = frontend.interactive.then_some(&*frontend.operator);
    connect_device(credentials::from_env()?, description, operator).await
}

/// Build the Studio bridge of the device the environment configures,
/// enrolling it first when it is not enrolled, through `operator`: it is
/// asked the device's name when `DEVICE_NAME` does not give one, and shown
/// each code a person approves the device with, as the
/// [module docs](self#enrollment) say. Fails when the enrollment is refused.
///
/// This is the operator-driven counterpart to [`connect`] (which never asks,
/// for embedders that have no operator). The bridge itself is built the same
/// way; only how a device that enrolls is named, and how its code is shown,
/// differ.
pub async fn connect_with_operator(operator: &dyn Operator) -> Result<Box<dyn Bridge>> {
    connect_with_operator_and_description(operator, &HalDescription::default()).await
}

/// [`connect_with_operator`], with the device's model completed by what its
/// HAL says of itself, as the [module docs](self#the-devices-model) say. Pass
/// `hal.describe().await`.
pub async fn connect_with_operator_and_description(
    operator: &dyn Operator,
    description: &HalDescription,
) -> Result<Box<dyn Bridge>> {
    connect_device(credentials::from_env()?, description, Some(operator)).await
}

/// Build the Semio Studio bridge — a ready-to-inject [`Bridge`] endpoint — of
/// the device the environment configures, enrolling it first when it is not
/// enrolled, and return it.
///
/// This is the injectable counterpart to [`crate::run_with_hal`]: where `run_with_hal`
/// builds the bridge *and* owns the run loop, `connect` returns just the
/// finished, connected bridge so an embedder can attach it to a device with
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
/// The whole Studio side (enrollment, sign-in, token rotation, the Zenoh
/// connection) is identical for every device; only the runtime it is attached
/// to differs. Nobody is asked anything: a device that enrolls is named by
/// `DEVICE_NAME`, or generated, and the log shows the codes it enrolls by —
/// see the [module docs](self#enrollment). Fails when the enrollment is
/// refused.
pub async fn connect() -> Result<Box<dyn Bridge>> {
    connect_with_description(&HalDescription::default()).await
}

/// [`connect`], with the device's model completed by what its HAL says of
/// itself (`hal.describe().await`), as the
/// [module docs](self#the-devices-model) say.
pub async fn connect_with_description(description: &HalDescription) -> Result<Box<dyn Bridge>> {
    connect_device(credentials::from_env()?, description, None).await
}

/// [`connect`], with `device_dir` as the device directory rather than the one
/// the environment names; the Studio credentials go in its `studio/`
/// subdirectory. An app whose platform gives it a data directory (Android, a
/// Tauri app) passes that; [`crate::device_dir::of`] gives the per-user one
/// of a local id. The directories are created if missing, and the
/// credentials when the device enrolls.
pub async fn connect_with_device_dir(device_dir: &Path) -> Result<Box<dyn Bridge>> {
    connect_device(
        credentials::in_device_dir(device_dir)?,
        &HalDescription::default(),
        None,
    )
    .await
}

// The connection futures are `Send`, so an embedder can await them in a spawned
// task: whatever they hold across an await is `Send`, and a reference to it
// `Sync`.
const _: () = {
    fn send<T: Send>(_: &T) {}
    #[allow(dead_code)]
    fn connection_futures_are_send(
        operator: &dyn Operator,
        description: &HalDescription,
        device_dir: &Path,
    ) {
        send(&connect());
        send(&connect_with_description(description));
        send(&connect_with_device_dir(device_dir));
        send(&connect_with_operator(operator));
        send(&connect_with_operator_and_description(
            operator,
            description,
        ));
    }
};

/// Build the Studio bridge of the device whose Studio credentials
/// `credentials` holds, reporting its model as `description` completes it,
/// and enrolling it first through `operator`, if any, when it is not
/// enrolled.
async fn connect_device(
    credentials: DeviceCredentials,
    description: &HalDescription,
    operator: Option<&dyn Operator>,
) -> Result<Box<dyn Bridge>> {
    // Install the rustls ring crypto provider (the Zenoh/Firebase TLS stacks
    // need a default provider).
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let studio = &Studio::from_env();
    let credentials = &credentials;
    let model = &Model::of(&env_nonempty, description);
    let client = enrolled(
        credentials.path(),
        || studio.connect(credentials, model.reported()),
        |why| async move {
            info!("Studio: this device is not enrolled with Semio Studio ({why}); enrolling it");
            let device = enrolling_device(&env_nonempty, model, operator).await;
            studio.enroll(credentials, &device, operator).await
        },
    )
    .await?;
    Ok(Box::new(client))
}

/// The client of the enrolled device whose Studio credentials are in
/// `credentials`: `connect`'s when the device is enrolled; otherwise
/// `connect`'s once `enroll`, told why the device is not enrolled, has
/// enrolled it.
///
/// Only a device the client finds not enrolled enrolls. Studio failing
/// otherwise says nothing of the device's enrollment, and a credential of
/// another Firebase project than the run's means the two disagree on which
/// Studio the device belongs to: enrolling would replace a credential that
/// may be good.
async fn enrolled<C, Connected, Enrolled>(
    credentials: &Path,
    mut connect: impl FnMut() -> Connected,
    enroll: impl FnOnce(String) -> Enrolled,
) -> Result<C>
where
    Connected: Future<Output = Result<C, ClientError>>,
    Enrolled: Future<Output = Result<()>>,
{
    let failed = |error| connection_error(error, credentials);
    match connect().await {
        Err(ClientError::NotEnrolled(why)) => {
            enroll(why).await?;
            connect().await.map_err(failed)
        }
        connected => connected.map_err(failed),
    }
}

/// `error`, connecting the device whose Studio credentials are in
/// `credentials`, with what to do when the credential is another Firebase
/// project's.
fn connection_error(error: ClientError, credentials: &Path) -> anyhow::Error {
    let context = match &error {
        ClientError::Studio(AuthError::WrongProject { .. }) => format!(
            "the device's Studio credential in {} is another Firebase project's than the one \
             this run reaches, so the device does not enroll again over it: point the run at \
             that project (the FIREBASE_* variables), or delete that directory to enroll the \
             device with this one",
            credentials.display()
        ),
        _ => "failed to connect to Semio Studio".to_string(),
    };
    anyhow::Error::new(error).context(context)
}

/// The Studio a run reaches, as the environment configures it.
struct Studio {
    firebase: FirebaseOptions,
    emulators: FirebaseEmulatorOptions,
    /// `STUDIO_BRIDGE_ENDPOINT`: the bridge endpoint, overriding the one baked
    /// into `arora-studio-bridge-client` to target a local or preproduction
    /// bridge (e.g. `tcp/localhost:7447`) without a rebuild.
    endpoint: Option<String>,
}

impl Studio {
    fn from_env() -> Self {
        Studio {
            firebase: FirebaseOptions::from_env(),
            emulators: FirebaseEmulatorOptions::from_env(),
            endpoint: env_nonempty("STUDIO_BRIDGE_ENDPOINT"),
        }
    }

    /// Connect the device `credentials` holds enrolled, reporting `model`.
    /// The client signs it in with the refresh token there and saves each
    /// rotation there; over a TLS endpoint it also keeps there the key and
    /// certificate the bridge knows the device by, obtained from Studio and
    /// renewed while it runs.
    async fn connect(
        &self,
        credentials: &DeviceCredentials,
        model: Option<PartialDeviceModel>,
    ) -> Result<ZenohDeviceClient, ClientError> {
        match &self.endpoint {
            Some(endpoint) => {
                info!("Connecting to Semio Studio via Zenoh (endpoint: {endpoint})");
                ZenohDeviceClient::new_endpoint(
                    &self.firebase,
                    Some(&self.emulators),
                    credentials,
                    model,
                    endpoint.clone(),
                )
                .await
            }
            None => {
                info!("Connecting to Semio Studio via the baked-in bridge endpoint");
                ZenohDeviceClient::new(&self.firebase, Some(&self.emulators), credentials, model)
                    .await
            }
        }
    }

    /// Enroll `device` with Studio, saving its credential in `credentials`:
    /// with the enrollment token `STUDIO_ENROLLMENT_TOKEN` holds, else by
    /// code, each code shown in the log and to `operator`.
    async fn enroll(
        &self,
        credentials: &DeviceCredentials,
        device: &DeviceDescription,
        operator: Option<&dyn Operator>,
    ) -> Result<()> {
        let enrolled = match EnrollWith::token_from_env() {
            Some(token) => {
                info!(
                    "Studio: enrolling this device as {} with the enrollment token in \
                     {ENROLLMENT_TOKEN_VAR}",
                    device.name
                );
                enrollment::enroll(
                    &self.firebase,
                    Some(&self.emulators),
                    credentials,
                    device,
                    token,
                )
                .await
            }
            None => {
                let codes = Codes { operator };
                let origin = enrollment::origin();
                let show = |pending: &PendingSignIn| {
                    codes.show(&enrollment_code(pending, origin.as_deref()));
                };
                enrollment::enroll(
                    &self.firebase,
                    Some(&self.emulators),
                    credentials,
                    device,
                    EnrollWith::Code(Box::new(show)),
                )
                .await
            }
        };
        let device_id = enrolled.context("failed to enroll the device with Semio Studio")?;
        info!("Studio: enrolled as device {device_id}");
        Ok(())
    }
}

/// The code `pending` asks a person to approve, with its approval page on the
/// Studio served at `origin`, when it is known.
fn enrollment_code(pending: &PendingSignIn, origin: Option<&str>) -> EnrollmentCode {
    EnrollmentCode {
        code: pending.user_code().to_string(),
        approval_url: origin.map(|origin| pending.approval_url(origin)),
    }
}

/// Shows the codes of an enrollment by code: each in the log, and to the
/// operator, if any, whose code is cleared once the enrollment ends — or once
/// it is dropped, which would otherwise leave the operator showing a code
/// nobody waits on.
struct Codes<'a> {
    operator: Option<&'a dyn Operator>,
}

impl Codes<'_> {
    fn show(&self, code: &EnrollmentCode) {
        self.show_on(
            code,
            log::log_enabled!(log::Level::Info),
            &mut std::io::stderr(),
        );
    }

    /// [`show`](Self::show), with `logged` whether the log shows info lines,
    /// and `console` where a code goes that nothing else shows. The run waits
    /// on a person entering the code, so someone must see it: when no
    /// operator shows it, and the log shows no info lines — the headless
    /// log's default without `RUST_LOG` — it goes to `console`.
    fn show_on(&self, code: &EnrollmentCode, logged: bool, console: &mut dyn std::io::Write) {
        let line = code_line(code);
        info!("{line}");
        let shown = self
            .operator
            .is_some_and(|operator| operator.show_enrollment_code(Some(code)));
        if !shown && !logged {
            let _ = writeln!(console, "{line}");
        }
    }
}

impl Drop for Codes<'_> {
    fn drop(&mut self) {
        if let Some(operator) = self.operator {
            operator.show_enrollment_code(None);
        }
    }
}

/// The log line showing `code`.
fn code_line(code: &EnrollmentCode) -> String {
    match &code.approval_url {
        Some(url) => format!(
            "Studio: to enroll this device, enter the code {} in Semio Studio, or open {url}",
            code.code
        ),
        None => format!(
            "Studio: to enroll this device, enter the code {} in Semio Studio",
            code.code
        ),
    }
}

/// The device's model: each field from its variable, else from the HAL's
/// description.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Model {
    family: Option<String>,
    hardware: Option<String>,
    software: Option<String>,
}

impl Model {
    /// The model `env` and the HAL's `description` give, the variable
    /// winning each field.
    fn of(env: &dyn Fn(&str) -> Option<String>, description: &HalDescription) -> Self {
        Model {
            family: env("MODEL_FAMILY").or_else(|| hal_field(&description.model_family)),
            hardware: env("HARDWARE_VERSION").or_else(|| hal_field(&description.hardware_version)),
            software: env("SOFTWARE_VERSION").or_else(|| hal_field(&description.software_version)),
        }
    }

    /// What the device reports to its Studio record: the fields it gives,
    /// the record keeping the others; `None` when it gives none.
    fn reported(&self) -> Option<PartialDeviceModel> {
        if *self == Model::default() {
            return None;
        }
        let version = (self.hardware.is_some() || self.software.is_some()).then(|| {
            PartialDeviceModelVersion {
                hardware: self.hardware.clone(),
                software: self.software.clone(),
            }
        });
        Some(PartialDeviceModel {
            family: self.family.clone(),
            version,
        })
    }

    /// The description of the device named `name` that enrolls, which the
    /// person approving it sees.
    fn described(&self, name: String) -> DeviceDescription {
        let mut device =
            DeviceDescription::new(name).with_version(concat!("arora ", env!("CARGO_PKG_VERSION")));
        if let Some(family) = &self.family {
            device = device.with_family(family);
        }
        if let Some(hardware) = &self.hardware {
            device = device.with_hardware(hardware);
        }
        if let Some(software) = &self.software {
            device = device.with_software(software);
        }
        device
    }
}

/// What a device that enrolls describes itself as to the person approving
/// it: its name — `DEVICE_NAME` in `env`, else what `operator`, if any,
/// answers, else a generated one — and `model`, whose family, when nothing
/// gives one, `operator` is asked for.
async fn enrolling_device(
    env: &(dyn Fn(&str) -> Option<String> + Sync),
    model: &Model,
    operator: Option<&dyn Operator>,
) -> DeviceDescription {
    let name = match env("DEVICE_NAME") {
        Some(name) => name,
        None => ask(operator, NAME_LABEL)
            .await
            .unwrap_or_else(generated_device_name),
    };
    let family = match &model.family {
        Some(family) => Some(family.clone()),
        None => ask(operator, MODEL_FAMILY_LABEL).await,
    };
    Model {
        family,
        ..model.clone()
    }
    .described(name)
}

/// What `operator`, if any, answers `label`, not required: `None` when it
/// skips it, answers blank, or there is no operator.
async fn ask(operator: Option<&dyn Operator>, label: &str) -> Option<String> {
    match operator {
        Some(operator) => ask_nonblank(operator, label).await,
        None => None,
    }
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

/// A generated device name, `arora-device-<random>`, for a device that
/// enrolls with no name given — so a device is never nameless.
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
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use arora_bridge::AccessDecision;

    use super::*;
    use crate::operator::{AccessRequestSummary, AccessRuling};

    /// An operator answering each prompt from a script, keyed by its label,
    /// and recording the labels it was asked and the codes it was shown.
    #[derive(Default)]
    struct Scripted {
        answers: HashMap<&'static str, &'static str>,
        asked: Mutex<Vec<String>>,
        shown: Mutex<Vec<Option<EnrollmentCode>>>,
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

        fn shown(&self) -> Vec<Option<EnrollmentCode>> {
            self.shown.lock().unwrap().clone()
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

        fn show_enrollment_code(&self, code: Option<&EnrollmentCode>) -> bool {
            self.shown.lock().unwrap().push(code.cloned());
            true
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

    fn code(code: &str, approval_url: Option<&str>) -> EnrollmentCode {
        EnrollmentCode {
            code: code.to_string(),
            approval_url: approval_url.map(str::to_string),
        }
    }

    /// Where the enrollment tests' device keeps its Studio credentials.
    fn credentials_dir() -> PathBuf {
        PathBuf::from("/devices/bench/studio")
    }

    /// What [`enrolled`] gives when the client's connections answer
    /// `connections`, in turn, and its enrollment answers `enrollment`; with
    /// the reasons the enrollment was given, and how many connections were
    /// made.
    fn enrolled_with(
        connections: Vec<Result<&'static str, ClientError>>,
        enrollment: Result<()>,
    ) -> (Result<&'static str>, Vec<String>, usize) {
        let connections = RefCell::new(connections.into_iter());
        let made = RefCell::new(0);
        let reasons = RefCell::new(Vec::new());
        let client = futures::executor::block_on(enrolled(
            &credentials_dir(),
            || {
                *made.borrow_mut() += 1;
                let connection = connections.borrow_mut().next();
                async move { connection.expect("no connection is made past those scripted") }
            },
            |why| {
                reasons.borrow_mut().push(why);
                async move { enrollment }
            },
        ));
        (client, reasons.into_inner(), made.into_inner())
    }

    #[test]
    fn an_enrolled_device_connects_without_enrolling() {
        let (client, reasons, made) = enrolled_with(vec![Ok("client")], Ok(()));
        assert_eq!(client.unwrap(), "client");
        assert!(reasons.is_empty(), "enrolled for {reasons:?}");
        assert_eq!(made, 1);
    }

    #[test]
    fn a_device_not_enrolled_enrolls_then_connects() {
        let (client, reasons, made) = enrolled_with(
            vec![
                Err(ClientError::NotEnrolled(
                    "it holds no credential".to_string(),
                )),
                Ok("client"),
            ],
            Ok(()),
        );
        assert_eq!(client.unwrap(), "client");
        assert_eq!(reasons, ["it holds no credential"]);
        assert_eq!(made, 2);
    }

    #[test]
    fn a_refused_enrollment_fails_and_does_not_connect_again() {
        let (client, _, made) = enrolled_with(
            vec![Err(ClientError::NotEnrolled("anonymous".to_string()))],
            Err(anyhow::Error::new(ClientError::EnrollmentDenied)),
        );
        let error = format!("{:#}", client.unwrap_err());
        assert!(error.contains("refused"), "{error}");
        assert_eq!(made, 1);
    }

    #[test]
    fn a_device_its_enrollment_leaves_unenrolled_fails() {
        let (client, reasons, made) = enrolled_with(
            vec![
                Err(ClientError::NotEnrolled(
                    "it holds no credential".to_string(),
                )),
                Err(ClientError::NotEnrolled("Studio refuses it".to_string())),
            ],
            Ok(()),
        );
        let error = format!("{:#}", client.unwrap_err());
        assert!(error.contains("Studio refuses it"), "{error}");
        assert_eq!(reasons.len(), 1, "a device enrolls once per run");
        assert_eq!(made, 2);
    }

    #[test]
    fn studio_failing_otherwise_fails_without_enrolling() {
        // Firebase down or a bridge out of reach says nothing of the device's
        // enrollment: enrolling again would replace a credential that is good.
        let (client, reasons, _) = enrolled_with(
            vec![Err(ClientError::Zenoh("no route".to_string()))],
            Ok(()),
        );
        let error = format!("{:#}", client.unwrap_err());
        assert!(error.contains("no route"), "{error}");
        assert!(reasons.is_empty(), "enrolled for {reasons:?}");
    }

    #[test]
    fn a_credential_of_another_project_fails_without_enrolling() {
        let wrong_project = AuthError::WrongProject {
            project: "studio".to_string(),
            audience: "studio-staging".to_string(),
        };
        let (client, reasons, made) =
            enrolled_with(vec![Err(ClientError::Studio(wrong_project))], Ok(()));
        let error = client.unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<ClientError>(),
                Some(ClientError::Studio(AuthError::WrongProject { .. }))
            ),
            "{error:?}"
        );
        let said = format!("{error:#}");
        assert!(said.contains("another Firebase project's"), "{said}");
        assert!(
            said.contains(&credentials_dir().display().to_string()),
            "{said}"
        );
        assert!(said.contains("studio-staging"), "{said}");
        assert!(reasons.is_empty(), "enrolled for {reasons:?}");
        assert_eq!(made, 1);
    }

    #[test]
    fn each_code_is_shown_then_cleared_once_the_enrollment_ends() {
        let operator = Scripted::default();
        let codes = Codes {
            operator: Some(&operator),
        };
        codes.show(&code("BCDF-GHJK", None));
        codes.show(&code("LMNP-QRST", Some("https://studio.example/approve")));
        assert_eq!(operator.shown().len(), 2);
        drop(codes);
        assert_eq!(
            operator.shown(),
            [
                Some(code("BCDF-GHJK", None)),
                Some(code("LMNP-QRST", Some("https://studio.example/approve"))),
                None
            ]
        );
    }

    #[test]
    fn a_code_nothing_else_shows_goes_to_the_console() {
        // The headless front end's log shows errors alone without RUST_LOG.
        let mut console = Vec::new();
        Codes { operator: None }.show_on(&code("BCDF-GHJK", None), false, &mut console);
        assert_eq!(
            String::from_utf8(console).unwrap(),
            "Studio: to enroll this device, enter the code BCDF-GHJK in Semio Studio\n"
        );

        // Shown by the log, or by the operator, it is not repeated there.
        let mut console = Vec::new();
        Codes { operator: None }.show_on(&code("BCDF-GHJK", None), true, &mut console);
        let operator = Scripted::default();
        Codes {
            operator: Some(&operator),
        }
        .show_on(&code("BCDF-GHJK", None), false, &mut console);
        assert!(console.is_empty(), "{:?}", String::from_utf8(console));
        assert_eq!(
            operator.shown().first(),
            Some(&Some(code("BCDF-GHJK", None)))
        );
    }

    #[test]
    fn a_code_an_operator_does_not_show_goes_to_the_console() {
        /// An operator keeping the default enrollment-code hook.
        struct Silent;

        #[async_trait]
        impl Operator for Silent {
            async fn ask_text(&self, _label: &str, _required: bool) -> Option<String> {
                None
            }

            async fn decide_access(&self, _request: &AccessRequestSummary) -> AccessRuling {
                AccessRuling {
                    decision: AccessDecision::Rejected,
                    remember: false,
                }
            }
        }

        let mut console = Vec::new();
        Codes {
            operator: Some(&Silent),
        }
        .show_on(&code("BCDF-GHJK", None), false, &mut console);
        assert_eq!(
            String::from_utf8(console).unwrap(),
            "Studio: to enroll this device, enter the code BCDF-GHJK in Semio Studio\n"
        );
    }

    #[test]
    fn a_code_line_names_the_approval_page_when_studio_s_address_is_known() {
        assert_eq!(
            code_line(&code("BCDF-GHJK", None)),
            "Studio: to enroll this device, enter the code BCDF-GHJK in Semio Studio"
        );
        assert_eq!(
            code_line(&code(
                "BCDF-GHJK",
                Some("https://studio.example/approve?code=BCDF-GHJK")
            )),
            "Studio: to enroll this device, enter the code BCDF-GHJK in Semio Studio, or open \
             https://studio.example/approve?code=BCDF-GHJK"
        );
    }

    #[tokio::test]
    async fn what_describes_the_device_is_not_asked() {
        let operator = Scripted::answering(&[(NAME_LABEL, "typed"), (MODEL_FAMILY_LABEL, "typed")]);
        let device = enrolling_device(
            &env(&[("DEVICE_NAME", "pepper-lab")]),
            &Model::of(&env(&[]), &description(Some("nao"), None)),
            Some(&operator),
        )
        .await;
        assert_eq!(device.name, "pepper-lab");
        assert_eq!(device.family.as_deref(), Some("nao"));
        assert!(operator.asked().is_empty(), "asked {:?}", operator.asked());
    }

    #[tokio::test]
    async fn the_operator_names_a_device_and_its_family_nothing_else_gives() {
        let operator = Scripted::answering(&[
            (NAME_LABEL, " pepper-lab "),
            (MODEL_FAMILY_LABEL, " pepper "),
        ]);
        let device = enrolling_device(&env(&[]), &Model::default(), Some(&operator)).await;
        assert_eq!(device.name, "pepper-lab");
        assert_eq!(device.family.as_deref(), Some("pepper"));
        assert_eq!(operator.asked(), [NAME_LABEL, MODEL_FAMILY_LABEL]);

        // Left blank, or with nobody to ask, the name is generated, and the
        // device enrolls with no family.
        let operator = Scripted::answering(&[(NAME_LABEL, " ")]);
        let blank = enrolling_device(&env(&[]), &Model::default(), Some(&operator)).await;
        assert!(blank.name.starts_with("arora-device-"), "{}", blank.name);
        assert_eq!(blank.family, None);
        let headless = enrolling_device(&env(&[]), &Model::default(), None).await;
        assert!(
            headless.name.starts_with("arora-device-"),
            "{}",
            headless.name
        );
        assert_eq!(headless.family, None);
    }

    #[test]
    fn each_model_field_is_its_variable_s_else_the_hal_s() {
        let model = Model::of(
            &env(&[
                ("MODEL_FAMILY", "env-family"),
                ("SOFTWARE_VERSION", "env-sw"),
            ]),
            &description(Some("nao"), Some("hal-v")),
        );
        assert_eq!(
            model,
            Model {
                family: Some("env-family".to_string()),
                hardware: Some("hal-v".to_string()),
                software: Some("env-sw".to_string()),
            }
        );
        assert_eq!(
            model.reported(),
            Some(PartialDeviceModel {
                family: Some("env-family".to_string()),
                version: Some(PartialDeviceModelVersion {
                    hardware: Some("hal-v".to_string()),
                    software: Some("env-sw".to_string()),
                }),
            })
        );
    }

    #[test]
    fn a_report_carries_only_the_fields_given() {
        let model = Model::of(&env(&[]), &description(Some(" "), None));
        assert_eq!(model, Model::default());
        assert_eq!(model.reported(), None);
        let versions_only = Model::of(&env(&[("HARDWARE_VERSION", "6")]), &description(None, None));
        assert_eq!(
            versions_only.reported(),
            Some(PartialDeviceModel {
                family: None,
                version: Some(PartialDeviceModelVersion {
                    hardware: Some("6".to_string()),
                    software: None,
                }),
            })
        );
    }

    #[test]
    fn a_device_enrolls_described_by_its_name_and_model() {
        let device = Model::of(&env(&[]), &description(Some("nao"), Some("6")))
            .described("pepper-lab".to_string());
        assert_eq!(device.name, "pepper-lab");
        assert_eq!(device.family.as_deref(), Some("nao"));
        assert_eq!(device.hardware.as_deref(), Some("6"));
        assert_eq!(device.software.as_deref(), Some("6"));
        assert_eq!(
            device.version.as_deref(),
            Some(concat!("arora ", env!("CARGO_PKG_VERSION")))
        );
    }

    /// A runner's command line, with the Studio option only.
    #[derive(Debug, clap::Parser)]
    struct Runner {
        #[command(flatten)]
        studio: StudioArgs,
    }

    /// What a run passing `args` does.
    fn choice_of(args: &[&str]) -> Choice {
        use clap::Parser;
        Runner::try_parse_from(std::iter::once("runner").chain(args.iter().copied()))
            .expect("the Studio option parses")
            .studio
            .choice()
    }

    #[test]
    fn a_run_connects_unless_it_passes_no_studio() {
        assert_eq!(choice_of(&[]), Choice::Studio);
        assert_eq!(choice_of(&["--no-studio"]), Choice::Local);
    }

    #[test]
    fn studio_is_no_option() {
        use clap::Parser;
        let error =
            Runner::try_parse_from(["runner", "--studio"]).expect_err("--studio is refused");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn a_builder_connects_only_when_a_runner_says_so() {
        assert_eq!(crate::Arora::builder().studio, None);
        let builder = crate::Arora::builder().with_studio(Choice::Studio);
        assert_eq!(builder.studio, Some(Choice::Studio));
    }

    /// The bridge [`bridge_for`] serves for `choice`: `"studio"` or
    /// `"local"`, as the future it ran says.
    fn bridge_for_(choice: Choice) -> String {
        let studio = async { Err(anyhow::anyhow!("studio")) };
        let local = async { Err(anyhow::anyhow!("local")) };
        let ran = futures::executor::block_on(bridge_for(choice, studio, local));
        ran.err()
            .expect("the futures fail, naming themselves")
            .to_string()
    }

    #[test]
    fn only_studio_runs_the_studio_connection() {
        assert_eq!(bridge_for_(Choice::Studio), "studio");
        assert_eq!(bridge_for_(Choice::Local), "local");
    }
}
