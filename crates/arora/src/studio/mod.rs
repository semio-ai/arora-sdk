//! The Semio Studio connection: the device's Studio bridge, which a runtime
//! carries from its start whether or not the device is enrolled with Studio,
//! and what a run does through the bridge's handle — the device's enrollment,
//! and showing the operator where the bridge stands. The bridge signs the
//! device in, rotates its token, and holds its Zenoh connection.
//!
//! The bridge and its handle are the device client's ([`StudioBridge`],
//! [`StudioHandle`]): [`StudioDevice::bridge`] gives both, for the
//! [`StudioDevice`] arora builds for a device ([`of`], [`in_device_dir`]).
//! The bridge goes to the runtime like any other
//! ([`AroraBuilder::with_bridge`](crate::AroraBuilder::with_bridge)); the
//! handle stays with whoever enrolls the device and follows the bridge — the
//! run ([`AroraBuilder::with_studio_handle`](crate::AroraBuilder::with_studio_handle)),
//! or the app embedding the device.
//!
//! # Whether a run connects
//!
//! Building with Studio is the opt-in: a runner built with it — arora's
//! `studio-bridge` feature for the `arora` binary, a HAL runner's own `studio`
//! feature — connects to Studio by default, and `--no-studio` opts a run out.
//! A build without it has no Studio option at all. A runner reads the option
//! with [`StudioArgs::connects`] and, when the run connects, gives the run the
//! device's Studio bridge and its handle:
//!
//! | the run | its bridge |
//! |---|---|
//! | passes nothing | Studio's; a device not enrolled with Studio [enrolls](#enrollment) while the run runs |
//! | passes `--no-studio` | the open local bridge, whatever the device holds |
//!
//! ```no_run
//! # async fn runner(
//! #     builder: arora::AroraBuilder,
//! #     studio: arora::studio::StudioArgs,
//! #     device: arora::Device,
//! #     hal: Box<dyn arora_hal::Hal>,
//! # ) -> anyhow::Result<()> {
//! let builder = if studio.connects() {
//!     let model = arora::studio::reported_model(&hal.describe().await);
//!     let (bridge, handle) = arora::studio::of(&device)?.bridge(model);
//!     builder.with_bridge(Box::new(bridge)).with_studio_handle(handle)
//! } else {
//!     builder
//! };
//! builder.with_device(device).with_hal(hal).run().await
//! # }
//! ```
//!
//! A run given no bridge serves the open local bridge
//! ([`AroraBuilder::run`](crate::AroraBuilder::run)). The Studio bridge is
//! the runner's to give: a feature of `arora` is enabled for every crate a
//! build links, so `studio-bridge` being on does not tell that the runner at
//! hand was built with Studio, and a runner built without it never gives one.
//!
//! A run logs which bridge it serves: the local bridge's address, or Semio
//! Studio's bridge.
//!
//! # Where the credential is
//!
//! The saved credential is the refresh token of the device's Studio session,
//! with the key encrypting it and the key and certificate a TLS bridge knows
//! the device by. Semio's device store keeps it under the device's name
//! ([`crate::Device::name`]), one directory per name, apart from the device
//! directory:
//!
//! ```text
//! <data_local_dir>/semio/devices/   the store: SEMIO_DEVICE_STORE replaces this whole path
//! └── <name>/                       the credential of the device <name>
//! ```
//!
//! arora names the device, and the store places its credential: arora asks
//! the device client for the name's credentials, and never computes where
//! they are. Several devices on one machine are several names.
//!
//! - The Studio side of a device given a device directory (`--device-dir`,
//!   `ARORA_DEVICE_DIR`) and no name is refused: "name this device with
//!   --device or ARORA_DEVICE". Two such devices would otherwise share the
//!   credential of the name `default`.
//! - A credential the device directory's `studio/` holds, where arora 11.1 to
//!   15 kept it, moves into the store under the device's name, once, when the
//!   store holds none for it; the log says so. Its bridge certificate is
//!   reissued.
//! - Given the deprecated `DEVICE_DIR` (or `IDENTITY_FILE`) and no name, the
//!   device keeps its credential in that directory's `studio/`:
//!   neither store nor move, so an upgraded runner keeps the credential it
//!   had, and two such devices never share one.
//!
//! # Enrollment
//!
//! Studio creates a device, under the owner a person chooses, when the device
//! enrolls; the device signs in with the credential its enrollment saved from
//! then on. While the device holds no credential Studio accepts — none, one
//! that does not read, one Studio refuses (a revoked or deleted device), or
//! one that is no device's, such as the anonymous account an earlier arora
//! signed the device up as — its bridge is offline: it contacts nobody, keeps
//! the latest value of each key the runtime sends, and publishes them once it
//! connects. The runtime runs meanwhile.
//!
//! A run given the bridge's handle enrolls the device then, while it runs:
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
//!   headless log's default without `RUST_LOG` — it goes to standard error.
//!   It comes with the Studio page approving it when the client knows
//!   Studio's web address (`STUDIO_URL`, else the one baked into
//!   `arora-studio-bridge-client`). A code that expires undecided is replaced
//!   by another; the run waits for a person's decision for as long as it
//!   runs.
//!
//! Then the bridge connects by itself, and the operator is shown the device's
//! id ([`Operator::show_device_id`]). A person refusing the code, or Studio
//! refusing the token, fails the run, and so does Studio refusing the device
//! it has just enrolled. Studio failing otherwise — Studio or the bridge out
//! of reach, a credential of another Firebase project than the run's — says
//! nothing of the device's enrollment: the device does not enroll again, and
//! its bridge logs why and connects again by itself, waiting longer after
//! each failure in a row, up to a minute. A device Studio refuses later in
//! the run, once revoked, enrolls again.
//!
//! A device can also be enrolled before it runs, with Semio's own client:
//! `semio device enroll NAME` saves the credential under its name in the same
//! store.
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
//! ([`reported_model`]). A report writes the fields it gives, the record
//! keeping the others. A device that enrolls with neither giving its family
//! is asked for one under the terminal UI: the family it enrolls with stays
//! its record's, as no later report gives another.
//!
//! # An app keeping the device's Studio side
//!
//! An app embedding a device builds its runtime with the device's Studio
//! bridge from the start, and keeps the bridge's handle: it runs at once,
//! whether or not the device is enrolled, and enrolls it when a person asks.
//! [`of`] gives the [`StudioDevice`] of a [`Device`], and [`in_device_dir`]
//! the one of a device directory and name, for a platform that gives the app
//! its data directory at run time (Android). Its credentials are the ones a
//! run as the device uses, one an earlier arora kept moving in as
//! [above](#where-the-credential-is); getting it, and getting its bridge for
//! a device holding no credential, contacts nobody. The handle's
//! [`status`](StudioHandle::status) says where the bridge stands — not
//! enrolled, enrolling with the code a person enters, connected as the
//! device's id — and [`enroll`](StudioHandle::enroll) enrolls the device,
//! after which the bridge connects by itself.
//!
//! ```no_run
//! use arora::studio::{self, DeviceDescription, EnrollWith, StudioHandle, StudioStatus};
//! use arora_hal::HalDescription;
//!
//! /// The device's runtime, built with its Studio bridge, and the handle the
//! /// app keeps for as long as it runs. Called within a Tokio runtime.
//! fn at_launch(
//!     device: &arora::Device,
//!     description: &HalDescription,
//! ) -> anyhow::Result<(arora::Arora, StudioHandle)> {
//!     let (bridge, handle) = studio::of(device)?.bridge(studio::reported_model(description));
//!     let arora = arora::Arora::builder()
//!         .with_bridge(Box::new(bridge))
//!         .build()?;
//!     Ok((arora, handle))
//! }
//!
//! /// Enrolls the device a person asked to enroll, under the name they gave;
//! /// the code to enter in Studio shows in the handle's status.
//! async fn on_enroll(handle: &StudioHandle, name: &str) -> anyhow::Result<String> {
//!     if *handle.status().borrow() != StudioStatus::NotEnrolled {
//!         anyhow::bail!("the device is enrolled");
//!     }
//!     let how = EnrollWith::token_from_env().unwrap_or_else(|| EnrollWith::Code(Box::new(|_| {})));
//!     Ok(handle.enroll(&DeviceDescription::new(name), how).await?)
//! }
//! ```
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
//!   - `ARORA_DEVICE` (`--device`), `ARORA_DEVICE_DIR` (`--device-dir`) — the
//!     device's name and directory, as [`crate::device`] says, with the
//!     deprecated `DEVICE_LOCAL_ID`, `DEVICE_DIR` and `IDENTITY_FILE`. An
//!     embedder passes them to [`in_device_dir`] instead.
//!   - `SEMIO_DEVICE_STORE` — the device store's root, replacing
//!     `<data_local_dir>/semio/devices/`: for a service running as another
//!     user, or tests. An embedder passes it to [`in_device_dir`] instead.
//!   - `DEVICE_NAME` — the name a device enrolls with; `MODEL_FAMILY`,
//!     `HARDWARE_VERSION`, `SOFTWARE_VERSION` — its model. A set variable wins
//!     over every other source of its field; a blank one counts as unset.
//!   - `RUST_LOG` — log filter (`env_logger`).

