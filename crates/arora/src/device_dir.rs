//! A device's directory: what the device keeps of its own from one run to
//! the next. Each use keeps a subdirectory of its own: the modules the device
//! loads at start in `modules/` ([`crate::module_dir`]), the Studio
//! connection's credentials in `studio/`.
//!
//! ```text
//! <data_local_dir>/semio/arora/
//! └── devices/
//!     └── <local id>/      the device directory; DEVICE_DIR replaces this whole path
//!         ├── modules/     the module directories the device loads at start
//!         └── studio/      the Studio connection's credentials
//! ```
//!
//! `<data_local_dir>` is per user: `~/Library/Application Support` on macOS,
//! `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows. The local id tells
//! the devices of one user on one host apart; `DEVICE_LOCAL_ID` sets it, and a
//! device that sets none is [`DEFAULT_LOCAL_ID`]. It is local only: the name a
//! device shows (`DEVICE_NAME`) can change without making it another device,
//! and the id Studio knows it by is assigned when it first signs in.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The local id of a device that sets none.
pub const DEFAULT_LOCAL_ID: &str = "default";

/// The per-user directory of the device whose local id is `local_id`:
/// `<data_local_dir>/semio/arora/devices/<local_id>`. The id is one path
/// component: not empty, not `.` or `..`, and free of path separators.
pub fn of(local_id: &str) -> Result<PathBuf> {
    if local_id.is_empty() || local_id == "." || local_id == ".." || local_id.contains(['/', '\\'])
    {
        bail!("a device's local id is one path component, not {local_id:?}");
    }
    Ok(dirs::data_local_dir()
        .context(
            "this platform has no per-user data directory: set DEVICE_DIR, or name the \
             device directory explicitly",
        )?
        .join("semio")
        .join("arora")
        .join("devices")
        .join(local_id))
}

/// The device directory of a run configured from the environment: `DEVICE_DIR`
/// when set; else, while the deprecated `IDENTITY_FILE` is set, the directory
/// its identity migrates to (`<IDENTITY_FILE>_dir`); else the per-user
/// directory of the device `DEVICE_LOCAL_ID` names ([`of`]). The directory
/// is named, not created: a use creates the subdirectory it writes.
pub fn from_env() -> Result<PathBuf> {
    if let Some(dir) = override_from_env() {
        return Ok(dir);
    }
    if let Some(file) = env_nonempty("IDENTITY_FILE") {
        return Ok(of_identity_file(Path::new(&file)));
    }
    of(&local_id_from_env())
}

/// The device directory `DEVICE_DIR` names, when set.
pub(crate) fn override_from_env() -> Option<PathBuf> {
    env_nonempty("DEVICE_DIR").map(PathBuf::from)
}

/// The device directory of a run that sets the deprecated `IDENTITY_FILE` to
/// `file`: `<file>_dir`, which the identity the file holds migrates to.
pub(crate) fn of_identity_file(file: &Path) -> PathBuf {
    with_suffix(file, "_dir")
}

/// `path` with `suffix` appended to its last component.
pub(crate) fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// The local id `DEVICE_LOCAL_ID` sets, or [`DEFAULT_LOCAL_ID`].
pub(crate) fn local_id_from_env() -> String {
    env_nonempty("DEVICE_LOCAL_ID").unwrap_or_else(|| DEFAULT_LOCAL_ID.to_string())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_id_names_one_directory_under_devices() {
        let dir = of("bench-2").unwrap();
        assert!(
            dir.ends_with("semio/arora/devices/bench-2"),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn a_local_id_is_one_path_component() {
        for bad in ["", ".", "..", "a/b", "a\\b"] {
            assert!(of(bad).is_err(), "{bad:?} is refused");
        }
    }

    #[test]
    fn an_identity_file_s_device_directory_is_beside_it() {
        assert_eq!(
            of_identity_file(Path::new("/etc/arora/identity")),
            PathBuf::from("/etc/arora/identity_dir")
        );
    }
}
