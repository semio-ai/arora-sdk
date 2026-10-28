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

    install_once(&cache, &install, download_and_extract)?;
    Ok(install)
}

/// Install the SDK at `install` with `fetch`, unless it is there. Build scripts run
/// concurrently: an exclusive lock on a file beside the install makes them take turns,
/// so the first one fetches and the others find its install once they hold the lock.
fn install_once(
    cache: &Path,
    install: &Path,
    fetch: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<()> {
    fs::create_dir_all(cache).with_context(|| format!("creating {}", cache.display()))?;
    let lock_path = cache.join(format!("wasi-sdk-{VERSION_MAJOR}.lock"));
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    lock.lock()
        .with_context(|| format!("locking {}", lock_path.display()))?;
    if install.join("bin").join(clang_exe_name()).exists() {
        return Ok(());
    }
    fetch(cache, install)
}

/// Path to the WASI SDK's CMake toolchain file for `wasm32-wasip2`, the target
/// the Rust guests build for: it compiles for it and links a component with
/// `wasm-component-ld`.
pub fn cmake_toolchain_file(sdk_root: &Path) -> PathBuf {
    sdk_root.join("share/cmake/wasi-sdk-p2.cmake")
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
    install_extracted(&top.path(), install)?;
    fs::remove_dir_all(&tmp_extract).ok();
    Ok(())
}

/// Move the SDK extracted at `extracted` to `install`. Build scripts download
/// it concurrently, so another may install it there between this one's check
/// and its rename: the rename then fails on the directory the other filled,
/// and that install is kept. A directory there without the SDK in it is
/// replaced; one with the SDK is never removed, as another build may be using
/// it.
fn install_extracted(extracted: &Path, install: &Path) -> Result<()> {
    let installed = || install.join("bin").join(clang_exe_name()).exists();
    if fs::rename(extracted, install).is_ok() || installed() {
        return Ok(());
    }
    fs::remove_dir_all(install).ok();
    match fs::rename(extracted, install) {
        Ok(()) => Ok(()),
        // Another build installed it in between.
        Err(_) if installed() => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("renaming {} to {}", extracted.display(), install.display())),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory for one test, under the system's temporary one.
    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("wasi-sdk-{}-{name}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An SDK, as far as the locator looks: `bin/clang` holding `marker`.
    fn sdk(at: &Path, marker: &str) {
        fs::create_dir_all(at.join("bin")).unwrap();
        fs::write(at.join("bin").join(clang_exe_name()), marker).unwrap();
    }

    fn marker(install: &Path) -> String {
        fs::read_to_string(install.join("bin").join(clang_exe_name())).unwrap()
    }

    #[test]
    fn an_extracted_sdk_is_moved_to_its_install() {
        let dir = scratch("moved");
        let (extracted, install) = (dir.join("extracted"), dir.join("install"));
        sdk(&extracted, "extracted");

        install_extracted(&extracted, &install).unwrap();
        assert_eq!(marker(&install), "extracted");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_sdk_another_build_installed_first_is_kept() {
        let dir = scratch("kept");
        let (extracted, install) = (dir.join("extracted"), dir.join("install"));
        sdk(&extracted, "extracted");
        sdk(&install, "installed");

        install_extracted(&extracted, &install).unwrap();
        assert_eq!(marker(&install), "installed");
        fs::remove_dir_all(&dir).ok();
    }

    /// Build scripts installing at once over a partial directory: one fetches, the others
    /// wait for it and keep its install.
    #[test]
    fn concurrent_installs_fetch_once() {
        let dir = scratch("concurrent");
        let install = dir.join("install");
        fs::create_dir_all(install.join("share")).unwrap();
        let fetches = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let builds: Vec<_> = (0..4)
                .map(|build| {
                    let (dir, install, fetches) = (&dir, &install, &fetches);
                    scope.spawn(move || {
                        install_once(dir, install, |_, install| {
                            fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            let extracted = dir.join(format!("extracted-{build}"));
                            sdk(&extracted, &format!("build {build}"));
                            install_extracted(&extracted, install)
                        })
                    })
                })
                .collect();
            for build in builds {
                build.join().unwrap().unwrap();
            }
        });
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(marker(&install).starts_with("build "));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_without_the_sdk_is_replaced() {
        let dir = scratch("replaced");
        let (extracted, install) = (dir.join("extracted"), dir.join("install"));
        sdk(&extracted, "extracted");
        fs::create_dir_all(install.join("share")).unwrap();
        fs::write(install.join("share").join("partial"), "").unwrap();

        install_extracted(&extracted, &install).unwrap();
        assert_eq!(marker(&install), "extracted");
        fs::remove_dir_all(&dir).ok();
    }
}