mod credentials;

use std::future::Future;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use arora_hal::HalDescription;
use arora_studio_bridge_client::credentials::DeviceCredentials;
use arora_studio_bridge_client::firestore_support::options::{
    FirebaseEmulatorOptions, FirebaseOptions,
};
use arora_studio_bridge_client::studio::ENROLLMENT_TOKEN_VAR;
use arora_studio_bridge_client::PartialDeviceModelVersion;
use futures::future::{Fuse, FusedFuture, FutureExt};
use log::info;
use tokio::sync::watch;

// The device's Studio side, the device client's, and what calling its
// methods takes: an embedder keeping it needs no dependency on the client.
pub use arora_studio_bridge_client::auth::DeviceDescription;
pub use arora_studio_bridge_client::error::Error;
pub use arora_studio_bridge_client::studio::{
    EnrollWith, StudioBridge, StudioDevice, StudioHandle, StudioStatus,
};
pub use arora_studio_bridge_client::PartialDeviceModel;

use crate::operator::{EnrollmentCode, Operator};
use crate::Device;

/// The prompt for the name of a device that enrolls. Empty falls back to a
/// generated `arora-device-<random>` name, so the device is never nameless.
const NAME_LABEL: &str = "Device name — leave empty for a generated name";

/// The prompt for the model family of a device that enrolls when neither
/// `MODEL_FAMILY` nor the HAL gives one. Empty is fine: the device enrolls
/// with no model family.
const MODEL_FAMILY_LABEL: &str = "Model family — optional, leave empty to skip";

