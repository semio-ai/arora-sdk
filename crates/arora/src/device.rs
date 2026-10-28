//! The device a run is: its name, its directory, and the port of its open
//! local bridge, from the options every runner takes ([`crate::RunnerArgs`])
//! or from the environment ([`Device::from_env`]).
//!
//! | input | option | variable | when neither is given |
//! |---|---|---|---|
//! | name | `--device NAME` | `ARORA_DEVICE` | [`DEFAULT_NAME`](crate::device_dir::DEFAULT_NAME) |
//! | directory | `--device-dir PATH` | `ARORA_DEVICE_DIR` | `<data_local_dir>/semio/arora/devices/<name>` ([`device_dir::of`](crate::device_dir::of)) |
//! | local bridge port | `--bridge-port PORT` | `ARORA_BRIDGE_PORT` | [`DEFAULT_BRIDGE_PORT`](crate::device::DEFAULT_BRIDGE_PORT) |
//!
//! The option wins over its variable. Several devices on one host are several
//! names, each with its own directory and port.
//!
//! The name is local to the machine: one path component, which Semio Studio
//! knows nothing of. It keys the device's Studio credential in Semio's device
//! store, apart from its directory, in a build with Studio
//! (`arora::studio`). A run that connects to Studio given a directory and no
//! name is refused there: two such devices would otherwise share the
//! credential of the default name.
//!
//! Deprecated variables, each with a warning:
//!
//! - `DEVICE_LOCAL_ID` names the device, as `ARORA_DEVICE` does, when neither
//!   `--device` nor `ARORA_DEVICE` does.
//! - `DEVICE_DIR`, when neither `--device-dir` nor `ARORA_DEVICE_DIR` is
//!   given, is the device directory. Given with a name, it acts as
//!   `ARORA_DEVICE_DIR`. Given alone, the device keeps its Studio credential
//!   where an earlier arora kept it, the directory's `studio/`: an upgraded
//!   runner keeps the credential it had, and two such devices never share
//!   one.
//! - `IDENTITY_FILE`, when no device directory is given either way, makes
//!   the directory `<IDENTITY_FILE>_dir`, which the identity it names
//!   migrates to; given alone, the device keeps its Studio credential in that
//!   directory's `studio/`, as `DEVICE_DIR` alone does.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::warn;

use crate::device_dir;

/// The port of the open local bridge of a device that gives none.
pub const DEFAULT_BRIDGE_PORT: u16 = 9000;

/// The variable naming the device.
pub(crate) const NAME_VAR: &str = "ARORA_DEVICE";
/// The variable naming the device directory.
pub(crate) const DIR_VAR: &str = "ARORA_DEVICE_DIR";
/// The variable giving the open local bridge's port.
pub(crate) const BRIDGE_PORT_VAR: &str = "ARORA_BRIDGE_PORT";

/// The device a run is, as its options and the environment give it: see the
/// [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    name: String,
    dir: PathBuf,
    bridge_port: u16,
    // A build without Studio keeps no credential, and reads none of this.
    #[cfg_attr(not(feature = "studio-bridge"), allow(dead_code))]
    credential: Credential,
}

/// Where a device keeps its Semio Studio credential.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "studio-bridge"), allow(dead_code))]
pub(crate) enum Credential {
    /// Under its name, in Semio's device store; one its directory's `studio/`
    /// holds moves in once. With `identity_file`, the deprecated
    /// `IDENTITY_FILE`, that identity migrates to the directory's `studio/`
    /// first.
    Store { identity_file: Option<PathBuf> },
    /// In its directory's `studio/`: the deprecated `DEVICE_DIR` or
    /// `IDENTITY_FILE` given without a name.
    DeviceDir { identity_file: Option<PathBuf> },
    /// None: a directory given without a name. A run that connects to Studio
    /// is refused.
    Unnamed,
}

impl Device {
    /// The device the environment configures: what a runner's options give
    /// when the run passes none of them ([`crate::RunnerArgs::device`]).
    pub fn from_env() -> Result<Self> {
        use clap::Parser;

        /// A command line passing no option, so each comes from its variable.
        #[derive(Parser)]
        struct Environment {
            #[command(flatten)]
            runner: crate::RunnerArgs,
        }

        Environment::try_parse_from(["arora"])
            .context("the environment does not configure a device")?
            .runner
            .device()
    }

    /// The device's name, local to the machine.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The device directory: what the device keeps of its own from one run
    /// to the next ([`crate::device_dir`]). It is named, not created: a use
    /// creates the subdirectory it writes.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The port the device's open local bridge serves on.
    pub fn bridge_port(&self) -> u16 {
        self.bridge_port
    }

