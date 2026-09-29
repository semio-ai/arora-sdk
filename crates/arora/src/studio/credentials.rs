//! The Studio connection's credentials: the `studio/` subdirectory of the
//! device directory ([`crate::device_dir`]), which arora names and
//! [`DeviceCredentials`] alone writes inside.
//!
//! Two older layouts come in here, both in [`DeviceCredentials`]' format:
//!
//! - **arora 11.0 and earlier** kept the credentials in `.semio/arora` under
//!   the first writable directory among the executable's directory, the home
//!   directory and the current directory. They were the one device a host
//!   ran, so they move into the device that sets no local id, once.
//! - **`IDENTITY_FILE`**, deprecated, named the refresh token's file alone,
//!   its key staying in that legacy directory. The device directory of a run
//!   that sets it is `<IDENTITY_FILE>_dir`, into which the file is copied with
//!   its key. The file is neither updated nor removed.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use arora_studio_bridge_client::credentials::DeviceCredentials;
use log::{info, warn};

use crate::device_dir;

/// The Studio connection's subdirectory of the device directory.
const STUDIO_DIR: &str = "studio";

/// The Studio credentials of the device whose directory is `device_dir`,
/// created if missing.
pub(super) fn in_device_dir(device_dir: &Path) -> Result<DeviceCredentials> {
    let dir = device_dir.join(STUDIO_DIR);
    DeviceCredentials::in_dir(&dir).with_context(|| {
        format!(
            "could not create the Studio credentials directory {}",
            dir.display()
        )
    })
}

/// The Studio credentials of a run configured from the environment: in the
/// device directory `DEVICE_DIR` names; else, when the deprecated
/// `IDENTITY_FILE` is set, in the directory it migrates to; else in the
/// per-user directory of the device `DEVICE_LOCAL_ID` names.
pub(super) fn from_env() -> Result<DeviceCredentials> {
    let identity_file = super::env_nonempty("IDENTITY_FILE").map(PathBuf::from);
    if let Some(dir) = device_dir::override_from_env() {
        if identity_file.is_some() {
            warn!("IDENTITY_FILE is deprecated, and ignored: DEVICE_DIR is set");
        }
        return in_device_dir(&dir);
    }
    if let Some(file) = identity_file {
        // The key the file is encrypted under stayed in a legacy directory,
        // unless the default device has since adopted it.
        let mut key_dirs = legacy_dirs();
        if let Ok(default) = device_dir::of(device_dir::DEFAULT_LOCAL_ID) {
            key_dirs.push(default.join(STUDIO_DIR));
        }
        return in_device_dir(&identity_file_dir(&file, &key_dirs)?);
    }
    let local_id = device_dir::local_id_from_env();
    let credentials = in_device_dir(&device_dir::of(&local_id)?)?;
    if local_id == device_dir::DEFAULT_LOCAL_ID {
        adopt_legacy(&credentials, &legacy_dirs())?;
    }
    Ok(credentials)
}

/// The legacy credentials locations: `.semio/arora` under the executable's
/// directory, the home directory and the current directory, in the order
/// arora 11.0 and earlier chose among them (the first writable one).
fn legacy_dirs() -> Vec<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    [exe_dir, dirs::home_dir(), std::env::current_dir().ok()]
        .into_iter()
        .flatten()
        .map(|dir| dir.join(".semio").join("arora"))
        .collect()
}

/// Moves the credentials of the first of `legacy` holding any into
/// `credentials`, when it holds none, so the device keeps its Studio
/// identity. The `.semio` directory a move empties is removed.
fn adopt_legacy(credentials: &DeviceCredentials, legacy: &[PathBuf]) -> Result<()> {
    for old in legacy {
        let adopted = credentials.adopt(old).with_context(|| {
            format!("could not move the Studio credentials in {}", old.display())
        })?;
        if adopted {
            if let Some(semio) = old.parent() {
                // `remove_dir` removes only an empty directory.
                let _ = fs::remove_dir(semio);
            }
            info!("moved the Studio credentials in {}", old.display());
            return Ok(());
        }
    }
    Ok(())
}

