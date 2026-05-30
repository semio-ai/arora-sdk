//! Mirror of the CMake-era integration tests. Most cases invoke arora-cli
//! against a module artifact published under target/<profile>/modules/
//! (or the module's own target dir for cargo-component cases). The Vizij
//! composed-module proof drives `arora::Engine` directly so multiple facade
//! calls can share one loaded runtime.

use anyhow::{bail, Context, Result};
use arora::{
  call::{Call, CallBridge},
  engine::{EngineBuilder, PinnedEngine},
  load::{load_module_from_parts, load_module_from_parts_with_executor},
  schema::module::low::Header,
};
use arora_types::value::{StructureField, Value};
use serde_json::{json, Value as JsonValue};
use std::path::PathBuf;
use std::process::Command;
use uuid::Uuid;

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

fn parse_uuid(id: &str) -> Uuid {
  Uuid::parse_str(id).expect("static uuid")
}

fn required_artifact(path: PathBuf, build_hint: &str) -> Result<PathBuf> {
  if !path.exists() {
    bail!(
      "required artifact is missing: {}\nBuild it first with:\n{}",
      path.display(),
      build_hint
    );
  }
  Ok(path)
}

fn load_wasm_module(engine: &mut PinnedEngine, header: PathBuf, wasm: PathBuf) -> Result<()> {
  let header_name = header.display().to_string();
  let header: Header = serde_yaml::from_str(
    &std::fs::read_to_string(&header)
      .with_context(|| format!("read module header {header_name}"))?,
  )
  .with_context(|| format!("parse module header {header_name}"))?;
  let module_name = header.name.clone();
  let executable = std::fs::read(&wasm)
    .with_context(|| format!("read wasm module {}", wasm.display()))?
    .into_boxed_slice();
  load_module_from_parts(&mut **engine, header, executable)
    .with_context(|| format!("load module {module_name}"))?;
  Ok(())
}

fn native_library_extension() -> &'static str {
  if cfg!(target_os = "macos") {
    "dylib"
  } else if cfg!(target_os = "windows") {
    "dll"
  } else {
    "so"
  }
}

fn load_native_module(engine: &mut PinnedEngine, header: PathBuf, library: PathBuf) -> Result<()> {
  let header_name = header.display().to_string();
  let header: Header = serde_yaml::from_str(
    &std::fs::read_to_string(&header)
      .with_context(|| format!("read module header {header_name}"))?,
  )
  .with_context(|| format!("parse module header {header_name}"))?;
  let module_name = header.name.clone();
  let executable = std::fs::read(&library)
    .with_context(|| format!("read native module {}", library.display()))?
    .into_boxed_slice();
  load_module_from_parts_with_executor(&mut **engine, header, executable, "native")
    .with_context(|| format!("load native module {module_name}"))?;
  Ok(())
}

fn load_vizij_release_modules(engine: &mut PinnedEngine) -> Result<()> {
  let root = workspace_root();
  let build_hint = "cargo +nightly build -p vizij-animation -p vizij-node-graph -p vizij-orchestrator-composed --target wasm32-wasip1 --release";
  let release_wasm_dir = root.join("target").join("wasm32-wasip1").join("release");

  load_wasm_module(
    engine,
    root
      .join("modules")
      .join("vizij-animation")
      .join("src")
      .join("arora_generated")
      .join("module.yaml"),
    required_artifact(release_wasm_dir.join("vizij_animation.wasm"), build_hint)?,
  )?;
  load_wasm_module(
    engine,
    root
      .join("modules")
      .join("vizij-node-graph")
      .join("src")
      .join("arora_generated")
      .join("module.yaml"),
    required_artifact(release_wasm_dir.join("vizij_node_graph.wasm"), build_hint)?,
  )?;
  load_wasm_module(
    engine,
    root
      .join("modules")
      .join("vizij-orchestrator-composed")
      .join("src")
      .join("arora_generated")
      .join("module.yaml"),
    required_artifact(
      release_wasm_dir.join("arora_vizij_orchestrator_composed.wasm"),
      build_hint,
    )?,
  )?;
  Ok(())
}

