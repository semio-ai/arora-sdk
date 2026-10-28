//! The Studio connection's credentials: the device name's in Semio's device
//! store (`DeviceStore`), which [`DeviceCredentials`] alone writes inside.
//! arora names the device; where its credential lives is the store's.
//!
//! Three older layouts come in here, all in [`DeviceCredentials`]' format:
//!
//! - **arora 11.1 to 15** kept the credentials in the device directory's
//!   `studio/` subdirectory. They move into the store under the device's
//!   name, once, the first time the device connects. Their bridge identity
//!   is removed: the client reissues it.
//! - **arora 11.0 and earlier** kept the credentials in `.semio/arora` under
//!   the first writable directory among the executable's directory, the home
//!   directory and the current directory. They were the one device a host
//!   ran, so they move into the device named `default`, once.
//! - **`IDENTITY_FILE`**, deprecated, named the refresh token's file alone,
//!   its key staying in that legacy directory. The device directory of a run
//!   that sets it is `<IDENTITY_FILE>_dir`, into whose `studio/` the file is
//!   copied with its key. The file is neither updated nor removed.
//!
//! A run given the deprecated `DEVICE_DIR` or `IDENTITY_FILE` without a name
//! keeps its credentials in its device directory's `studio/`
//! ([`crate::device`]).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use arora_studio_bridge_client::auth::{DeviceName, DeviceStore};
use arora_studio_bridge_client::credentials::DeviceCredentials;
use log::{info, warn};

use crate::device::{Credential, Device};
use crate::device_dir;

/// The subdirectory of the device directory an earlier arora kept the Studio
/// connection's credentials in.
pub(super) const STUDIO_DIR: &str = "studio";

/// The Studio credentials of `device`, created if missing: the device name's
/// in the store under `store`, else in the default store
/// (`DeviceStore::from_env`), as [`crate::device`] says. Refuses a device
/// given a directory and no name.
pub(super) fn of(device: &Device, store: Option<&Path>) -> Result<DeviceCredentials> {
    match device.credential() {
        Credential::Unnamed => bail!(
            "this run connects to Semio Studio, and gives the device directory {} but no device \
             name: name this device with --device or ARORA_DEVICE",
            device.dir().display()
        ),
        Credential::DeviceDir { identity_file } => {
            if let Some(file) = identity_file {
                identity_file_dir(file, &identity_key_dirs(None))?;
            }
            info!(
                "Studio: this device keeps its credential in {}, as a device directory the \
                 deprecated DEVICE_DIR or IDENTITY_FILE gives without a device name does",
                device.dir().join(STUDIO_DIR).display()
            );
            in_device_dir(device.dir())
        }
        Credential::Store { identity_file } => {
            let store = store_at(store)?;
            // A file gone, its identity is wherever it migrated to.
            if let Some(file) = identity_file.as_ref().filter(|file| file.is_file()) {
                identity_file_dir(file, &identity_key_dirs(Some(&store)))?;
            }
            in_store(&store, device.name(), device.dir())
        }
    }
}

/// The store under `root`, else the default one.
pub(super) fn store_at(root: Option<&Path>) -> Result<DeviceStore> {
    match root {
        Some(root) => Ok(DeviceStore::at(root)),
        None => DeviceStore::from_env().context("could not find Semio's device store"),
    }
}

/// The Studio credentials of the device `name` in `store`, created if
/// missing. When they hold none, the device directory `device_dir`'s
/// `studio/` moves in, and for the device named `default`, the first legacy
/// `.semio/arora` holding any.
pub(super) fn in_store(
    store: &DeviceStore,
    name: &str,
    device_dir: &Path,
) -> Result<DeviceCredentials> {
    device_dir::check_name(name)?;
    let name = DeviceName::new(name)?;
    let credentials = DeviceCredentials::in_store(store, &name).with_context(|| {
        format!(
            "could not create the Studio credentials directory {}",
            store.dir(&name).display()
        )
    })?;
    adopt_device_dir(&credentials, device_dir)?;
    if name == DeviceName::default() {
        adopt_legacy(&credentials, &legacy_dirs())?;
    }
    Ok(credentials)
}

/// The Studio credentials kept in the device directory `device_dir`'s
/// `studio/`, created if missing.
pub(super) fn in_device_dir(device_dir: &Path) -> Result<DeviceCredentials> {
    let dir = device_dir.join(STUDIO_DIR);
    DeviceCredentials::in_dir(&dir).with_context(|| {
        format!(
            "could not create the Studio credentials directory {}",
            dir.display()
        )
    })
}