/// The device directory of a run that sets the deprecated `IDENTITY_FILE`,
/// `file`: `<file>_dir`, which the identity migrates to.
///
/// - The file present, the directory absent: the file is copied in, with the
///   first key among `key_dirs` it decrypts under.
/// - Both present: the directory is used.
/// - The file absent, the directory present: the directory is used.
/// - Both absent: an error.
///
/// Each case warns that `IDENTITY_FILE` is deprecated, and the file, when
/// present, that it is no longer read nor updated and can be removed.
fn identity_file_dir(file: &Path, key_dirs: &[PathBuf]) -> Result<PathBuf> {
    let dir = with_suffix(file, "_dir");
    warn!(
        "IDENTITY_FILE is deprecated: DEVICE_DIR names the directory holding what the \
         device keeps. The identity IDENTITY_FILE names ({}) migrates to {}",
        file.display(),
        dir.display()
    );
    match (file.is_file(), dir.is_dir()) {
        (true, true) => warn!(
            "{} is no longer read nor updated, and can be removed: the identity is in {}",
            file.display(),
            dir.display()
        ),
        (true, false) => {
            migrate_identity_file(file, &dir, key_dirs)?;
            warn!(
                "{} migrated to {}; it is no longer read nor updated, and can be removed",
                file.display(),
                dir.display()
            );
        }
        (false, true) => warn!(
            "{} is absent: falling back to {}",
            file.display(),
            dir.display()
        ),
        (false, false) => bail!(
            "neither IDENTITY_FILE ({}) nor the directory it migrates to ({}) was found",
            file.display(),
            dir.display()
        ),
    }
    Ok(dir)
}

/// Copies the refresh token `file` holds into the Studio credentials of the
/// device directory `dir`, with the first key among `key_dirs` it decrypts
/// under. The device directory is assembled as `<dir>.partial` and renamed to
/// `dir` once the copy succeeds, so an interrupted migration leaves no `dir`
/// behind.
fn migrate_identity_file(file: &Path, dir: &Path, key_dirs: &[PathBuf]) -> Result<()> {
    let partial = with_suffix(dir, ".partial");
    let _ = fs::remove_dir_all(&partial);
    let copied = in_device_dir(&partial)?
        .copy_refresh_token_from(file, key_dirs)
        .with_context(|| format!("could not copy {} in", file.display()))?;
    if !copied {
        let _ = fs::remove_dir_all(&partial);
        let searched: Vec<String> = key_dirs
            .iter()
            .map(|key_dir| key_dir.display().to_string())
            .collect();
        bail!(
            "IDENTITY_FILE ({}) cannot migrate: it decrypts under the key of none of {}",
            file.display(),
            searched.join(", ")
        );
    }
    fs::rename(&partial, dir)?;
    info!("migrated {} to {}", file.display(), dir.display());
    Ok(())
}

