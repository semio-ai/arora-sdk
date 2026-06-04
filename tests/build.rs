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

    // Try to get arora-cli from bindeps first, fall back to target dir
    let arora_cli = env::var("CARGO_BIN_FILE_ARORA_CLI")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| target_dir.join(&profile).join(format!("arora-cli{host_bin_ext}")));

    println!("cargo:rustc-env=ARORA_CLI_BIN={}", arora_cli.display());

    // Forward artifact dependency paths for WASM modules
    forward_env_var("CARGO_CDYLIB_FILE_BEHAVIOR_TREE_NODES_behavior_tree_nodes");
    forward_env_var("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm");
    forward_env_var("CARGO_STATICLIB_FILE_ARORA_BUFFERS");
    forward_env_var("CARGO_STATICLIB_FILE_ARORA_UTIL");

    println!("cargo:rerun-if-changed=build.rs");
}

fn forward_env_var(name: &str) {
    if let Ok(val) = std::env::var(name) {
        println!("cargo::rustc-env={}={}", name, val);
    }
}