/// Moves the credentials the device directory `device_dir`'s `studio/` holds
/// into `credentials`, when it holds none. The bridge identity left behind is
/// removed, the client reissuing it, and so is the emptied `studio/`.
fn adopt_device_dir(credentials: &DeviceCredentials, device_dir: &Path) -> Result<()> {
    let old = device_dir.join(STUDIO_DIR);
    if !old.is_dir() {
        return Ok(());
    }
    let moved = credentials
        .adopt(&old)
        .with_context(|| format!("could not move the Studio credentials in {}", old.display()))?;
    if !moved {
        return Ok(());
    }
    if old.is_dir() {
        let bridge = DeviceCredentials::in_dir(&old)?.bridge_identity_files();
        for file in [bridge.private_key, bridge.certificate, bridge.ca_bundle] {
            match fs::remove_file(&file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    warn!("could not remove {}: {error}", file.display())
                }
                _ => {}
            }
        }
        // `remove_dir` removes only an empty directory.
        let _ = fs::remove_dir(&old);
    }
    info!(
        "Studio: moved this device's credential from {} to {}",
        old.display(),
        credentials.path().display()
    );
    Ok(())
}

/// The directories whose key the identity `IDENTITY_FILE` names may decrypt
/// under: the legacy ones it stayed in, unless the device named `default`
/// has since adopted it — into `store`, or, by an earlier arora, into its
/// device directory's `studio/`.
fn identity_key_dirs(store: Option<&DeviceStore>) -> Vec<PathBuf> {
    let mut key_dirs = legacy_dirs();
    let store = store.cloned().or_else(|| DeviceStore::from_env().ok());
    if let Some(store) = store {
        key_dirs.push(store.dir(&DeviceName::default()));
    }
    if let Ok(default) = device_dir::of(device_dir::DEFAULT_NAME) {
        key_dirs.push(default.join(STUDIO_DIR));
    }
    key_dirs
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
    let dir = device_dir::of_identity_file(file);
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
    let partial = device_dir::with_suffix(dir, ".partial");
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

    /// The device `name` (none when `None`) whose directory is `dir`, given
    /// no deprecated variable.
    fn device(name: Option<&str>, dir: &Path) -> Device {
        crate::device::resolve(
            name.map(str::to_string),
            Some(dir.to_path_buf()),
            None,
            &|_| None,
        )
        .unwrap()
    }

    // The devices below are never named `default`: its credentials adopt the
    // legacy `.semio/arora` of the host running the tests.

    #[test]
    fn a_device_s_credential_is_its_name_s_in_the_store() {
        let root = scratch("store");
        let store = root.join("store");
        let dir = root.join("device");

        let credentials = of(&device(Some("bench"), &dir), Some(&store)).unwrap();

        assert_eq!(credentials.path(), store.join("bench"));
        credentials.save_refresh_token("t").unwrap();
        assert!(
            !dir.join(STUDIO_DIR).exists(),
            "nothing is kept in the device directory"
        );
        let other = of(&device(Some("bench-2"), &dir), Some(&store)).unwrap();
        assert_eq!(other.refresh_token().unwrap(), None, "a name is a device");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_credential_in_the_device_directory_moves_into_the_store_once() {
        let root = scratch("moves-to-store");
        let store = root.join("store");
        let dir = root.join("device");
        let old = holding(&dir.join(STUDIO_DIR), "t");
        old.save_bridge_private_key("key").unwrap();
        old.save_bridge_certificate("crt", "ca").unwrap();

        let credentials = of(&device(Some("bench"), &dir), Some(&store)).unwrap();

        assert_eq!(credentials.refresh_token().unwrap(), Some("t".to_string()));
        assert!(
            !dir.join(STUDIO_DIR).exists(),
            "the device directory's studio/ is emptied, its bridge identity reissued, and removed"
        );

        // Once the store holds one, the device directory is not read again.
        holding(&dir.join(STUDIO_DIR), "other");
        let credentials = of(&device(Some("bench"), &dir), Some(&store)).unwrap();
        assert_eq!(credentials.refresh_token().unwrap(), Some("t".to_string()));
        assert_eq!(token_of(&dir), Some("other".to_string()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_run_given_a_directory_and_no_name_is_refused() {
        let root = scratch("unnamed");
        let store = root.join("store");
        let dir = root.join("device");

        let error = of(&device(None, &dir), Some(&store)).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("name this device with --device or ARORA_DEVICE"),
            "{error}"
        );
        assert!(!store.exists(), "the store is not touched");
        assert!(!dir.exists(), "nor the device directory");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn device_dir_alone_keeps_the_credential_in_its_studio_directory() {
        let root = scratch("device-dir-alone");
        let store = root.join("store");
        let dir = root.join("device");
        holding(&dir.join(STUDIO_DIR), "kept");
        let device = crate::device::resolve(None, None, None, &|key: &str| {
            (key == "DEVICE_DIR").then(|| dir.display().to_string())
        })
        .unwrap();

        let credentials = of(&device, Some(&store)).unwrap();

        assert_eq!(credentials.path(), dir.join(STUDIO_DIR));
        assert_eq!(
            credentials.refresh_token().unwrap(),
            Some("kept".to_string())
        );
        assert!(!store.exists(), "no store, no move");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_identity_file_s_migrated_directory_moves_into_the_store_under_a_name() {
        let root = scratch("file-to-store");
        let (file, key_dir) = identity_file_layout(&root, "t");
        let dir = identity_file_dir(&file, std::slice::from_ref(&key_dir)).unwrap();
        let store = DeviceStore::at(root.join("store"));

        let credentials = in_store(&store, "bench", &dir).unwrap();

        assert_eq!(credentials.refresh_token().unwrap(), Some("t".to_string()));
        assert!(!dir.join(STUDIO_DIR).exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_name_that_is_not_one_path_component_names_no_credential() {
        let root = scratch("bad-name");
        let store = DeviceStore::at(root.join("store"));
        for bad in ["", "..", "a/b"] {
            assert!(
                in_store(&store, bad, &root.join("device")).is_err(),
                "{bad:?} is refused"
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn neither_the_identity_file_nor_its_directory_is_an_error() {
        let root = scratch("file-neither");
        let error = identity_file_dir(&root.join("token"), &[]).unwrap_err();
        assert!(error.to_string().contains("neither"), "{error}");
        fs::remove_dir_all(&root).unwrap();
    }

    /// The variable handing the process
    /// [`credentials_resolve_in_the_device_directory_given_as_on_android`]
    /// starts the device directory it passes explicitly, the device store
    /// inside it.
    #[cfg(unix)]
    const ANDROID_CASE_DIR: &str = "ARORA_TEST_ANDROID_CASE_DIR";

    /// The Android case: the executable's directory is not writable, there is
    /// no home directory, and the working directory is `/`, so nothing the
    /// host gives places a device directory. The test runs itself again as
    /// such a process — its executable hard-linked into a read-only
    /// directory, `HOME` unset, `/` its working directory — where the
    /// credentials a connection hands the client resolve in the device store
    /// it is given with a device directory and a name
    /// (`connect_in_device_dir`), and in the directory the deprecated
    /// `DEVICE_DIR` names alone (`connect`), and keep what is saved in them.
    #[cfg(unix)]
    #[test]
    fn credentials_resolve_in_the_device_directory_given_as_on_android() {
        match std::env::var_os(ANDROID_CASE_DIR) {
            Some(given) => resolve_as_on_android(Path::new(&given)),
            None => run_as_on_android(),
        }
    }

    /// Starts this test again as a process in the Android case, and checks
    /// what it saved.
    #[cfg(unix)]
    fn run_as_on_android() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("android");
        let given = root.join("given");
        let named = root.join("named");
        // Beside the executable, so that it hard-links into it.
        let exe = std::env::current_exe().unwrap();
        let bin = exe
            .parent()
            .unwrap()
            .join(format!("arora-android-case-{}", std::process::id()));
        let _ = fs::remove_dir_all(&bin);
        fs::create_dir_all(&bin).unwrap();
        let linked = bin.join(exe.file_name().unwrap());
        fs::hard_link(&exe, &linked).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o555)).unwrap();

        let test = concat!(
            module_path!(),
            "::credentials_resolve_in_the_device_directory_given_as_on_android"
        );
        let output = std::process::Command::new(&linked)
            .args([test.strip_prefix("arora::").unwrap(), "--exact"])
            .env(ANDROID_CASE_DIR, &given)
            .env("DEVICE_DIR", &named)
            .env_remove("HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("DEVICE_LOCAL_ID")
            .env_remove("IDENTITY_FILE")
            .env_remove("ARORA_DEVICE")
            .env_remove("ARORA_DEVICE_DIR")
            .env_remove("ARORA_BRIDGE_PORT")
            .env_remove("SEMIO_DEVICE_STORE")
            .current_dir("/")
            .output()
            .unwrap();

        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        let left: Vec<_> = fs::read_dir(&bin)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        fs::remove_dir_all(&bin).unwrap();
        assert!(
            output.status.success(),
            "the Android-case process failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            left,
            [exe.file_name().unwrap().to_os_string()],
            "nothing is written beside the executable"
        );
        assert_eq!(
            DeviceCredentials::in_dir(android_store(&given).join("android"))
                .unwrap()
                .refresh_token()
                .unwrap(),
            Some("given".to_string())
        );
        assert_eq!(token_of(&named), Some("named".to_string()));
        fs::remove_dir_all(&root).unwrap();
    }

    /// The device store the Android-case process is given, in the device
    /// directory `given`, as an app keeps both in its data directory.
    #[cfg(unix)]
    fn android_store(given: &Path) -> PathBuf {
        given.join("devices")
    }

    /// In the Android-case process: the credentials of the device `android`
    /// in the store inside the device directory `given`, and those of the
    /// environment, resolve where they are told and keep a token.
    #[cfg(unix)]
    fn resolve_as_on_android(given: &Path) {
        assert!(std::env::var_os("HOME").is_none(), "HOME is unset");
        assert_eq!(std::env::current_dir().unwrap(), Path::new("/"));

        let store = android_store(given);
        let explicit = in_store(&DeviceStore::at(&store), "android", given).unwrap();
        assert_eq!(explicit.path(), store.join("android"));
        explicit.save_refresh_token("given").unwrap();

        let named = PathBuf::from(std::env::var_os("DEVICE_DIR").unwrap());
        let from_environment = of(&Device::from_env().unwrap(), None).unwrap();
        assert_eq!(from_environment.path(), named.join(STUDIO_DIR));
        from_environment.save_refresh_token("named").unwrap();
    }
}