/// `path` with `suffix` appended to its last component.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir, unique to `name` and this
    /// process.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arora-studio-credentials-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Credentials in `dir` holding `token`.
    fn holding(dir: &Path, token: &str) -> DeviceCredentials {
        let credentials = DeviceCredentials::in_dir(dir).unwrap();
        credentials.save_refresh_token(token).unwrap();
        credentials
    }

    /// A legacy `.semio/arora` under `root` holding `token`.
    fn legacy_identity(root: &Path, token: &str) -> PathBuf {
        let old = root.join(".semio").join("arora");
        holding(&old, token);
        old
    }

    fn token_of(device_dir: &Path) -> Option<String> {
        in_device_dir(device_dir).unwrap().refresh_token().unwrap()
    }

    #[test]
    fn legacy_credentials_move_into_the_device() {
        let root = scratch("moves");
        let old = legacy_identity(&root, "t");
        let device = root.join("devices").join("default");

        adopt_legacy(&in_device_dir(&device).unwrap(), std::slice::from_ref(&old)).unwrap();

        assert_eq!(token_of(&device), Some("t".to_string()));
        assert!(!old.exists(), "the emptied legacy directory is removed");
        assert!(
            !root.join(".semio").exists(),
            "and so is its emptied parent"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_first_legacy_directory_holding_credentials_is_the_one_adopted() {
        let root = scratch("first");
        let empty = root.join("exe").join(".semio").join("arora");
        fs::create_dir_all(&empty).unwrap();
        let home = legacy_identity(&root.join("home"), "home");
        let cwd = legacy_identity(&root.join("cwd"), "cwd");
        let device = root.join("device");

        adopt_legacy(
            &in_device_dir(&device).unwrap(),
            &[empty, home, cwd.clone()],
        )
        .unwrap();

        assert_eq!(token_of(&device), Some("home".to_string()));
        assert_eq!(
            DeviceCredentials::in_dir(&cwd)
                .unwrap()
                .refresh_token()
                .unwrap(),
            Some("cwd".to_string()),
            "the other legacy credentials are left alone"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_device_holding_credentials_adopts_none() {
        let root = scratch("keeps");
        let old = legacy_identity(&root, "old");
        let device = root.join("device");
        holding(&device.join(STUDIO_DIR), "new");

        adopt_legacy(&in_device_dir(&device).unwrap(), std::slice::from_ref(&old)).unwrap();

        assert_eq!(token_of(&device), Some("new".to_string()));
        assert!(old.exists(), "the legacy credentials are left alone");
        fs::remove_dir_all(&root).unwrap();
    }

    /// An `IDENTITY_FILE` layout: the key in a legacy directory, the token it
    /// encrypts in `<root>/token`.
    fn identity_file_layout(root: &Path, token: &str) -> (PathBuf, PathBuf) {
        let key_dir = legacy_identity(root, token);
        let file = root.join("token");
        // `refresh_token` beside `key`: the legacy layout arora 11.0 wrote.
        fs::rename(key_dir.join("refresh_token"), &file).unwrap();
        (file, key_dir)
    }

    #[test]
    fn an_identity_file_migrates_next_to_itself_and_stays_untouched() {
        let root = scratch("file-migrates");
        let (file, key_dir) = identity_file_layout(&root, "t");
        let before = fs::read(&file).unwrap();

        let dir = identity_file_dir(&file, std::slice::from_ref(&key_dir)).unwrap();

        assert_eq!(dir, root.join("token_dir"));
        assert_eq!(token_of(&dir), Some("t".to_string()));
        assert_eq!(fs::read(&file).unwrap(), before, "the file is not touched");
        assert!(!root.join("token_dir.partial").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_identity_file_migrates_with_the_key_it_decrypts_under() {
        let root = scratch("file-key");
        let other = legacy_identity(&root.join("other"), "other");
        let (file, key_dir) = identity_file_layout(&root, "t");

        let dir = identity_file_dir(&file, &[other, key_dir]).unwrap();

        assert_eq!(token_of(&dir), Some("t".to_string()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_identity_file_no_key_decrypts_does_not_migrate() {
        let root = scratch("file-no-key");
        let other = legacy_identity(&root.join("other"), "other");
        let (file, _) = identity_file_layout(&root, "t");

        assert!(identity_file_dir(&file, &[other]).is_err());
        assert!(!root.join("token_dir").exists());
        assert!(!root.join("token_dir.partial").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_migrated_directory_is_used_whether_or_not_the_file_remains() {
        let root = scratch("file-dir");
        let (file, key_dir) = identity_file_layout(&root, "t");
        let dir = identity_file_dir(&file, std::slice::from_ref(&key_dir)).unwrap();
        // The directory is maintained from here on; the file is not read.
        in_device_dir(&dir)
            .unwrap()
            .save_refresh_token("rotated")
            .unwrap();

        assert_eq!(identity_file_dir(&file, &[]).unwrap(), dir);
        fs::remove_file(&file).unwrap();
        assert_eq!(identity_file_dir(&file, &[]).unwrap(), dir);
        assert_eq!(token_of(&dir), Some("rotated".to_string()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn neither_the_identity_file_nor_its_directory_is_an_error() {
        let root = scratch("file-neither");
        let error = identity_file_dir(&root.join("token"), &[]).unwrap_err();
        assert!(error.to_string().contains("neither"), "{error}");
        fs::remove_dir_all(&root).unwrap();
    }
}
