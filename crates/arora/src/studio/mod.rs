//! The Semio Studio connection: the device's Studio bridge, which a runtime
//! carries from its start whether or not the device is enrolled with Studio,
//! and the device's enrollment while it runs. The bridge signs the device
//! in, rotates its token, and holds its Zenoh connection.
//!
//! The bridge, its handle and the enrollment are the device client's
//! (`arora-studio-bridge-client`), which this module re-exports:
//! [`StudioDevice::bridge`] gives the bridge and its [`StudioHandle`], for the
//! [`StudioDevice`] arora builds for a device ([`of`], [`in_device_dir`]).
//! The bridge goes to the runtime like any other
//! ([`AroraBuilder::with_bridge`](crate::AroraBuilder::with_bridge)); the
//! handle stays with the runner, or the app embedding the device, which
//! enrolls the device through it
//! ([`enroll_when_needed`](StudioHandle::enroll_when_needed)), presented to a
//! person by an [`Enroller`]: a run's is its front end's
//! ([`Operator::enroller`](crate::operator::Operator::enroller)).
//!
//! # Whether a run connects
//!
//! Building with Studio is the opt-in: a runner built with it — arora's
//! `studio-bridge` feature for the `arora` binary, a HAL runner's own `studio`
//! feature — connects to Studio by default, and `--no-studio` opts a run out.
//! A build without it has no Studio option at all. A runner reads the option
//! with [`StudioArgs::connects`] and, when the run connects, gives the run the
//! device's Studio bridge, and enrolls the device beside the run through the
//! front end's enroller:
//!
//! | the run | its bridge |
//! |---|---|
//! | passes nothing | Studio's; a device not enrolled with Studio [enrolls](#enrollment) while the run runs |
//! | passes `--no-studio` | the open local bridge, whatever the device holds |
//!
//! ```no_run
//! # async fn runner(
//! #     studio: arora::studio::StudioArgs,
//! #     device: arora::Device,
//! #     hal: Box<dyn arora_hal::Hal>,
//! # ) -> anyhow::Result<()> {
//! // The front end first: building it installs the log sink.
//! let frontend = arora::standard_frontend();
//! let enroller = frontend.operator.clone().enroller();
//! let builder = arora::Arora::builder().with_frontend(frontend);
//! if studio.connects() {
//!     let model = arora::studio::reported_model(&hal.describe().await);
//!     let (bridge, handle) = arora::studio::of(&device)?.bridge(model);
//!     let run = builder
//!         .with_bridge(Box::new(bridge))
//!         .with_device(device)
//!         .with_hal(hal)
//!         .run();
//!     // The enrollment ends only when the device can neither enroll nor
//!     // connect, which fails the run.
//!     return tokio::select! {
//!         ran = run => ran,
//!         ended = handle.enroll_when_needed(&*enroller) => Err(ended.into()),
//!     };
//! }
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
//! [`StudioHandle::enroll_when_needed`] enrolls the device then, for as long
//! as the runner runs it, as its documentation says: with the enrollment
//! token `STUDIO_ENROLLMENT_TOKEN` holds, else by a code a person enters in
//! Studio once the enroller accepts to enroll; enrolling again after a wait
//! when the enrollment was declined or failed for a reason that may pass,
//! and when the bridge finds the device not enrolled again, revoked in
//! Studio. The token is a secret, read from the environment at run time
//! only: no option takes it, since a command line is visible to the host's
//! other users. It returns only when the device can neither enroll nor
//! connect through the bridge ([`EnrollmentEnded`]), which fails the run:
//!
//! - a person refusing the code, or Studio refusing the token;
//! - the bridge halting ([`HaltReason`]), such as on a credential of another
//!   Firebase project than the run's: the error names the credentials
//!   directory to delete for the device to enroll with the run's project.
//!
//! What a person sees is the front end's enroller:
//!
//! - under the terminal UI, the run asks whether to enroll the device — a
//!   person declining is asked again later —, its name when `DEVICE_NAME`
//!   gives none (left blank, a name is generated), and its model family when
//!   the [model](#the-devices-model) gives none. It shows each code in the
//!   prompt line, with the Studio page approving it when the client knows
//!   Studio's web address (`STUDIO_URL`, else the one baked into the client),
//!   then the device's Studio record — its name and model family — and its
//!   id in the header.
//! - headless ([`UnattendedEnroller`]), nobody is asked: the device enrolls
//!   at once, named by `DEVICE_NAME`, else a generated name; each code is
//!   logged, and goes to standard error when the log shows no info lines —
//!   the headless log's default without `RUST_LOG`.
//!
//! A device can also be enrolled before it runs, with Semio's own client:
//! `semio device enroll NAME` saves the credential under its name in the same
//! store.
//!
//! The device's record is Studio's once it is enrolled: its name and owner
//! change there, never from the device.
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
//! whether or not the device is enrolled. [`of`] gives the [`StudioDevice`]
//! of a [`Device`], and [`in_device_dir`] the one of a device directory and
//! name, for a platform that gives the app its data directory at run time
//! (Android). Its credentials are the ones a run as the device uses, one an
//! earlier arora kept moving in as [above](#where-the-credential-is);
//! getting it, and getting its bridge for a device holding no credential,
//! contacts nobody.
//!
//! The app enrolls the device as a run does, through
//! [`enroll_when_needed`](StudioHandle::enroll_when_needed) and an
//! [`Enroller`] of its own, which asks and shows on its screen; or itself,
//! when a person asks, with [`enroll`](StudioHandle::enroll), after which the
//! bridge connects by itself. The handle's [`status`](StudioHandle::status)
//! says where the bridge stands — not enrolled, enrolling with the code a
//! person enters, connected as the device's id, halted and why — and its
//! [`record`](StudioHandle::record) gives the device's Studio record.
//!
//! ```no_run
//! use arora::studio::{self, Enroller, EnrollmentCode, EnrollmentEnded, StudioHandle};
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
//! /// The app's screen, presenting the device's enrollment.
//! struct Screen;
//!
//! #[async_trait::async_trait]
//! impl Enroller for Screen {
//!     fn show_code(&self, code: Option<&EnrollmentCode>) -> bool {
//!         if let Some(code) = code {
//!             println!("Enter {} in Semio Studio to enroll this device.", code.code);
//!         }
//!         true
//!     }
//! }
//!
//! /// For as long as the app runs: it ends only when the device can neither
//! /// enroll nor connect.
//! async fn enroll(handle: &StudioHandle) -> EnrollmentEnded {
//!     handle.enroll_when_needed(&Screen).await
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

