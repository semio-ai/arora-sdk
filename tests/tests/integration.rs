//! Mirror of the CMake-era integration tests. Each invokes arora-cli
//! against a module artifact published under target/<profile>/modules/
//! (or the module's own target dir for cargo-component cases).

use std::path::PathBuf;
use std::process::Command;

const ARORA_CLI: &str = env!("ARORA_CLI_BIN");

fn workspace_root() -> PathBuf {
  let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  dir.pop();
  dir
}

fn behavior_tree_records() -> PathBuf {
  workspace_root()
    .join("crates")
    .join("arora-behavior-tree-types-yaml")
    .join("records")
}

fn run(args: &[&str]) {
  let output = Command::new(ARORA_CLI)
    .args(args)
    .output()
    .expect("spawning arora-cli");
  if !output.status.success() {
    eprintln!("--- stdout ---");
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("--- stderr ---");
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    panic!("arora-cli {args:?} failed with status {}", output.status);
  }
}

#[test]
fn call_polly_from_engine() {
  let polly_root = workspace_root().join("modules").join("polly");
  let module_yaml = polly_root
    .join("src")
    .join("arora_generated")
    .join("module.yaml");
  let module_exe = PathBuf::from(env!("CARGO_CDYLIB_FILE_POLLY_polly"));
  run(&[
    "--include",
    behavior_tree_records().to_str().unwrap(),
    "--header",
    module_yaml.to_str().unwrap(),
    "--exe",
    module_exe.to_str().unwrap(),
    "--call",
    "id: e5a41333-4848-411f-878c-f1d662ebb4a0",
  ]);
}

#[test]
fn call_test_rust_wasm_from_engine() {
  let module_root = workspace_root().join("modules").join("test-rust-wasm");
  let module_yaml = module_root
    .join("src")
    .join("arora_generated")
    .join("module.yaml");
  // Use the artifact dependency path from build script
  let wasm = PathBuf::from(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));
  run(&[
    "--include",
    behavior_tree_records().to_str().unwrap(),
    "--header",
    module_yaml.to_str().unwrap(),
    "--exe",
    wasm.to_str().unwrap(),
    "--call",
    "id: 00cd31a8-2cf4-48e6-a957-69a55de90424",
  ]);
}

#[test]
fn call_test_cpp_2_from_engine_with_struct() {
  let workspace = workspace_root();
  let test_cpp_2_root = workspace.join("modules").join("test-cpp-2");
  let profile = if cfg!(debug_assertions) {
    "debug"
  } else {
    "release"
  };
  let modules_dir = workspace.join("target").join(profile).join("modules");
  let test_cpp_2_module_yaml = modules_dir.join("test-cpp-2").join("module.yaml");
  let test_cpp_module_yaml = modules_dir.join("test-cpp").join("module.yaml");
  let test_cpp_2_records = test_cpp_2_root.join("records");
  let test_cpp_records_published = modules_dir.join("test-cpp").join("records");

  run(&[
    "--include",
    behavior_tree_records().to_str().unwrap(),
    "--include",
    test_cpp_2_records.to_str().unwrap(),
    "--include",
    test_cpp_records_published.to_str().unwrap(),
    "--header",
    test_cpp_2_module_yaml.to_str().unwrap(),
    "--exe",
    modules_dir.join("test-cpp-2.wasm").to_str().unwrap(),
    "--header",
    test_cpp_module_yaml.to_str().unwrap(),
    "--exe",
    modules_dir.join("test-cpp.wasm").to_str().unwrap(),
    "--call",
    concat!(
      "id: 07f5740c-ba4a-45af-8ec5-bedde5737e99\n",
      "args:\n",
      "- id: b41899c3-66dc-40d4-ab61-d1ccf5231c88\n",
      "  value:\n",
      "    enum:\n",
      "      id: 325a5767-e344-4532-860e-0749bcf2e428\n",
      "      variant_id: 766e9e9a-446d-4e46-83e6-14b7ca101169\n",
      "      value: unit\n",
      "- id: 63086e48-804f-403a-8862-3358ddedc08d\n",
      "  value:\n",
      "    struct:\n",
      "      id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c\n",
      "      fields:\n",
      "      - id: 7d94a956-e50d-4cc4-9714-f62e1f9b134e\n",
      "        value:\n",
      "          enums:\n",
      "            id: 325a5767-e344-4532-860e-0749bcf2e428\n",
      "            elements:\n",
      "              - variant_id: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2\n",
      "                value: unit\n",
      "      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563\n",
      "        value:\n",
      "          i32: 113\n",
    ),
  ]);
}