/// The Studio option of a runner built with Studio, as a clap helper that
/// [`DeviceCli`](crate::DeviceCli) and the robot runners flatten:
/// `--no-studio`.
#[derive(Clone, Debug, Default, clap::Args)]
pub struct StudioArgs {
    /// Serve the open local bridge instead of Semio Studio's, which a run of
    /// this build otherwise serves, enrolling this device with Studio while
    /// it runs when it is not enrolled.
    #[arg(long)]
    no_studio: bool,
}

impl StudioArgs {
    /// Whether the run connects to Studio, giving the runtime the device's
    /// Studio bridge: unless it passes `--no-studio`.
    pub fn connects(&self) -> bool {
        !self.no_studio
    }
}

/// The Studio side of `device`, of the Studio the environment configures
/// ([configuration](self#configuration)), whose
/// [`bridge`](StudioDevice::bridge) a runtime carries.
///
/// Its credentials are the ones a run as `device` uses
/// ([where the credential is](self#where-the-credential-is)), created if
/// missing, one an earlier arora kept moving in once. Getting it reads and
/// writes local files only: nobody is contacted. Refuses a device given a
/// directory and no name.
pub fn of(device: &Device) -> Result<StudioDevice> {
    Ok(studio_device(credentials::of(device, None)?))
}

/// [`of`], as the device named `name` whose directory is `device_dir`, its
/// credential the name's in the device store under `store`, or in the
/// default one (`SEMIO_DEVICE_STORE`, else `<data_local_dir>/semio/devices/`)
/// with `None`.
///
/// Given a store, nothing else is looked up to place the device's directory
/// and its Studio credential: an Android app passes the directory its
/// platform gives it at run time (`AndroidApp::internal_data_path()`) and a
/// store root inside it, where the executable's directory is not writable,
/// there is no home directory, and the working directory is `/`. Fails on a
/// name that is not one path component.
pub fn in_device_dir(device_dir: &Path, name: &str, store: Option<&Path>) -> Result<StudioDevice> {
    let store = credentials::store_at(store)?;
    let credentials = credentials::in_store(&store, name, device_dir)?;
    Ok(studio_device(credentials))
}

/// The model the device reports to its Studio record as it connects, which
/// [`StudioDevice::bridge`] takes: each field from its variable, else from
/// what the device's HAL says of itself (`hal.describe().await`), as the
/// [module docs](self#the-devices-model) say. `None` when neither gives any.
pub fn reported_model(description: &HalDescription) -> Option<PartialDeviceModel> {
    Model::of(&env_nonempty, description).reported()
}

// What a run follows its Studio bridge with is `Send`, so the run's future
// is, and so is the Studio side an embedder shares with the tasks enrolling
// the device.
const _: () = {
    fn send<T: Send>(_: &T) {}
    fn shared<T: Send + Sync>(_: &T) {}
    #[allow(dead_code)]
    fn studio_futures_are_send(
        handle: StudioHandle,
        description: &HalDescription,
        operator: &dyn Operator,
        studio_device: &StudioDevice,
    ) {
        shared(studio_device);
        shared(&handle);
        send(&follow(handle, description, Some(operator)));
    }
};

/// The Studio side of the device whose Studio credentials are
/// `credentials`, of the Studio the environment configures
/// ([configuration](self#configuration)).
fn studio_device(credentials: DeviceCredentials) -> StudioDevice {
    // The Zenoh and Firebase TLS stacks need a default crypto provider,
    // whoever connects the device: a run, or an embedder through the
    // device's bridge.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let studio_device = StudioDevice::new(FirebaseOptions::from_env(), credentials)
        .with_emulators(FirebaseEmulatorOptions::from_env());
    match env_nonempty("STUDIO_BRIDGE_ENDPOINT") {
        Some(endpoint) => {
            info!("Studio: the device's bridge endpoint is {endpoint} (STUDIO_BRIDGE_ENDPOINT)");
            studio_device.with_endpoint(endpoint)
        }
        None => studio_device,
    }
}

