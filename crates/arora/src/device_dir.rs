//! A device's directory: what the device keeps of its own from one run to
//! the next. Each use keeps a subdirectory of its own, the Studio connection
//! its credentials in `studio/`.
//!
//! ```text
//! <data_local_dir>/semio/arora/
//! └── devices/
//!     └── <local id>/      the device directory; DEVICE_DIR replaces this whole path
//!         └── studio/      the Studio connection's credentials
//! ```
//!
//! `<data_local_dir>` is per user: `~/Library/Application Support` on macOS,
//! `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows. The local id tells
//! the devices of one user on one host apart; `DEVICE_LOCAL_ID` sets it, and a
//! device that sets none is [`DEFAULT_LOCAL_ID`]. It is local only: the name a
//! device shows (`DEVICE_NAME`) can change without making it another device,
//! and the id Studio knows it by is assigned when it first signs in.

use std::path::PathBuf;

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

/// The device directory `DEVICE_DIR` names, when set.
pub(crate) fn override_from_env() -> Option<PathBuf> {
    env_nonempty("DEVICE_DIR").map(PathBuf::from)
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
}
