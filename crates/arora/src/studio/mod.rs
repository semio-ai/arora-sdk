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
//! Studio is opt-in: a run contacts it only when it asks for it. What a run
//! asks is a [`Choice`], which a runner's command line gives ([`StudioArgs`]:
//! `--studio`, `--no-studio`) and
//! [`AroraBuilder::with_studio`](crate::AroraBuilder::with_studio) hands to
//! [`AroraBuilder::run`](crate::AroraBuilder::run):
//!
//! | the run asks | the default bridge |
//! |---|---|
//! | `--studio`, or `ARORA_STUDIO=1` | Studio; a device not enrolled with Studio [enrolls](#enrollment) first |
//! | nothing, or `--no-studio` | the open local bridge, whatever the device holds |
//!
//! `--no-studio` wins over `--studio` and `ARORA_STUDIO`: a run told both
//! declines, and a run passing it overrides the variable.
//!
//! The saved credential is the refresh token of the device's Studio session,
//! in its device directory's `studio/` subdirectory ([`crate::device_dir`]).
//! It does not connect a run that does not ask: a saved credential is not
//! proof of an opt-in, as a headless run of arora 13.4 or earlier signed the
//! device up unasked. A run says what it decided in one line of its log; one
//! that does not ask looks the credential up only to say so, pointing a
//! device holding one to `--studio`, and serves the local bridge whatever the
//! lookup finds.
//!
//! The `connect` functions below are themselves the ask: an embedder calling
//! one connects, a device that is not enrolled enrolling first.
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
//!   with no operator and a log showing no info lines — the headless log's
//!   default without `RUST_LOG` — it goes to standard error. It comes with
//!   the Studio page approving it when the client knows Studio's web address
//!   (`STUDIO_URL`, else the one baked into `arora-studio-bridge-client`). A
//!   code that expires undecided is replaced by another; the run waits for a
//!   person's decision for as long as it runs.
//!
//! Then the run connects. A person refusing the code, or Studio refusing the
//! token, fails the run: it asked for Studio, and Studio refused the device.
//! Only a device the client finds not enrolled enrolls: Studio failing
//! otherwise fails the run, the credential kept, and so does a credential of
//! another Firebase project than the run's, which says the two disagree on
//! which Studio the device belongs to.
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
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use arora_bridge::Bridge;
use arora_hal::HalDescription;
use arora_studio_bridge_client::credentials::DeviceCredentials;
use arora_studio_bridge_client::error::Error as ClientError;
use arora_studio_bridge_client::firestore_support::options::{
    FirebaseEmulatorOptions, FirebaseOptions,
};
use arora_studio_bridge_client::session::{
    DeviceDescription, Error as SessionError, PendingSignIn,
};
use arora_studio_bridge_client::studio::{self as enrollment, EnrollWith, ENROLLMENT_TOKEN_VAR};
use arora_studio_bridge_client::zenoh::ZenohDeviceClient;
use arora_studio_bridge_client::{PartialDeviceModel, PartialDeviceModelVersion};
use log::{info, warn};

use crate::device_dir;
use crate::operator::{EnrollmentCode, Operator};

/// The prompt for the name of a device that enrolls. Empty falls back to a
/// generated `arora-device-<random>` name, so the device is never nameless.
const NAME_LABEL: &str = "Device name — leave empty for a generated name";

/// The prompt for the model family of a device that enrolls when neither
/// `MODEL_FAMILY` nor the HAL gives one. Empty is fine: the device enrolls
/// with no model family.
const MODEL_FAMILY_LABEL: &str = "Model family — optional, leave empty to skip";

/// What a run asks of Semio Studio: the [module docs](self#whether-a-run-connects)
/// say what each gives. A runner's command line gives it with
/// [`StudioArgs::choice`]; [`AroraBuilder::with_studio`](crate::AroraBuilder::with_studio)
/// hands it to the run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    /// `--studio`: Studio, a device that is not enrolled enrolling first.
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
    /// Connect to Semio Studio, enrolling this device with it when it is not
    /// enrolled; ARORA_STUDIO=1 asks the same. Without either, the run serves
    /// the open local bridge.
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
/// Studio session, which a connection signs it in with.
#[derive(Debug)]
enum Credential {
    /// One that reads.
    Saved,
    /// None: a connection would enroll the device.
    Absent,
    /// One that does not read (its key missing or another, the file damaged):
    /// a connection would enroll the device anew, as another Studio device.
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
                "Studio: connecting, as --studio asks; a device not enrolled with Semio Studio \
                 enrolls first."
            ),
            Decision::Unregistered(dir) => info!(
                "Studio: no saved credential in {}; serving the local bridge. Pass --studio to \
                 enroll this device with Semio Studio.",
                dir.display()
            ),
            Decision::Saved(dir) => info!(
                "Studio: serving the local bridge, as the run does not ask for Studio. Pass \
                 --studio to connect with the saved credential in {}.",
                dir.display()
            ),
            Decision::Unreadable(dir, error) => warn!(
                "Studio: the saved credential in {} does not read ({error}); serving the local \
                 bridge. Pass --studio to enroll this device with Semio Studio anew.",
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
/// the client signs in with; a device holding none, or one that does not
/// read, enrolls.
fn held(credentials: &DeviceCredentials) -> Credential {
    match credentials.refresh_token() {
        Ok(Some(_)) => Credential::Saved,
        Ok(None) => Credential::Absent,
        Err(error) => Credential::Unreadable(error.to_string()),
    }
}

/// Resolve the default bridge of a `studio-bridge` run asking `choice`: the
/// Semio Studio connection, or the open local bridge when the run does not
/// connect ([module docs](self#whether-a-run-connects)).
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
    studio: impl Future<Output = Result<Box<dyn Bridge>>>,
    local: impl Future<Output = Result<Box<dyn Bridge>>>,
) -> Result<Box<dyn Bridge>> {
    decision.log();
    if decision.connects() {
        studio.await
    } else {
        local.await
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
        ClientError::Studio(SessionError::WrongProject { .. }) => format!(
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
    /// on a person entering the code, so someone must see it: with no
    /// operator, and a log showing no info lines — the headless log's default
    /// without `RUST_LOG` — it goes to `console`.
    fn show_on(&self, code: &EnrollmentCode, logged: bool, console: &mut dyn std::io::Write) {
        let line = code_line(code);
        info!("{line}");
        match self.operator {
            Some(operator) => operator.show_enrollment_code(Some(code)),
            None if !logged => {
                let _ = writeln!(console, "{line}");
            }
            None => {}
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

        fn show_enrollment_code(&self, code: Option<&EnrollmentCode>) {
            self.shown.lock().unwrap().push(code.cloned());
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
        let wrong_project = SessionError::WrongProject {
            project: "studio".to_string(),
            audience: "studio-staging".to_string(),
        };
        let (client, reasons, made) =
            enrolled_with(vec![Err(ClientError::Studio(wrong_project))], Ok(()));
        let error = client.unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<ClientError>(),
                Some(ClientError::Studio(SessionError::WrongProject { .. }))
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
}
