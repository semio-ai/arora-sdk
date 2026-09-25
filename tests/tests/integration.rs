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

/// Run arora-cli and return what it printed: each call's result, as YAML.
fn run(args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn call_test_rust_wasm_from_engine() {
    // The module declares itself in Rust: the header the engine loads it with
    // is written from that declaration, which is what an export step does.
    let module_yaml = std::env::temp_dir().join("arora-test-rust-wasm-header.yaml");
    let header = test_rust_wasm::test_rust_wasm::header(arora_types::module::low::Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    });
    std::fs::write(
        &module_yaml,
        serde_yaml::to_string(&header).expect("the declared header serializes"),
    )
    .expect("writing the header");
    // Use the artifact dependency path from build script
    let wasm = PathBuf::from(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));
    run(&[
        "--header",
        module_yaml.to_str().unwrap(),
        "--exe",
        wasm.to_str().unwrap(),
        "--call",
        "id: 00cd31a8-2cf4-48e6-a957-69a55de90424",
    ]);
}

/// A C++ module's optional parameter and return, through the generated
/// bindings: a present argument comes back incremented, an absent one or an
/// explicit `None` comes back as `None`.
#[test]
fn call_test_cpp_with_optionals() {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let modules_dir = workspace_root()
        .join("target")
        .join(profile)
        .join("modules");
    let header = modules_dir.join("test-cpp").join("module.yaml");
    let exe = modules_dir.join("test-cpp.wasm");
    let call = |args: &str| {
        run(&[
            "--header",
            header.to_str().unwrap(),
            "--exe",
            exe.to_str().unwrap(),
            "--call",
            &format!("id: a24fd5ab-baf8-4f44-af41-053a54b3fa81\n{args}"),
        ])
    };
    let present = call(concat!(
        "args:\n",
        "- id: b3ec8dd2-2df1-43ae-bf2d-0a567c998243\n",
        "  value:\n",
        "    option:\n",
        "      u32: 41\n",
    ));
    assert!(present.contains("u32: 42"), "{present}");
    let explicit_none = call(concat!(
        "args:\n",
        "- id: b3ec8dd2-2df1-43ae-bf2d-0a567c998243\n",
        "  value:\n",
        "    option: null\n",
    ));
    assert!(explicit_none.contains("option: null"), "{explicit_none}");
    let absent = call("args: []\n");
    assert!(absent.contains("option: null"), "{absent}");
}

/// The arguments arrive whatever their order (here, not sorted by id), and a
/// structure argument decodes: the function returns 1 only when both did.
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

    let result = run(&[
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
            "    bool: true\n",
            "- id: 63086e48-804f-403a-8862-3358ddedc08d\n",
            "  value:\n",
            "    struct:\n",
            "      id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c\n",
            "      fields:\n",
            "      - id: 7d94a956-e50d-4cc4-9714-f62e1f9b134e\n",
            "        value:\n",
            "          bool: true\n",
            "      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563\n",
            "        value:\n",
            "          i32: 113\n",
        ),
    ]);
    assert!(result.contains("i32: 1\n"), "{result}");
}
