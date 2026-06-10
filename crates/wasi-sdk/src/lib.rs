//! WASI SDK fetcher / locator.
//!
//! Designed to be called from a `build.rs`. Locates a pinned WASI SDK install
//! either from `WASI_SDK_PATH` (caller-provided) or by downloading the
//! upstream release tarball to a shared cache directory.

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

/// Pinned version. Update in lockstep with consumer build scripts.
pub const VERSION_FULL: &str = "33.0";
pub const VERSION_MAJOR: &str = "33";

/// Returns the root of the WASI SDK install. Downloads it if not present.
///
/// Resolution order:
///   1. `WASI_SDK_PATH` env var, if set and pointing at a directory containing
///      `bin/clang`.
///   2. Cache dir under the workspace `target/` directory.
///
/// The cache layout is `<target_dir>/wasi-sdk-<VERSION_MAJOR>/` so multiple
/// consumers share one download.
pub fn locate_or_download() -> Result<PathBuf> {
    if let Ok(path) = env::var("WASI_SDK_PATH") {
        let p = PathBuf::from(path);
        if p.join("bin").join(clang_exe_name()).exists() {
            return Ok(p);
        }
    }

    let cache = cache_dir()?;
    let install = cache.join(format!("wasi-sdk-{VERSION_MAJOR}"));
    if install.join("bin").join(clang_exe_name()).exists() {
        return Ok(install);
    }

    download_and_extract(&cache, &install)?;
    Ok(install)
}

/// Path to the CMake toolchain file shipped with WASI SDK.
pub fn cmake_toolchain_file(sdk_root: &Path) -> PathBuf {
    sdk_root.join("share/cmake/wasi-sdk.cmake")
}

/// Path to clang.
pub fn clang(sdk_root: &Path) -> PathBuf {
    sdk_root.join("bin").join(clang_exe_name())
}

/// Path to clang++.
pub fn clangpp(sdk_root: &Path) -> PathBuf {
    sdk_root.join("bin").join(clangpp_exe_name())
}

fn clang_exe_name() -> &'static str {
    if cfg!(windows) {
        "clang.exe"
    } else {
        "clang"
    }
}

fn clangpp_exe_name() -> &'static str {
    if cfg!(windows) {
        "clang++.exe"
    } else {
        "clang++"
    }
}

fn cache_dir() -> Result<PathBuf> {
    // Prefer CARGO_TARGET_DIR if set; otherwise the conventional target/ next
    // to the workspace Cargo.toml. When called from a build.rs the workspace
    // target dir is the parent of OUT_DIR/../../../../ but that traversal is
    // fragile. Use CARGO_TARGET_DIR when available, fall back to an absolute
    // path derived from CARGO_MANIFEST_DIR.
    if let Ok(t) = env::var("CARGO_TARGET_DIR") {
        return Ok(PathBuf::from(t));
    }
    if let Ok(manifest) = env::var("CARGO_MANIFEST_DIR") {
        // Walk up to find a Cargo.toml that declares [workspace].
        let mut dir = PathBuf::from(manifest);
        while dir.pop() {
            let candidate = dir.join("Cargo.toml");
            if candidate.is_file() {
                if let Ok(s) = fs::read_to_string(&candidate) {
                    if s.contains("[workspace]") {
                        return Ok(dir.join("target"));
                    }
                }
            }
        }
    }
    bail!("could not determine a target directory for the WASI SDK cache")
}

fn download_and_extract(cache: &Path, install: &Path) -> Result<()> {
    fs::create_dir_all(cache).with_context(|| format!("creating {}", cache.display()))?;
    let (os, arch) = host_os_arch()?;
    let url = format!(
        "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-{VERSION_MAJOR}/wasi-sdk-{VERSION_FULL}-{arch}-{os}.tar.gz"
    );
    eprintln!("wasi-sdk: downloading {url}");
    let mut resp = ureq::get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    if resp.status() != 200 {
        bail!("download failed: HTTP {} for {}", resp.status(), url);
    }
    let mut reader = resp.body_mut().as_reader();
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .context("reading response body")?;

    let gz = flate2::read::GzDecoder::new(io::Cursor::new(buf));
    let mut archive = tar::Archive::new(gz);
    // Use OUT_DIR (unique per build invocation) to avoid races when multiple
    // build scripts download WASI SDK concurrently.
    let tmp_base = env::var("OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| cache.to_path_buf());
    let tmp_extract = tmp_base.join(format!(".wasi-sdk-{VERSION_FULL}-extract"));
    if tmp_extract.exists() {
        fs::remove_dir_all(&tmp_extract).ok();
    }
    fs::create_dir_all(&tmp_extract)?;
    archive
        .unpack(&tmp_extract)
        .with_context(|| format!("unpacking into {}", tmp_extract.display()))?;

    // The tarball contains a single top-level directory named
    // `wasi-sdk-<full>-<arch>-<os>`. Move it to `install`.
    let entries: Vec<_> = fs::read_dir(&tmp_extract)?.collect::<io::Result<_>>()?;
    let top = entries
        .iter()
        .find(|e| e.path().is_dir())
        .ok_or_else(|| anyhow!("no top-level dir in extracted tarball"))?;
    // If a concurrent build script already installed it, skip the rename.
    if !install.join("bin").join(clang_exe_name()).exists() {
        if install.exists() {
            fs::remove_dir_all(install).ok();
        }
        fs::rename(top.path(), install).with_context(|| {
            format!("renaming {} to {}", top.path().display(), install.display())
        })?;
    }
    fs::remove_dir_all(&tmp_extract).ok();
    Ok(())
}

fn host_os_arch() -> Result<(&'static str, String)> {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        bail!("unsupported host OS for WASI SDK download")
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64".to_string()
    } else if cfg!(target_arch = "x86_64") {
        "x86_64".to_string()
    } else {
        bail!("unsupported host arch for WASI SDK download")
    };
    Ok((os, arch))
}