    /// Where the device keeps its Semio Studio credential.
    #[cfg_attr(not(feature = "studio-bridge"), allow(dead_code))]
    pub(crate) fn credential(&self) -> &Credential {
        &self.credential
    }
}

/// The device the options give — `name`, `dir` and `bridge_port`, each from
/// its option or its variable — with the deprecated variables `env` holds.
pub(crate) fn resolve(
    name: Option<String>,
    dir: Option<PathBuf>,
    bridge_port: Option<u16>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Device> {
    // A blank option or variable is not given, as a blank deprecated
    // variable is not.
    let name = name.filter(|name| !name.trim().is_empty());
    let dir = dir.filter(|dir| !dir.as_os_str().is_empty());
    let name = name.or_else(|| {
        let name = env("DEVICE_LOCAL_ID")?;
        warn!("DEVICE_LOCAL_ID is deprecated: name the device with --device or {NAME_VAR}");
        Some(name)
    });
    let named = name.is_some();
    let name = name.unwrap_or_else(|| device_dir::DEFAULT_NAME.to_string());
    device_dir::check_name(&name)?;
    let deprecated_dir = env("DEVICE_DIR").map(PathBuf::from);
    let identity_file = env("IDENTITY_FILE").map(PathBuf::from);
    let (dir, credential) = if let Some(dir) = dir {
        for variable in [
            deprecated_dir.as_ref().map(|_| "DEVICE_DIR"),
            identity_file.as_ref().map(|_| "IDENTITY_FILE"),
        ]
        .into_iter()
        .flatten()
        {
            warn!("{variable} is deprecated, and ignored: the device directory is given");
        }
        let credential = match named {
            true => Credential::Store {
                identity_file: None,
            },
            false => Credential::Unnamed,
        };
        (dir, credential)
    } else if let Some(dir) = deprecated_dir {
        if identity_file.is_some() {
            warn!("IDENTITY_FILE is deprecated, and ignored: DEVICE_DIR is set");
        }
        if named {
            warn!("DEVICE_DIR is deprecated: give the device directory with --device-dir or {DIR_VAR}");
            (
                dir,
                Credential::Store {
                    identity_file: None,
                },
            )
        } else {
            warn!(
                "DEVICE_DIR is deprecated: name the device with --device or {NAME_VAR}, and give \
                 its directory with --device-dir or {DIR_VAR}"
            );
            (
                dir,
                Credential::DeviceDir {
                    identity_file: None,
                },
            )
        }
    } else if let Some(file) = identity_file {
        let dir = device_dir::of_identity_file(&file);
        warn!(
            "IDENTITY_FILE is deprecated: --device-dir or {DIR_VAR} names the device directory. \
             This run's is {}, the directory the identity {} migrates to",
            dir.display(),
            file.display()
        );
        let identity_file = Some(file);
        let credential = match named {
            true => Credential::Store { identity_file },
            false => Credential::DeviceDir { identity_file },
        };
        (dir, credential)
    } else {
        (
            device_dir::of(&name)?,
            Credential::Store {
                identity_file: None,
            },
        )
    };
    Ok(Device {
        name,
        dir,
        bridge_port: bridge_port.unwrap_or(DEFAULT_BRIDGE_PORT),
        credential,
    })
}

/// An environment variable's value, trimmed, or `None` if unset or blank.
pub(crate) fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// An environment setting `vars`, and nothing else.
    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |key| vars.get(key).cloned()
    }

    fn store() -> Credential {
        Credential::Store {
            identity_file: None,
        }
    }

    #[test]
    fn a_device_given_nothing_is_the_default_one_on_the_default_port() {
        let device = resolve(None, None, None, &env(&[])).unwrap();
        assert_eq!(device.name(), "default");
        assert!(
            device.dir().ends_with("semio/arora/devices/default"),
            "{}",
            device.dir().display()
        );
        assert_eq!(device.bridge_port(), 9000);
        assert_eq!(device.credential(), &store());
    }

    #[test]
    fn a_name_names_the_directory_and_keys_the_credential() {
        let device = resolve(Some("bench-2".into()), None, Some(9001), &env(&[])).unwrap();
        assert_eq!(device.name(), "bench-2");
        assert!(device.dir().ends_with("semio/arora/devices/bench-2"));
        assert_eq!(device.bridge_port(), 9001);
        assert_eq!(device.credential(), &store());
    }

    #[test]
    fn a_blank_name_or_directory_is_not_given() {
        let device = resolve(Some(" ".into()), Some("".into()), None, &env(&[])).unwrap();
        assert_eq!(device.name(), "default");
        assert!(device.dir().ends_with("semio/arora/devices/default"));
        assert_eq!(device.credential(), &store());
    }

    #[test]
    fn a_name_that_is_not_one_path_component_is_refused() {
        for bad in ["..", "a/b", "a\\b"] {
            assert!(
                resolve(Some(bad.into()), Some("/d".into()), None, &env(&[])).is_err(),
                "{bad:?} is refused"
            );
        }
    }

    #[test]
    fn a_directory_given_with_a_name_keeps_the_credential_in_the_store() {
        let device = resolve(
            Some("bench".into()),
            Some("/srv/bench".into()),
            None,
            &env(&[]),
        )
        .unwrap();
        assert_eq!(device.dir(), Path::new("/srv/bench"));
        assert_eq!(device.credential(), &store());
    }

    #[test]
    fn a_directory_given_without_a_name_is_unnamed() {
        let device = resolve(None, Some("/srv/bench".into()), None, &env(&[])).unwrap();
        assert_eq!(device.name(), "default");
        assert_eq!(device.dir(), Path::new("/srv/bench"));
        assert_eq!(device.credential(), &Credential::Unnamed);
    }

    #[test]
    fn device_local_id_names_the_device_unless_a_name_is_given() {
        let local_id = env(&[("DEVICE_LOCAL_ID", "old")]);
        let device = resolve(None, None, None, &local_id).unwrap();
        assert_eq!(device.name(), "old");
        assert!(device.dir().ends_with("semio/arora/devices/old"));
        let device = resolve(None, Some("/d".into()), None, &local_id).unwrap();
        assert_eq!(
            device.credential(),
            &store(),
            "it names the device, as ARORA_DEVICE does"
        );
        let device = resolve(Some("new".into()), None, None, &local_id).unwrap();
        assert_eq!(device.name(), "new");
    }

    #[test]
    fn device_dir_alone_keeps_the_credential_in_the_directory() {
        let device = resolve(None, None, None, &env(&[("DEVICE_DIR", "/srv/old")])).unwrap();
        assert_eq!(device.dir(), Path::new("/srv/old"));
        assert_eq!(
            device.credential(),
            &Credential::DeviceDir {
                identity_file: None
            }
        );
    }

    #[test]
    fn device_dir_with_a_name_is_the_device_directory() {
        let device = resolve(
            Some("bench".into()),
            None,
            None,
            &env(&[("DEVICE_DIR", "/srv/old")]),
        )
        .unwrap();
        assert_eq!(device.dir(), Path::new("/srv/old"));
        assert_eq!(device.credential(), &store());
        let device = resolve(
            None,
            None,
            None,
            &env(&[("DEVICE_DIR", "/srv/old"), ("DEVICE_LOCAL_ID", "bench")]),
        )
        .unwrap();
        assert_eq!(device.credential(), &store());
    }

    #[test]
    fn a_given_directory_wins_over_the_deprecated_variables() {
        let device = resolve(
            None,
            Some("/srv/new".into()),
            None,
            &env(&[("DEVICE_DIR", "/srv/old"), ("IDENTITY_FILE", "/etc/id")]),
        )
        .unwrap();
        assert_eq!(device.dir(), Path::new("/srv/new"));
        assert_eq!(device.credential(), &Credential::Unnamed);
    }

    #[test]
    fn an_identity_file_s_directory_is_beside_it() {
        let file = env(&[("IDENTITY_FILE", "/etc/arora/identity")]);
        let device = resolve(None, None, None, &file).unwrap();
        assert_eq!(device.dir(), Path::new("/etc/arora/identity_dir"));
        assert_eq!(
            device.credential(),
            &Credential::DeviceDir {
                identity_file: Some("/etc/arora/identity".into())
            }
        );
        let device = resolve(Some("bench".into()), None, None, &file).unwrap();
        assert_eq!(device.dir(), Path::new("/etc/arora/identity_dir"));
        assert_eq!(
            device.credential(),
            &Credential::Store {
                identity_file: Some("/etc/arora/identity".into())
            }
        );
        let device = resolve(
            None,
            None,
            None,
            &env(&[("IDENTITY_FILE", "/etc/id"), ("DEVICE_DIR", "/srv/old")]),
        )
        .unwrap();
        assert_eq!(device.dir(), Path::new("/srv/old"), "DEVICE_DIR wins");
    }
}