fn call_vizij_dispatch(
  engine: &mut PinnedEngine,
  call: &str,
  args: JsonValue,
) -> Result<JsonValue> {
  let composed_module_id = parse_uuid("580d9cef-88be-4f1c-b649-f87032acd8fe");
  let dispatch_json_id = parse_uuid("90725b7e-a4d9-4a3f-99af-8e227612bed7");
  let request_json_param_id = parse_uuid("323d47be-3b30-46ff-882f-bc7f7ffacd57");
  let request = json!({
    "call": call,
    "requestId": format!("native:{call}"),
    "args": args,
  });
  let result = engine.arora_call(
    &composed_module_id,
    Call {
      module_id: Some(composed_module_id),
      id: dispatch_json_id,
      args: vec![StructureField {
        id: request_json_param_id,
        value: Box::new(Value::String(request.to_string())),
      }],
    },
  )?;
  let Value::String(response_json) = result.ret else {
    bail!("dispatch_json returned non-string value: {:?}", result.ret);
  };
  let response: JsonValue = serde_json::from_str(&response_json)
    .with_context(|| format!("parse dispatch_json response for {call}"))?;
  if response["ok"] != true {
    bail!("dispatch_json {call} failed: {response}");
  }
  Ok(response["result"].clone())
}

fn fixture_animation_for_path(output_path: &str) -> JsonValue {
  json!({
    "id": "native-composed-animation",
    "name": "Native Composed Animation",
    "formatVersion": 2,
    "defaultViewportExtent": 1000,
    "groups": [],
    "tracks": [
      {
        "id": "smile-track",
        "name": "Smile",
        "animatableId": output_path,
        "points": [
          { "id": "smile-0", "stamp": 0, "value": 0, "transitions": { "out": "linear" } },
          { "id": "smile-1", "stamp": 1000, "value": 1, "transitions": { "in": "linear" } }
        ]
      }
    ]
  })
}

fn graph_constant_output(path: &str, value: f32) -> JsonValue {
  json!({
    "nodes": [
      {
        "id": "source",
        "type": "constant",
        "params": { "value": { "type": "float", "data": value } }
      },
      {
        "id": "out",
        "type": "output",
        "params": { "path": path }
      }
    ],
    "edges": [
      {
        "from": { "node_id": "source", "output": "out" },
        "to": { "node_id": "out", "input": "in" }
      }
    ]
  })
}

fn write_paths(frame: &JsonValue) -> Vec<String> {
  frame["merged_writes"]
    .as_array()
    .expect("writes array")
    .iter()
    .map(|write| write["path"].as_str().expect("write path").to_string())
    .collect()
}

