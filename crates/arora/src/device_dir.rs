//! A device's directory: what the device keeps of its own from one run to
//! the next. Each use keeps a subdirectory of its own: the modules the device
//! loads at start in `modules/` ([`crate::module_dir`]).
//!
//! ```text
//! <data_local_dir>/semio/arora/
//! └── devices/
//!     └── <name>/          the device directory; --device-dir replaces this whole path
//!         └── modules/     the module directories the device loads at start
//! ```
//!
//! `<data_local_dir>` is per user: `~/Library/Application Support` on macOS,
//! `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows. The device's name
//! tells the devices of one user on one host apart ([`crate::Device`] says
//! how a run names it); a device that sets none is
//! [`DEFAULT_NAME`](crate::device_dir::DEFAULT_NAME). It is local only: the
//! name a device shows (`DEVICE_NAME`) can change without making it another
//! device, and the id Studio knows it by is assigned when it enrolls.
//!
//! The device directory holds no Semio Studio credential: Semio's device
//! store keeps it under the device's name (`arora::studio`), except for a
//! device the deprecated `DEVICE_DIR` places without naming it
//! ([`crate::device`]).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The name of a device that sets none.
pub const DEFAULT_NAME: &str = "default";

/// The per-user directory of the device named `name`:
/// `<data_local_dir>/semio/arora/devices/<name>`. The name is one path
/// component, as [`crate::Device`] says.
pub fn of(name: &str) -> Result<PathBuf> {
    check_name(name)?;
    Ok(dirs::data_local_dir()
        .context(
            "this platform has no per-user data directory: give the device directory with \
             --device-dir or ARORA_DEVICE_DIR",
        )?
        .join("semio")
        .join("arora")
        .join("devices")
        .join(name))
}

/// Refuses a device name that is not exactly one normal path component: the
/// empty name, `.`, `..`, a name holding `/` or `\`, and one a platform reads
/// as more than a name, such as `D:bench`, which Windows reads as a path on
/// drive `D:`.
pub(crate) fn check_name(name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    let one_normal = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(component)), None) if component == name
    );
    if !one_normal || name.contains(['/', '\\']) {
        bail!("a device's name is one path component, not {name:?}");
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_names_one_directory_under_devices() {
        let dir = of("bench-2").unwrap();
        assert!(
            dir.ends_with("semio/arora/devices/bench-2"),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn a_name_is_one_path_component() {
        for bad in ["", ".", "..", "a/b", "a\\b", "/", "bench/", "/bench"] {
            assert!(check_name(bad).is_err(), "{bad:?} is refused");
            assert!(of(bad).is_err(), "{bad:?} names no directory");
        }
        for good in ["default", "bench-2", "sim.0", "...", "a b"] {
            assert!(check_name(good).is_ok(), "{good:?} is a name");
        }
    }

    /// `Path::join` reads a name with a drive prefix as another drive's path,
    /// leaving the devices' directory.
    #[cfg(windows)]
    #[test]
    fn a_name_with_a_drive_prefix_is_refused_on_windows() {
        for bad in ["D:bench", "D:", "C:\\bench", "\\\\server\\share"] {
            assert!(check_name(bad).is_err(), "{bad:?} is refused");
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
