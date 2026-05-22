use std::env;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

fn main() -> Result<()> {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?);
    println!("cargo:rerun-if-changed=CMakeLists.txt");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=module.yaml");
    println!("cargo:rerun-if-changed=mac-homebrew-i686.toolchain.cmake");
    println!("cargo:rerun-if-env-changed=ENABLE_NAO");

    if env::var("ENABLE_NAO").unwrap_or_else(|_| "0".to_string()) != "1" {
        println!(
            "cargo:warning=arora-nao: ENABLE_NAO != 1, skipping cross-build. Set ENABLE_NAO=1 to build the NAO module."
        );
        return Ok(());
    }

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
    let qi_stub_include = workspace_root.join("libs").join("qi-stub").join("include");
    let toolchain_file = manifest_dir.join("mac-homebrew-i686.toolchain.cmake");

    // We override cmake-rs's HOST target derivation: it defaults to the
    // build script's TARGET (aarch64-apple-darwin) and injects OSX-specific
    // flags, which collide with the i686-unknown-linux-musl cross toolchain.
    let dst = cmake::Config::new(&manifest_dir)
        .target("i686-unknown-linux-musl")
        .host("i686-unknown-linux-musl")
        .no_default_flags(true)
        .define("CMAKE_TOOLCHAIN_FILE", &toolchain_file)
        .define("ARORA_MODULE_CLI", &arora_module_cli)
        .define("ARORA_BEHAVIOR_TREE_INCLUDE", &behavior_tree_include)
        .define("ARORA_CPP_SOURCE_DIR", &arora_cpp_source)
        .define("ARORA_INCLUDE_DIR", &arora_include_dir)
        .define("ARORA_BUFFERS_LIB", &arora_buffers_lib)
        .define("ARORA_UTIL_LIB", &arora_util_lib)
        .define("QI_STUB_INCLUDE", &qi_stub_include)
        .build_target("nao")
        .very_verbose(false)
        .build();

    let so = dst.join("build").join("libnao.so");
    if !so.exists() {
        return Err(anyhow!(
            "expected libnao.so at {} but not found",
            so.display()
        ));
    }
    println!("cargo:libnao={}", so.display());

    let stable = workspace_root
        .join("target")
        .join(env::var("PROFILE").unwrap_or_else(|_| "debug".to_string()))
        .join("modules");
    std::fs::create_dir_all(&stable).ok();
    let stable_so = stable.join("libnao.so");
    std::fs::copy(&so, &stable_so)
        .with_context(|| format!("copying {} to {}", so.display(), stable_so.display()))?;
    println!("cargo:libnao-stable={}", stable_so.display());
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
