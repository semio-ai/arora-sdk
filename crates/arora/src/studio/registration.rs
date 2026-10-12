//! What a device registers with Studio as — its owners, its name, and the
//! model family the operator typed — remembered from the run the operator
//! answered for it, so that a later run asks nothing.
//!
//! The registration is kept in `device.json`, in the `studio/` subdirectory of
//! the device directory ([`crate::device_dir`]), beside the credentials:
//!
//! ```json
//! {
//!   "owners": ["<owner uid>"],
//!   "name": "pepper-lab",
//!   "model_family": "pepper"
//! }
//! ```
//!
//! `model_family` is absent when the operator typed none. Deleting the file
//! makes the next interactive run ask again.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use log::warn;
use serde::{Deserialize, Serialize};

use super::credentials::STUDIO_DIR;

/// The file the registration is kept in, in the device directory's `studio/`.
const FILE: &str = "device.json";

/// A device's saved registration: what the operator answered, or what the
/// environment gave, for the fields an interactive run resolves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Registration {
    /// The Studio owner UIDs; never empty in a saved registration.
    pub owners: Vec<String>,
    /// The device name; never blank in a saved registration.
    pub name: String,
    /// The model family the operator typed, when they typed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_family: Option<String>,
}

/// The registration file of the device whose directory is `device_dir`.
pub(super) fn path_in(device_dir: &Path) -> PathBuf {
    device_dir.join(STUDIO_DIR).join(FILE)
}

/// The registration saved at `path`, or `None` when there is none. A file
/// that cannot be read, or that names no owner or no name, is ignored with a
/// warning: the run then resolves the registration as if none was saved.
pub(super) fn load(path: &Path) -> Option<Registration> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            warn!(
                "studio: could not read the saved registration {}: {e}; ignoring it",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str::<Registration>(&text) {
        Ok(registration)
            if !registration.owners.is_empty() && !registration.name.trim().is_empty() =>
        {
            Some(registration)
        }
        Ok(_) => {
            warn!(
                "studio: the saved registration {} names no owner or no name; ignoring it",
                path.display()
            );
            None
        }
        Err(e) => {
            warn!(
                "studio: {} is not a saved registration ({e}); ignoring it",
                path.display()
            );
            None
        }
    }
}

/// Saves `registration` at `path`, creating its directory if missing. As with
/// the credentials beside it, the directory and the file are the current
/// user's alone on Unix. The file is written beside `path` and renamed over
/// it, so a reader never sees a partial one.
pub(super) fn save(path: &Path, registration: &Registration) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", path.display()),
        )
    })?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;

    let mut json = serde_json::to_string_pretty(registration)?;
    json.push('\n');
    let partial = dir.join(format!("{FILE}.partial"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&partial)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh device directory under the system temp dir, unique to `name`
    /// and this process.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arora-studio-registration-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn registration(model_family: Option<&str>) -> Registration {
        Registration {
            owners: vec!["uid-1".to_string(), "uid-2".to_string()],
            name: "pepper-lab".to_string(),
            model_family: model_family.map(str::to_string),
        }
    }

    #[test]
    fn a_saved_registration_reads_back() {
        let device = scratch("round-trip");
        let path = path_in(&device);
        assert_eq!(path, device.join("studio").join("device.json"));
        assert_eq!(load(&path), None, "nothing is saved yet");

        for saved in [registration(Some("pepper")), registration(None)] {
            save(&path, &saved).unwrap();
            assert_eq!(load(&path), Some(saved));
        }
        assert!(!device.join("studio").join("device.json.partial").exists());
        fs::remove_dir_all(&device).unwrap();
    }

    #[test]
    fn the_file_is_plain_json_without_an_absent_model_family() {
        let device = scratch("json");
        let path = path_in(&device);
        save(&path, &registration(None)).unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"owners": ["uid-1", "uid-2"], "name": "pepper-lab"})
        );
        fs::remove_dir_all(&device).unwrap();
    }

    #[test]
    fn a_damaged_or_incomplete_file_is_ignored() {
        let device = scratch("damaged");
        let path = path_in(&device);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in [
            "not json",
            r#"{"owners": [], "name": "pepper-lab"}"#,
            r#"{"owners": ["uid-1"], "name": " "}"#,
            r#"{"owners": ["uid-1"]}"#,
        ] {
            fs::write(&path, text).unwrap();
            assert_eq!(load(&path), None, "{text} is ignored");
        }
        fs::remove_dir_all(&device).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_the_user_s_alone() {
        use std::os::unix::fs::PermissionsExt;
        let device = scratch("private");
        let path = path_in(&device);
        save(&path, &registration(None)).unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
        fs::remove_dir_all(&device).unwrap();
    }
}