/// What a run does through its Studio bridge's `handle`, for as long as the
/// run runs: it enrolls the device while it is not enrolled, as the
/// [module docs](self#enrollment) say, the device described by its name and
/// the model `description` completes; it shows each code that enrolls the
/// device, and the device's id once its bridge connects. `operator`, under
/// the terminal UI, names the device when `DEVICE_NAME` does not and is
/// shown the codes and the id; headless, nobody is asked.
///
/// Returns only when the run fails: the error a refused enrollment gives.
pub(crate) async fn follow(
    handle: StudioHandle,
    description: &HalDescription,
    operator: Option<&dyn Operator>,
) -> anyhow::Error {
    info!("Studio: the device's bridge is Semio Studio's");
    let status = handle.status();
    let handle = &handle;
    follow_status(
        status,
        || enroll(handle, description, operator),
        operator,
        &mut std::io::stderr(),
    )
    .await
}

/// [`follow`], over the bridge's `status` as it changes, enrolling the device
/// with `enroll`. A code nothing else shows goes to `console`.
async fn follow_status<Enrolled>(
    mut status: watch::Receiver<StudioStatus>,
    mut enroll: impl FnMut() -> Enrolled,
    operator: Option<&dyn Operator>,
    console: &mut (dyn std::io::Write + Send),
) -> anyhow::Error
where
    Enrolled: Future<Output = Result<String>>,
{
    let mut codes = Codes {
        operator,
        shown: None,
    };
    let enrollment = Fuse::terminated();
    futures::pin_mut!(enrollment);
    // The device enrolled in this run, and its bridge has not connected
    // since: Studio refusing it now refuses the device it has just enrolled.
    let mut just_enrolled = false;
    let mut followed = true;
    loop {
        let current = status.borrow_and_update().clone();
        match &current {
            StudioStatus::NotEnrolled if enrollment.is_terminated() => {
                if just_enrolled {
                    return anyhow!(
                        "Semio Studio refuses the device it has just enrolled: its bridge finds \
                         it not enrolled"
                    );
                }
                info!("Studio: this device is not enrolled with Semio Studio; enrolling it");
                enrollment.set(enroll().fuse());
            }
            StudioStatus::Connected { device_id } => {
                just_enrolled = false;
                info!("Studio: connected to Semio Studio as device {device_id}");
                if let Some(operator) = operator {
                    operator.show_device_id(device_id);
                }
            }
            _ => {}
        }
        codes.follow(&current, log::log_enabled!(log::Level::Info), &mut *console);
        // The enrollment's end is taken first: the bridge connects only once
        // the enrollment has told it the device is enrolled, so what its
        // status says next follows the enrollment.
        tokio::select! {
            biased;
            enrolled = &mut enrollment, if !enrollment.is_terminated() => match enrolled {
                Ok(_) => just_enrolled = true,
                Err(error) => return error,
            },
            changed = status.changed(), if followed => {
                // The bridge was dropped: its status changes no more.
                followed = changed.is_ok();
            }
            () = std::future::pending(), if !followed && enrollment.is_terminated() => {}
        }
    }
}

/// Enrolls the device of the Studio bridge `handle` follows, as the
/// [module docs](self#enrollment) say, and returns its id: with the
/// enrollment token `STUDIO_ENROLLMENT_TOKEN` holds, else by code, which the
/// bridge's status shows. The device is described by its name —
/// `DEVICE_NAME`, else what `operator` answers, else generated — and its
/// model as `description` completes it, `operator` asked its family when
/// nothing gives one.
async fn enroll(
    handle: &StudioHandle,
    description: &HalDescription,
    operator: Option<&dyn Operator>,
) -> Result<String> {
    let device = enrolling_device(
        &env_nonempty,
        &Model::of(&env_nonempty, description),
        operator,
    )
    .await;
    let how = match EnrollWith::token_from_env() {
        Some(token) => {
            info!(
                "Studio: enrolling this device as {} with the enrollment token in \
                 {ENROLLMENT_TOKEN_VAR}",
                device.name
            );
            token
        }
        None => {
            info!("Studio: enrolling this device as {} by code", device.name);
            EnrollWith::Code(Box::new(|_| {}))
        }
    };
    let device_id = handle
        .enroll(&device, how)
        .await
        .context("failed to enroll the device with Semio Studio")?;
    info!("Studio: enrolled as device {device_id}");
    Ok(device_id)
}

/// Shows the codes of an enrollment by code: each in the log, and to the
/// operator, if any, whose code is cleared once the bridge's status shows
/// none — or once this is dropped, which would otherwise leave the operator
/// showing a code nobody waits on.
struct Codes<'a> {
    operator: Option<&'a dyn Operator>,
    /// The code shown last, until it is cleared.
    shown: Option<EnrollmentCode>,
}

impl Codes<'_> {
    /// Shows the code `status` asks a person to enter, when it is another
    /// than the one shown, and clears the operator's once `status` asks
    /// none. `logged` is whether the log shows info lines, and `console`
    /// where a code goes that nothing else shows: someone must see it to
    /// enroll the device, so when no operator shows it and the log shows no
    /// info lines — the headless log's default without `RUST_LOG` — it goes
    /// there.
    fn follow(&mut self, status: &StudioStatus, logged: bool, console: &mut dyn std::io::Write) {
        let asked = match status {
            StudioStatus::Enrolling {
                code: Some(code),
                approval_url,
            } => Some(EnrollmentCode {
                code: code.clone(),
                approval_url: approval_url.clone(),
            }),
            _ => None,
        };
        if asked == self.shown {
            return;
        }
        match &asked {
            Some(code) => {
                let line = code_line(code);
                info!("{line}");
                let shown = self
                    .operator
                    .is_some_and(|operator| operator.show_enrollment_code(Some(code)));
                if !shown && !logged {
                    let _ = writeln!(console, "{line}");
                }
            }
            None => self.clear(),
        }
        self.shown = asked;
    }

    /// Clears the operator's code.
    fn clear(&self) {
        if let Some(operator) = self.operator {
            operator.show_enrollment_code(None);
        }
    }
}

