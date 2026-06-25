use std::env;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

fn main() -> Result<()> {
    // Re-run if any of these change.
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?);
    println!("cargo:rerun-if-changed=CMakeLists.txt");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=module.yaml");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");

    // Locate the WASI SDK (downloads if needed).
    let sdk_root = wasi_sdk::locate_or_download().context("locating or downloading WASI SDK")?;
    let toolchain_file = wasi_sdk::cmake_toolchain_file(&sdk_root);

    // Resolve the bindeps artifact paths.
    let arora_module_cli_src = env_path("CARGO_BIN_FILE_ARORA_MODULE_CLI")?;
    let arora_module_cpp_src = env_path("CARGO_BIN_FILE_ARORA_MODULE_CPP")?;
    let arora_buffers_lib = staticlib_artifact("ARORA_BUFFERS")?;
    let arora_util_lib = staticlib_artifact("ARORA_UTIL")?;

    // Stage host generators side-by-side under OUT_DIR. arora-module-cli
    // discovers the language generator by appending its --language argument
    // to its own exe directory ("arora-module-cpp"), so the cli + cpp bins
    // must live in the same directory with their canonical names.
    let out_dir = PathBuf::from(env::var("OUT_DIR").context("OUT_DIR not set")?);
    let tools_dir = out_dir.join("arora-tools");
    std::fs::create_dir_all(&tools_dir).ok();
    let arora_module_cli = tools_dir.join("arora-module-cli");
    let arora_module_cpp = tools_dir.join("arora-module-cpp");
    copy_executable(&arora_module_cli_src, &arora_module_cli)?;
    copy_executable(&arora_module_cpp_src, &arora_module_cpp)?;

    let workspace_root = workspace_root(&manifest_dir)?;
    let arora_cpp_source = workspace_root.join("libs").join("cpp");
    let arora_include_dir = workspace_root.join("target").join("include");

    // Build via cmake. We must override target/host flags: cmake-rs picks up
    // the build script's TARGET (host triple) and injects --target=arm64-apple-macosx
    // plus CMAKE_OSX_ARCHITECTURES=arm64 by default, which conflicts with the WASI
    // toolchain. target("wasm32-wasi") suppresses the OSX flags; no_default_flags
    // stops the C/CXX/ASM_FLAGS injection so the toolchain file's flags win.
    let dst = cmake::Config::new(&manifest_dir)
        .target("wasm32-wasi")
        .host("wasm32-wasi")
        .no_default_flags(true)
        .define("CMAKE_TOOLCHAIN_FILE", &toolchain_file)
        .define("ARORA_MODULE_CLI", &arora_module_cli)
        .define("ARORA_BUFFERS_LIB", &arora_buffers_lib)
        .define("ARORA_UTIL_LIB", &arora_util_lib)
        .define("ARORA_CPP_SOURCE_DIR", &arora_cpp_source)
        .define("ARORA_INCLUDE_DIR", &arora_include_dir)
        .build_target("test-cpp")
        .very_verbose(false)
        .build();

    // Locate produced wasm. cmake::Config builds in <out>/build by default.
    let wasm = dst.join("build").join("test-cpp");
    if !wasm.exists() {
        return Err(anyhow!("expected wasm at {} but not found", wasm.display()));
    }
    println!("cargo:wasm={}", wasm.display());
    println!("cargo:rustc-env=TEST_CPP_WASM={}", wasm.display());

    // Also drop a copy in a stable location for external consumers.
    let stable = workspace_root
        .join("target")
        .join(env::var("PROFILE").unwrap_or_else(|_| "debug".to_string()))
        .join("modules");
    std::fs::create_dir_all(&stable).ok();
    let stable_wasm = stable.join("test-cpp.wasm");
    std::fs::copy(&wasm, &stable_wasm)
        .with_context(|| format!("copying {} to {}", wasm.display(), stable_wasm.display()))?;
    println!("cargo:wasm-stable={}", stable_wasm.display());

    // Re-publish the generated records dir at a stable path. Downstream
    // modules that import types from this module (e.g. test-cpp-2) need to
    // be able to pass it as an --include to arora-module-cli.
    let generated_records = dst.join("build").join("arora").join("records");
    let stable_records = stable.join("test-cpp").join("records");
    if generated_records.is_dir() {
        let _ = std::fs::remove_dir_all(&stable_records);
        std::fs::create_dir_all(&stable_records).ok();
        copy_dir_recursive(&generated_records, &stable_records)?;
        println!("cargo:records={}", stable_records.display());
    }

    // Publish the generated module.yaml too — integration tests pass it
    // as --header to arora-cli.
    let generated_module_yaml = dst.join("build").join("arora").join("module.yaml");
    if generated_module_yaml.is_file() {
        let stable_yaml = stable.join("test-cpp").join("module.yaml");
        std::fs::copy(&generated_module_yaml, &stable_yaml).ok();
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).ok();
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn env_path(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("{name} not set; bindeps may not be enabled (-Z bindeps)"))
}

/// Resolve a cross-target `staticlib` artifact dependency's `.a` file via
/// `CARGO_STATICLIB_DIR_<DEP>`. For dash-named lib crates cargo does not set the
/// bare `CARGO_STATICLIB_FILE_<DEP>` (lib `arora_buffers` ≠ dep `arora-buffers`),
/// so the directory is the reliable source. `dep` is the upper-cased dep name.
fn staticlib_artifact(dep: &str) -> Result<PathBuf> {
    if let Some(p) = env::var_os(format!("CARGO_STATICLIB_FILE_{dep}")) {
        return Ok(PathBuf::from(p));
    }
    let dir = env::var_os(format!("CARGO_STATICLIB_DIR_{dep}")).ok_or_else(|| {
        anyhow!("CARGO_STATICLIB_DIR_{dep} not set; bindeps may not be enabled (-Z bindeps)")
    })?;
    let dir = PathBuf::from(dir);
    std::fs::read_dir(&dir)
        .with_context(|| format!("reading staticlib dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("a"))
        .ok_or_else(|| anyhow!("no .a staticlib found in {}", dir.display()))
}

#[cfg(unix)]
fn copy_executable(src: &Path, dst: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::copy(src, dst)
        .with_context(|| format!("copying {} to {}", src.display(), dst.display()))?;
    let mut perms = std::fs::metadata(dst)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(dst, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn copy_executable(src: &Path, dst: &Path) -> Result<()> {
    std::fs::copy(src, dst)
        .with_context(|| format!("copying {} to {}", src.display(), dst.display()))?;
    Ok(())
}

fn workspace_root(manifest_dir: &Path) -> Result<PathBuf> {
    let mut dir = manifest_dir.to_path_buf();
    while dir.pop() {
        let candidate = dir.join("Cargo.toml");
        if candidate.is_file() {
            if let Ok(s) = std::fs::read_to_string(&candidate) {
                if s.contains("[workspace]") {
                    return Ok(dir);
                }
            }
        }
    }
    Err(anyhow!(
        "could not find workspace root above {}",
        manifest_dir.display()
    ))
}
