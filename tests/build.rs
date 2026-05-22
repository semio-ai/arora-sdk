use std::env;
use std::path::PathBuf;

// We deliberately do NOT bindep arora-cli or the host-targeted modules
// (polly, test-cpp, test-cpp-2). Bindeps on workspace members with cdylib
// crate-type produce duplicate compilation units (cargo issue #6313 / output
// filename collision warnings). Instead, rely on the workspace having been
// built — `cargo build --workspace` populates everything we need at known
// paths under <workspace>/target. The integration tests panic with a clear
// message if an artifact is missing.
fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_root = manifest_dir.parent().expect("tests/ has a parent").to_path_buf();
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let host_bin_ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let arora_cli = target_dir.join(&profile).join(format!("arora-cli{host_bin_ext}"));
    println!("cargo:rustc-env=ARORA_CLI_BIN={}", arora_cli.display());
    println!("cargo:rerun-if-changed=build.rs");
}