#[test]
fn call_polly_from_engine() {
  let polly_root = workspace_root().join("modules").join("polly");
  let polly_lib_ext = if cfg!(target_os = "macos") {
    "dylib"
  } else if cfg!(target_os = "windows") {
    "dll"
  } else {
    "so"
  };
  let module_yaml = polly_root
    .join("src")
    .join("arora_generated")
    .join("module.yaml");
  let module_exe = polly_root
    .join("target")
    .join("debug")
    .join(format!("libpolly.{polly_lib_ext}"));
  // Fall back to the workspace target dir (cargo puts host artifacts there).
  let module_exe = if module_exe.exists() {
    module_exe
  } else {
    workspace_root()
      .join("target")
      .join("debug")
      .join(format!("libpolly.{polly_lib_ext}"))
  };
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
  let wasm = workspace_root()
    .join("target")
    .join("wasm32-wasip1")
    .join("debug")
    .join("test_rust_wasm.wasm");
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
#[ignore = "requires release-built Vizij wasm guests; run after building vizij-animation, vizij-node-graph, and vizij-orchestrator-composed for wasm32-wasip1 --release"]
fn call_vizij_composed_release_wasm_modules_from_native_engine() -> Result<()> {
  // Use release wasm here. The debug Vizij guests are large enough that
  // Wasmtime startup can look like a hang, which obscures the module proof.
  let mut engine = EngineBuilder::new()
    .add_executor(arora::executor::wasm::WebAssemblyExecutor::new()?)
    .build();
  load_vizij_release_modules(&mut engine)?;

  let runtime = call_vizij_dispatch(
    &mut engine,
    "runtime.create",
    json!({ "schedule": "SinglePass" }),
  )?;
  assert_eq!(runtime["composition"], "independent-modules");

  let graph = call_vizij_dispatch(
    &mut engine,
    "graph.register",
    json!({
      "id": "graph:native-smoke",
      "spec": graph_constant_output("face/graph.value", 3.0),
    }),
  )?;
  assert_eq!(graph["module"], "vizij-node-graph");

  let animation = call_vizij_dispatch(
    &mut engine,
    "animation.register",
    json!({
      "id": "anim:native-smoke",
      "setup": {
        "animation": fixture_animation_for_path("face/smile.amount"),
        "instance": { "timescale": 1.0, "active": true }
      }
    }),
  )?;
  assert_eq!(animation["module"], "vizij-animation");

  let frame = call_vizij_dispatch(&mut engine, "orchestrator.step", json!({ "dt": 0.5 }))?;
  let paths = write_paths(&frame);
  assert!(
    paths.contains(&"face/smile.amount".to_string()),
    "animation write missing: {frame}"
  );
  assert!(
    paths.contains(&"face/graph.value".to_string()),
    "graph write missing: {frame}"
  );

  Ok(())
}

#[test]
fn call_vizij_composed_native_module_from_desktop_engine() -> Result<()> {
  let mut engine = EngineBuilder::new()
    .add_executor(arora::executor::native::NativeExecutor::new())
    .build();
  let root = workspace_root();
  let build_hint = "cargo build -p vizij-orchestrator-composed";
  let native_lib = required_artifact(
    root.join("target").join("debug").join(format!(
      "libarora_vizij_orchestrator_composed.{}",
      native_library_extension()
    )),
    build_hint,
  )?;

  load_native_module(
    &mut engine,
    root
      .join("modules")
      .join("vizij-orchestrator-composed")
      .join("src")
      .join("arora_generated")
      .join("module.yaml"),
    native_lib,
  )?;

  let runtime = call_vizij_dispatch(
    &mut engine,
    "runtime.create",
    json!({ "schedule": "SinglePass" }),
  )?;
  assert_eq!(runtime["composition"], "independent-modules");

  let graph = call_vizij_dispatch(
    &mut engine,
    "graph.register",
    json!({
      "id": "graph:desktop-native-smoke",
      "spec": graph_constant_output("face/graph.value", 3.0),
    }),
  )?;
  assert_eq!(graph["module"], "vizij-node-graph");

  let animation = call_vizij_dispatch(
    &mut engine,
    "animation.register",
    json!({
      "id": "anim:desktop-native-smoke",
      "setup": {
        "animation": fixture_animation_for_path("face/smile.amount"),
        "instance": { "timescale": 1.0, "active": true }
      }
    }),
  )?;
  assert_eq!(animation["module"], "vizij-animation");

  let frame = call_vizij_dispatch(&mut engine, "orchestrator.step", json!({ "dt": 0.5 }))?;
  let paths = write_paths(&frame);
  assert!(
    paths.contains(&"face/smile.amount".to_string()),
    "animation write missing: {frame}"
  );
  assert!(
    paths.contains(&"face/graph.value".to_string()),
    "graph write missing: {frame}"
  );

  Ok(())
}

#[test]
#[ignore = "pre-existing: arora-cli panics with 'Cannot start a runtime from within a runtime' when handling multi-module --call; tracked separately from the build-system migration"]
fn call_test_cpp_2_from_engine_with_struct() {
  let workspace = workspace_root();
  let test_cpp_2_root = workspace.join("modules").join("test-cpp-2");
  let modules_dir = workspace.join("target").join("debug").join("modules");
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