use std::path::Path;

use anyhow::Result;
use arora_hal::HalDescription;
use arora_studio_bridge_client::credentials::DeviceCredentials;
use arora_studio_bridge_client::firestore_support::options::{
    FirebaseEmulatorOptions, FirebaseOptions,
};
use arora_studio_bridge_client::PartialDeviceModelVersion;
use log::info;

// The device's Studio side, the device client's, and what calling its
// methods takes: an embedder keeping it needs no dependency on the client.
pub use arora_studio_bridge_client::auth::DeviceDescription;
pub use arora_studio_bridge_client::error::Error;
pub use arora_studio_bridge_client::studio::{
    EnrollWith, Enroller, EnrollmentCode, EnrollmentEnded, HaltReason, StudioBridge, StudioDevice,
    StudioHandle, StudioStatus, UnattendedEnroller,
};
pub use arora_studio_bridge_client::PartialDeviceModel;

use crate::Device;

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

// The Studio side an embedder shares with the tasks enrolling the device is
// `Send` and `Sync`.
const _: () = {
    fn shared<T: Send + Sync>(_: &T) {}
    #[allow(dead_code)]
    fn studio_sides_are_shared(handle: &StudioHandle, studio_device: &StudioDevice) {
        shared(studio_device);
        shared(handle);
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;

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

    /// A run whose device is not enrolled runs at once, as a runner runs it:
    /// its runtime steps while the device enrolls beside it, through the
    /// bridge's handle and the front end's enroller. The device's Studio is
    /// a socket that accepts and never answers, so the enrollment waits on
    /// it for as long as the test watches.
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
        let builder = crate::Arora::builder().with_bridge(Box::new(bridge));
        let (ready_tx, ready) = std::sync::mpsc::channel();
        let frontend = crate::operator::Frontend {
            operator: std::sync::Arc::new(crate::operator::UnattendedOperator::default()),
            on_ready: Box::new(move |state, _, _| {
                let _ = ready_tx.send(state);
            }),
        };
        let enroller = frontend.operator.clone().enroller();

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
                ended = handle.enroll_when_needed(&*enroller) => {
                    panic!("the enrollment ended: {ended}")
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
