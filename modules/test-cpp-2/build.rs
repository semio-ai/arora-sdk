use std::env;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

fn main() -> Result<()> {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?);
    println!("cargo:rerun-if-changed=CMakeLists.txt");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=module.yaml");
    println!("cargo:rerun-if-changed=records");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");

    let sdk_root = wasi_sdk::locate_or_download().context("locating or downloading WASI SDK")?;
    let toolchain_file = wasi_sdk::cmake_toolchain_file(&sdk_root);

    let arora_module_cli_src = env_path("CARGO_BIN_FILE_ARORA_MODULE_CLI")?;
    let arora_module_cpp_src = env_path("CARGO_BIN_FILE_ARORA_MODULE_CPP")?;
    let arora_buffers_lib = env_path("CARGO_STATICLIB_FILE_ARORA_BUFFERS")?;
    let arora_util_lib = env_path("CARGO_STATICLIB_FILE_ARORA_UTIL")?;

    let out_dir = PathBuf::from(env::var("OUT_DIR").context("OUT_DIR not set")?);
    let tools_dir = out_dir.join("arora-tools");
    std::fs::create_dir_all(&tools_dir).ok();
    let arora_module_cli = tools_dir.join("arora-module-cli");
    let arora_module_cpp = tools_dir.join("arora-module-cpp");
    copy_executable(&arora_module_cli_src, &arora_module_cli)?;
    copy_executable(&arora_module_cpp_src, &arora_module_cpp)?;

    let workspace_root = workspace_root(&manifest_dir)?;
    let behavior_tree_include = workspace_root
        .join("crates")
        .join("arora-behavior-tree-types-yaml")
        .join("records");
    let arora_cpp_source = workspace_root.join("libs").join("cpp");
    let arora_include_dir = workspace_root.join("target").join("include");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let test_cpp_records = workspace_root
        .join("target")
        .join(&profile)
        .join("modules")
        .join("test-cpp")
        .join("records");
    let local_records = manifest_dir.join("records");

    let dst = cmake::Config::new(&manifest_dir)
        .target("wasm32-wasi")
        .host("wasm32-wasi")
        .no_default_flags(true)
        .define("CMAKE_TOOLCHAIN_FILE", &toolchain_file)
        .define("ARORA_MODULE_CLI", &arora_module_cli)
        .define("ARORA_BEHAVIOR_TREE_INCLUDE", &behavior_tree_include)
        .define("ARORA_BUFFERS_LIB", &arora_buffers_lib)
        .define("ARORA_UTIL_LIB", &arora_util_lib)
        .define("ARORA_CPP_SOURCE_DIR", &arora_cpp_source)
        .define("ARORA_INCLUDE_DIR", &arora_include_dir)
        .define("TEST_CPP_RECORDS", &test_cpp_records)
        .define("LOCAL_RECORDS", &local_records)
        .build_target("test-cpp-2")
        .very_verbose(false)
        .build();

    let wasm = dst.join("build").join("test-cpp-2");
    if !wasm.exists() {
        return Err(anyhow!("expected wasm at {} but not found", wasm.display()));
    }
    println!("cargo:wasm={}", wasm.display());

    let stable = workspace_root.join("target").join(&profile).join("modules");
    std::fs::create_dir_all(&stable).ok();
    let stable_wasm = stable.join("test-cpp-2.wasm");
    std::fs::copy(&wasm, &stable_wasm).with_context(|| {
        format!("copying {} to {}", wasm.display(), stable_wasm.display())
    })?;
    println!("cargo:wasm-stable={}", stable_wasm.display());
    Ok(())
}

fn env_path(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("{name} not set; bindeps may not be enabled (-Z bindeps)"))
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
