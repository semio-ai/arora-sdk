//! App-data directory discovery.
//!
//! Ported ~verbatim from `studio-bridge/headless/src/app_data_files.rs`. Picks
//! the first writable directory among the executable's directory, the home
//! directory, then the current working directory, and stores arora's per-device
//! data (encryption key + refresh token) under `.semio/arora` there.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use dirs::home_dir;

fn is_dir_is_writable(dir: &Path) -> bool {
    let test_file = dir.join("test_file");
    match File::create(&test_file) {
        Ok(_) => {
            fs::remove_file(&test_file).expect("Could not remove test file");
            true
        }
        Err(_) => false,
    }
}

fn find_writable_dir() -> Result<PathBuf, std::io::Error> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            if is_dir_is_writable(dir) {
                return Ok(dir.into());
            }
        }
    }

    if let Some(home) = home_dir() {
        if is_dir_is_writable(&home) {
            return Ok(home);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        if is_dir_is_writable(&cwd) {
            return Ok(cwd);
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "Could not find a writable directory",
    ))
}

pub fn ensure_app_data_dir() -> Result<PathBuf, std::io::Error> {
    let mut app_data_dir = find_writable_dir()?;
    app_data_dir.push(".semio");
    app_data_dir.push("arora");
    fs::create_dir_all(&app_data_dir)?;
    Ok(app_data_dir)
}
