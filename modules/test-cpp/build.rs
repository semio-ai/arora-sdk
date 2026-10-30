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
    // to its own exe directory ("arora-module-cpp", with the platform's
    // executable suffix), so the cli + cpp bins must live in the same
    // directory with their canonical names.
    let out_dir = PathBuf::from(env::var("OUT_DIR").context("OUT_DIR not set")?);
    let tools_dir = out_dir.join("arora-tools");
    std::fs::create_dir_all(&tools_dir).ok();
    let exe = std::env::consts::EXE_SUFFIX;
    let arora_module_cli = tools_dir.join(format!("arora-module-cli{exe}"));
    let arora_module_cpp = tools_dir.join(format!("arora-module-cpp{exe}"));
    copy_executable(&arora_module_cli_src, &arora_module_cli)?;
    copy_executable(&arora_module_cpp_src, &arora_module_cpp)?;

    let workspace_root = workspace_root(&manifest_dir)?;
    let arora_cpp_source = workspace_root.join("libs").join("cpp");
    // The cmake build below reads this, outside the package: declare it so
    // cargo rebuilds on an edit (and CI selection, checked against cargo's
    // records, sees it).
    println!("cargo:rerun-if-changed={}", arora_cpp_source.display());
    // The world the guest is a component of, which libs/cpp links it against.
    println!(
        "cargo:rerun-if-changed={}",
        workspace_root
            .join("crates/arora-engine/wit/arora-module.wit")
            .display()
    );
    // The C headers, generated under the OUT_DIRs of arora-buffers and
    // arora-util. Their `links` metadata names the directories, and reaches
    // this build script through the regular dependencies on them in
    // Cargo.toml: the artifact dependencies carry none. The headers are the
    // same for every target, so the host build's serve the wasm32-wasip2 one.
    let arora_buffers_include = PathBuf::from(
        env::var_os("DEP_ARORA_BUFFERS_INCLUDE").context("DEP_ARORA_BUFFERS_INCLUDE not set")?,
    );
    let arora_util_include = PathBuf::from(
        env::var_os("DEP_ARORA_UTIL_INCLUDE").context("DEP_ARORA_UTIL_INCLUDE not set")?,
    );

    // The module directory a device loads the module from, which the cmake
    // build writes (`arora_module_directory`, libs/cpp/CMakeLists.txt).
    // test-cpp-2 reads this module's records from its `records/`.
    let module_dir = workspace_root
        .join("target")
        .join(env::var("PROFILE").unwrap_or_else(|_| "debug".to_string()))
        .join("modules")
        .join("test-cpp");

    // Build via cmake. We must override target/host flags: cmake-rs picks up
    // the build script's TARGET (host triple) and injects --target=arm64-apple-macosx
    // plus CMAKE_OSX_ARCHITECTURES=arm64 by default, which conflicts with the WASI
    // toolchain. target("wasm32-wasip2") suppresses the OSX flags and names the
    // target the toolchain file compiles for, the one the Rust guests build
    // for; no_default_flags stops the C/CXX/ASM_FLAGS injection so the
    // toolchain file's flags win.
    let dst = cmake::Config::new(&manifest_dir)
        .target("wasm32-wasip2")
        .host("wasm32-wasip2")
        .no_default_flags(true)
        .define("CMAKE_TOOLCHAIN_FILE", &toolchain_file)
        .define("ARORA_MODULE_CLI", &arora_module_cli)
        .define("ARORA_BUFFERS_LIB", &arora_buffers_lib)
        .define("ARORA_UTIL_LIB", &arora_util_lib)
        .define("ARORA_CPP_SOURCE_DIR", &arora_cpp_source)
        .define("ARORA_BUFFERS_INCLUDE_DIR", &arora_buffers_include)
        .define("ARORA_UTIL_INCLUDE_DIR", &arora_util_include)
        .define("ARORA_MODULE_DIR", &module_dir)
        // Its module directory, which builds the guest first.
        .build_target("test-cpp-module-directory")
        .very_verbose(false)
        .build();

    // Locate produced wasm. cmake::Config builds in <out>/build by default.
    let wasm = dst.join("build").join("test-cpp");
    if !wasm.exists() {
        return Err(anyhow!("expected wasm at {} but not found", wasm.display()));
    }
    println!("cargo:wasm={}", wasm.display());
    let module_wasm = module_dir.join("test-cpp.wasm");
    if !module_wasm.is_file() || !module_dir.join("header.json").is_file() {
        return Err(anyhow!(
            "expected the module directory {} with header.json and test-cpp.wasm",
            module_dir.display()
        ));
    }
    println!("cargo:module-dir={}", module_dir.display());
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