impl Drop for Codes<'_> {
    fn drop(&mut self) {
        if self.shown.is_some() {
            self.clear();
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
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;

    use arora_bridge::AccessDecision;

    use super::*;
    use crate::operator::{AccessRequestSummary, AccessRuling};

    /// An operator answering each prompt from a script, keyed by its label,
    /// and recording the labels it was asked, the codes it was shown and the
    /// device ids.
    #[derive(Default)]
    struct Scripted {
        answers: HashMap<&'static str, &'static str>,
        asked: Mutex<Vec<String>>,
        shown: Mutex<Vec<Option<EnrollmentCode>>>,
        device_ids: Mutex<Vec<String>>,
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

        fn device_ids(&self) -> Vec<String> {
            self.device_ids.lock().unwrap().clone()
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

        fn show_device_id(&self, device_id: &str) {
            self.device_ids.lock().unwrap().push(device_id.to_string());
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

    fn enrolling(code: Option<&str>, approval_url: Option<&str>) -> StudioStatus {
        StudioStatus::Enrolling {
            code: code.map(str::to_string),
            approval_url: approval_url.map(str::to_string),
        }
    }

    fn connected(device_id: &str) -> StudioStatus {
        StudioStatus::Connected {
            device_id: device_id.to_string(),
        }
    }

    /// How long a test waits for what it expects before it fails.
    const WAIT: Duration = Duration::from_secs(10);

    /// A Studio bridge's status as a test drives it, and a run following it
    /// whose enrollments the test answers, one at a time.
    struct Followed {
        status: watch::Sender<StudioStatus>,
        /// Each enrollment the run starts asks for its answer here.
        enrollments:
            tokio::sync::mpsc::UnboundedReceiver<tokio::sync::oneshot::Sender<Result<String>>>,
        /// The error the run ended with, once it ends.
        ended: tokio::task::JoinHandle<String>,
        operator: Arc<Scripted>,
        console: Arc<Mutex<Vec<u8>>>,
    }

    /// A console a test reads back.
    struct Console(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Console {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Followed {
        /// A run following a bridge whose status starts as `initial`, with an
        /// operator under the terminal UI when `interactive`.
        fn start(initial: StudioStatus, interactive: bool) -> Self {
            let (status, receiver) = watch::channel(initial);
            let (asks, enrollments) = tokio::sync::mpsc::unbounded_channel();
            let operator = Arc::new(Scripted::default());
            let console = Arc::new(Mutex::new(Vec::new()));
            let ended = tokio::spawn({
                let operator = operator.clone();
                let mut console = Console(console.clone());
                async move {
                    let enroll = || {
                        let (answer, answered) = tokio::sync::oneshot::channel();
                        asks.send(answer).unwrap();
                        async move { answered.await.unwrap() }
                    };
                    let operator = interactive.then_some(&*operator as &dyn Operator);
                    format!(
                        "{:#}",
                        follow_status(receiver, enroll, operator, &mut console).await
                    )
                }
            });
            Followed {
                status,
                enrollments,
                ended,
                operator,
                console,
            }
        }

        /// The next enrollment the run starts.
        async fn enrollment(&mut self) -> tokio::sync::oneshot::Sender<Result<String>> {
            tokio::time::timeout(WAIT, self.enrollments.recv())
                .await
                .expect("the run enrolls the device")
                .expect("the run is following")
        }

        /// Waits until what the operator was shown passes `check`.
        async fn until(&self, what: &str, check: impl Fn(&Scripted) -> bool) {
            tokio::time::timeout(WAIT, async {
                while !check(&self.operator) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("the operator is not shown {what}"));
        }

        /// Whether the run starts an enrollment, given the time to.
        async fn enrolls(&mut self) -> bool {
            settle().await;
            self.enrollments.try_recv().is_ok()
        }

        /// The error the run ends with.
        async fn ended(self) -> String {
            tokio::time::timeout(WAIT, self.ended)
                .await
                .expect("the run ends")
                .unwrap()
        }

        fn console(&self) -> String {
            String::from_utf8(self.console.lock().unwrap().clone()).unwrap()
        }
    }

    /// Lets the run take in what the test sent it.
    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    #[tokio::test]
    async fn a_device_not_enrolled_enrolls_shows_its_code_then_its_id() {
        let mut run = Followed::start(StudioStatus::NotEnrolled, true);
        let enrollment = run.enrollment().await;
        run.status.send_replace(enrolling(None, None));
        run.status.send_replace(enrolling(Some("BCDF-GHJK"), None));
        run.until("the first code", |operator| operator.shown().len() == 1)
            .await;
        run.status.send_replace(enrolling(
            Some("LMNP-QRST"),
            Some("https://studio.example/approve"),
        ));
        run.until("the next code", |operator| operator.shown().len() == 2)
            .await;
        enrollment.send(Ok("device-1".to_string())).unwrap();
        run.status.send_replace(StudioStatus::Connecting);
        run.until("the code cleared", |operator| operator.shown().len() == 3)
            .await;
        run.status.send_replace(connected("device-1"));
        run.until("the device's id", |operator| {
            !operator.device_ids().is_empty()
        })
        .await;

        assert_eq!(
            run.operator.shown(),
            [
                Some(code("BCDF-GHJK", None)),
                Some(code("LMNP-QRST", Some("https://studio.example/approve"))),
                None
            ]
        );
        assert_eq!(run.operator.device_ids(), ["device-1"]);
        assert!(!run.enrolls().await, "the device enrolls once");
        assert!(run.console().is_empty(), "{}", run.console());
        assert!(!run.ended.is_finished());
    }

    #[tokio::test]
    async fn an_enrolled_device_connects_without_enrolling() {
        let mut run = Followed::start(StudioStatus::Connecting, true);
        run.status
            .send_replace(StudioStatus::Failed("no route".to_string()));
        settle().await;
        run.status.send_replace(StudioStatus::Connecting);
        run.status.send_replace(connected("device-1"));
        run.until("the device's id", |operator| {
            !operator.device_ids().is_empty()
        })
        .await;
        assert!(
            !run.enrolls().await,
            "a device Studio did not refuse enrolls"
        );
        assert_eq!(run.operator.device_ids(), ["device-1"]);
        assert!(run.operator.shown().is_empty());
    }

    #[tokio::test]
    async fn a_refused_enrollment_fails_the_run() {
        let mut run = Followed::start(StudioStatus::NotEnrolled, false);
        let enrollment = run.enrollment().await;
        run.status.send_replace(enrolling(Some("BCDF-GHJK"), None));
        settle().await;
        enrollment
            .send(Err(anyhow::Error::new(Error::EnrollmentDenied)))
            .unwrap();
        run.status.send_replace(StudioStatus::NotEnrolled);
        let error = run.ended().await;
        assert!(error.contains("refused"), "{error}");
    }

    #[tokio::test]
    async fn a_device_studio_refuses_once_enrolled_fails_the_run() {
        let mut run = Followed::start(StudioStatus::NotEnrolled, false);
        let enrollment = run.enrollment().await;
        enrollment.send(Ok("device-1".to_string())).unwrap();
        settle().await;
        run.status.send_replace(StudioStatus::Connecting);
        run.status.send_replace(StudioStatus::NotEnrolled);
        let error = run.ended().await;
        assert!(error.contains("just enrolled"), "{error}");
    }

    #[tokio::test]
    async fn a_device_revoked_after_it_connected_enrolls_again() {
        let mut run = Followed::start(StudioStatus::NotEnrolled, true);
        let enrollment = run.enrollment().await;
        enrollment.send(Ok("device-1".to_string())).unwrap();
        run.status.send_replace(StudioStatus::Connecting);
        run.status.send_replace(connected("device-1"));
        run.until("the device's id", |operator| {
            !operator.device_ids().is_empty()
        })
        .await;
        run.status.send_replace(StudioStatus::NotEnrolled);
        let enrollment = run.enrollment().await;
        enrollment.send(Ok("device-2".to_string())).unwrap();
        run.status.send_replace(StudioStatus::Connecting);
        run.status.send_replace(connected("device-2"));
        settle().await;
        assert!(!run.ended.is_finished());
    }

    #[tokio::test]
    async fn a_code_nothing_else_shows_goes_to_the_console() {
        // Headless, with no log showing info lines (no logger in tests).
        let mut run = Followed::start(StudioStatus::NotEnrolled, false);
        let _enrollment = run.enrollment().await;
        run.status.send_replace(enrolling(Some("BCDF-GHJK"), None));
        tokio::time::timeout(WAIT, async {
            while run.console().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the code goes to the console");
        run.status.send_replace(enrolling(Some("BCDF-GHJK"), None));
        settle().await;
        assert_eq!(
            run.console(),
            "Studio: to enroll this device, enter the code BCDF-GHJK in Semio Studio\n",
            "each code once"
        );
        assert!(run.operator.shown().is_empty(), "headless, nobody is shown");
    }

    #[test]
    fn a_code_the_log_or_the_operator_shows_is_not_repeated_on_the_console() {
        let status = enrolling(Some("BCDF-GHJK"), None);
        let mut console = Vec::new();
        let mut codes = Codes {
            operator: None,
            shown: None,
        };
        codes.follow(&status, true, &mut console);
        let operator = Scripted::default();
        let mut codes = Codes {
            operator: Some(&operator),
            shown: None,
        };
        codes.follow(&status, false, &mut console);
        assert!(console.is_empty(), "{:?}", String::from_utf8(console));
        assert_eq!(operator.shown(), [Some(code("BCDF-GHJK", None))]);

        // The operator's code is cleared when the codes are dropped.
        drop(codes);
        assert_eq!(operator.shown(), [Some(code("BCDF-GHJK", None)), None]);
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
            shown: None,
        }
        .follow(&enrolling(Some("BCDF-GHJK"), None), false, &mut console);
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

    #[test]
    fn a_run_connects_unless_it_passes_no_studio() {
        use clap::Parser;
        let connects = |args: &[&str]| {
            Runner::try_parse_from(std::iter::once("runner").chain(args.iter().copied()))
                .expect("the Studio option parses")
                .studio
                .connects()
        };
        assert!(connects(&[]));
        assert!(!connects(&["--no-studio"]));
        let error =
            Runner::try_parse_from(["runner", "--studio"]).expect_err("--studio is refused");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    /// A run whose device is not enrolled runs at once: its runtime steps
    /// while the device enrolls, through the handle the run was given. The
    /// device's Studio is a socket that accepts and never answers, so the
    /// enrollment waits on it for as long as the test watches.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_run_whose_device_is_not_enrolled_runs_while_it_enrolls() {
        use std::net::TcpListener;

        let root = std::env::temp_dir().join(format!("arora-studio-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let silent = TcpListener::bind("127.0.0.1:0").unwrap();
        silent.set_nonblocking(true).unwrap();
        let address = silent.local_addr().unwrap().to_string();
        let studio = StudioDevice::new(
            FirebaseOptions {
                api_key: None,
                auth_domain: None,
                database_url: None,
                project_id: Some("studio".to_string()),
                storage_bucket: None,
                messaging_sender_id: None,
                app_id: None,
                measurement_id: None,
            },
            DeviceCredentials::in_dir(root.join("credentials")).unwrap(),
        )
        .with_emulators(FirebaseEmulatorOptions {
            auth_url: Some(format!("http://{address}")),
            firestore_url: Some(format!("http://{address}")),
            storage_url: None,
            database_url: None,
            functions_url: Some(format!("http://{address}")),
        })
        .with_endpoint(format!("tcp/{address}"));
        let (bridge, handle) = studio.bridge(None);
        let status = handle.status();
        let builder = crate::Arora::builder()
            .with_bridge(Box::new(bridge))
            .with_studio_handle(handle.clone());
        let (ready_tx, ready) = std::sync::mpsc::channel();
        let frontend = crate::operator::Frontend {
            operator: Arc::new(Scripted::default()),
            interactive: false,
            on_ready: Box::new(move |state, _, _| {
                let _ = ready_tx.send(state);
            }),
        };

        let watched = async {
            let state = loop {
                if let Ok(state) = ready.try_recv() {
                    break state;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            // The device starts enrolling: Studio is contacted.
            let mut contacts = Vec::new();
            while contacts.is_empty() {
                if let Ok((stream, _)) = silent.accept() {
                    contacts.push(stream);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            // And the runtime steps meanwhile, its clock moving on.
            let mut times = Vec::new();
            while times.len() < 2 {
                while let Some(change) = state.try_recv() {
                    if let Some(Some(time)) = change.set.get(&arora_types::data::Key::from(
                        arora_behavior::built_in::TIME,
                    )) {
                        times.push(time.clone());
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_ne!(times[0], times[1], "the clock moves");
            let status = status.borrow().clone();
            status
        };
        let outcome = tokio::time::timeout(Duration::from_secs(60), async {
            tokio::select! {
                ended = crate::run::run_builder_with_frontend(builder, frontend) => {
                    panic!("the run ended: {ended:?}")
                }
                status = watched => status,
            }
        })
        .await
        .expect("within 60 s, the device starts enrolling and its runtime steps");
        assert!(
            matches!(outcome, StudioStatus::Enrolling { .. }),
            "{outcome:?}"
        );
        drop(handle);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The variable handing the process
    /// [`an_embedder_s_studio_side_is_a_run_s_and_contacts_nobody`] starts
    /// the directory its devices are laid out in.
    const STUDIO_SIDE_CASE_DIR: &str = "ARORA_TEST_STUDIO_SIDE_CASE_DIR";

    /// The Studio side an embedder gets is a run's — the credentials a run
    /// as the device uses, a device directory's older one moved in, and the
    /// model it reports — and getting it, asking whether it holds a
    /// credential, and the bridge of a device holding none contact nobody. The test runs itself again as
    /// a process whose environment configures the device and points Firebase
    /// and the bridge at a socket the test watches.
    #[test]
    fn an_embedder_s_studio_side_is_a_run_s_and_contacts_nobody() {
        match std::env::var_os(STUDIO_SIDE_CASE_DIR) {
            Some(root) => get_studio_sides(Path::new(&root)),
            None => watch_studio_sides_got(),
        }
    }

    /// Starts this test again as a process getting the Studio side of the
    /// devices laid out in a fresh directory, each directory holding the
    /// credential an earlier arora kept there, and fails when anything
    /// reaches the socket its Studio and bridge are at.
    fn watch_studio_sides_got() {
        use std::net::TcpListener;
        use std::process::Command;

        let root = std::env::temp_dir().join(format!("arora-studio-side-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["bench", "android"] {
            DeviceCredentials::in_dir(root.join(name).join(credentials::STUDIO_DIR))
                .unwrap()
                .save_refresh_token(&format!("{name}-token"))
                .unwrap();
        }
        let watched = TcpListener::bind("127.0.0.1:0").unwrap();
        watched.set_nonblocking(true).unwrap();
        let address = watched.local_addr().unwrap().to_string();

        let test = concat!(
            module_path!(),
            "::an_embedder_s_studio_side_is_a_run_s_and_contacts_nobody"
        );
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([test.strip_prefix("arora::").unwrap(), "--exact"])
            .env(STUDIO_SIDE_CASE_DIR, &root)
            .env("ARORA_DEVICE", "bench")
            .env("ARORA_DEVICE_DIR", root.join("bench"))
            .env("SEMIO_DEVICE_STORE", root.join("store"))
            .env("MODEL_FAMILY", "env-family")
            .env("FIREBASE_PROJECT_ID", "studio")
            .env("STUDIO_BRIDGE_ENDPOINT", format!("tcp/{address}"))
            .env_remove("FIREBASE_CONFIG")
            .env_remove("DEVICE_LOCAL_ID")
            .env_remove("DEVICE_DIR")
            .env_remove("IDENTITY_FILE")
            .env_remove("HARDWARE_VERSION")
            .env_remove("SOFTWARE_VERSION");
        for emulator in [
            "FIREBASE_AUTH_EMULATOR_HOST",
            "FIREBASE_FIRESTORE_EMULATOR_HOST",
            "FIREBASE_STORAGE_EMULATOR_HOST",
            "FIREBASE_DATABASE_EMULATOR_HOST",
            "FIREBASE_FUNCTIONS_EMULATOR_HOST",
            "FIRESTORE_EMULATOR_HOST",
        ] {
            command.env(emulator, &address);
        }

        // The process writes to a file, not a pipe, so it never blocks on a
        // pipe nobody reads while the loop below watches for connections.
        let log = root.join("process.log");
        let log_file = std::fs::File::create(&log).unwrap();
        command
            .stdout(log_file.try_clone().unwrap())
            .stderr(log_file);
        let mut process = command.spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let contacted = || watched.accept().ok().map(|(_, from)| from);
        let status = loop {
            let finished = process.try_wait().unwrap();
            let failure = match contacted() {
                Some(from) => Some(format!(
                    "getting a device's Studio side contacted {address}, from {from}"
                )),
                None if finished.is_none() && std::time::Instant::now() > deadline => {
                    Some("the process did not finish within 120 s".to_string())
                }
                None => None,
            };
            if let Some(failure) = failure {
                let _ = process.kill();
                let _ = process.wait();
                panic!(
                    "{failure}:\n{}",
                    std::fs::read_to_string(&log).unwrap_or_default()
                );
            }
            if let Some(status) = finished {
                break status;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let said = std::fs::read_to_string(&log).unwrap();
        assert!(status.success(), "the process failed:\n{said}");
        assert!(
            said.contains("1 passed"),
            "the process ran no test:\n{said}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// In the process the test starts: the Studio side of the device the
    /// environment configures, and of devices given their directory and
    /// store, as an Android app gives them.
    fn get_studio_sides(root: &Path) {
        let bench = of(&Device::from_env().unwrap()).unwrap();
        assert_eq!(bench.credentials().path(), root.join("store").join("bench"));
        assert_eq!(
            bench.credentials().refresh_token().unwrap().as_deref(),
            Some("bench-token")
        );
        assert!(bench.holds_credential());
        assert!(
            !root.join("bench").join(credentials::STUDIO_DIR).exists(),
            "the device directory's credential moved into the store"
        );

        let store = root.join("android").join("devices");
        let android = in_device_dir(&root.join("android"), "android", Some(&store)).unwrap();
        assert_eq!(android.credentials().path(), store.join("android"));
        assert_eq!(
            android.credentials().refresh_token().unwrap().as_deref(),
            Some("android-token")
        );
        assert!(android.holds_credential());

        let fresh = in_device_dir(&root.join("fresh"), "fresh", Some(&store)).unwrap();
        assert!(!fresh.holds_credential(), "nothing was kept for it");
        // Its bridge, which a runtime carries from its start, stays offline.
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (bridge, handle) = fresh.bridge(None);
                tokio::time::sleep(Duration::from_millis(200)).await;
                assert_eq!(*handle.status().borrow(), StudioStatus::NotEnrolled);
                drop(bridge);
            });

        assert_eq!(
            reported_model(&description(Some("nao"), Some("6"))),
            Some(PartialDeviceModel {
                family: Some("env-family".to_string()),
                version: Some(PartialDeviceModelVersion {
                    hardware: Some("6".to_string()),
                    software: Some("6".to_string()),
                }),
            }),
            "each field is its variable's, else the HAL's"
        );
    }
}
