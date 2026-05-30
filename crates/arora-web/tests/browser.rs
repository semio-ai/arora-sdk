//! Browser integration test. Loads the test-rust-wasm guest module
//! (built by the workspace's `arora-integration-tests` crate as a
//! bindep, into `target/wasm32-wasip1/debug/test_rust_wasm.wasm`)
//! through arora-web's JS-facing `Engine`, then calls its `ping`
//! function.

#![cfg(target_arch = "wasm32")]

use arora::schema::module::low::Header;
use arora_web::Engine;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const HEADER_YAML: &str = include_str!(env!("TEST_RUST_WASM_HEADER_YAML"));
const WASM_BYTES: &[u8] = include_bytes!(env!("TEST_RUST_WASM_BYTES"));
const VIZIJ_ORCHESTRATOR_HEADER_YAML: &str = include_str!(env!("VIZIJ_ORCHESTRATOR_HEADER_YAML"));
const VIZIJ_ORCHESTRATOR_WASM_BYTES: &[u8] = include_bytes!(env!("VIZIJ_ORCHESTRATOR_WASM_BYTES"));
const VIZIJ_ANIMATION_HEADER_YAML: &str = include_str!(env!("VIZIJ_ANIMATION_HEADER_YAML"));
const VIZIJ_ANIMATION_WASM_BYTES: &[u8] = include_bytes!(env!("VIZIJ_ANIMATION_WASM_BYTES"));
const VIZIJ_NODE_GRAPH_HEADER_YAML: &str = include_str!(env!("VIZIJ_NODE_GRAPH_HEADER_YAML"));
const VIZIJ_NODE_GRAPH_WASM_BYTES: &[u8] = include_bytes!(env!("VIZIJ_NODE_GRAPH_WASM_BYTES"));
const VIZIJ_ORCHESTRATOR_COMPOSED_HEADER_YAML: &str =
  include_str!(env!("VIZIJ_ORCHESTRATOR_COMPOSED_HEADER_YAML"));
const VIZIJ_ORCHESTRATOR_COMPOSED_WASM_BYTES: &[u8] =
  include_bytes!(env!("VIZIJ_ORCHESTRATOR_COMPOSED_WASM_BYTES"));

// `ping` from modules/test-rust-wasm/src/arora_generated/module.yaml.
const PING_FN_ID: &str = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6";
const VIZIJ_ORCHESTRATOR_DISPATCH_FN_ID: &str = "debf32e5-1650-48ac-af4a-da2da617aef7";
const VIZIJ_ORCHESTRATOR_REQUEST_PARAM_ID: &str = "71b4a759-ded6-42a3-b59d-9716472ac045";
const VIZIJ_ORCHESTRATOR_COMPOSED_DISPATCH_FN_ID: &str = "90725b7e-a4d9-4a3f-99af-8e227612bed7";
const VIZIJ_ORCHESTRATOR_COMPOSED_REQUEST_PARAM_ID: &str = "323d47be-3b30-46ff-882f-bc7f7ffacd57";

fn yaml_header_to_json(yaml: &str) -> String {
  let header: Header = serde_yaml::from_str(yaml).expect("parse header yaml");
  serde_json::to_string(&header).expect("re-serialize header to json")
}

fn load_guest(engine: &mut Engine, header_yaml: &str, wasm_bytes: &[u8]) -> String {
  engine
    .load_module(&yaml_header_to_json(header_yaml), wasm_bytes)
    .map_err(jsval_to_string)
    .expect("load guest module succeeded")
}

fn call_json_dispatch(
  engine: &mut Engine,
  function_id: &str,
  request_param_id: &str,
  request_json: &str,
) -> Result<serde_json::Value, String> {
  let call_json = serde_json::json!({
    "id": function_id,
    "args": [
      {
        "id": request_param_id,
        "value": { "str": request_json }
      }
    ]
  })
  .to_string();
  let result = engine.call(&call_json).map_err(jsval_to_string)?;
  let result: serde_json::Value = serde_json::from_str(&result).map_err(|e| e.to_string())?;
  let response_json = result["ret"]["str"]
    .as_str()
    .ok_or_else(|| "dispatch return was not a string".to_string())?;
  serde_json::from_str(response_json).map_err(|e| e.to_string())
}

#[wasm_bindgen_test]
fn load_and_ping_test_rust_wasm() {
  let header_json = yaml_header_to_json(HEADER_YAML);
  let mut engine = Engine::new();

  let module_id: String = engine
    .load_module(&header_json, WASM_BYTES)
    .map_err(jsval_to_string)
    .expect("loadModule succeeded");

  // Sanity: returned ID matches the header's id.
  let header: Header = serde_yaml::from_str(HEADER_YAML).unwrap();
  assert_eq!(module_id, header.id.to_string());

  let call_json = format!(r#"{{"id":"{PING_FN_ID}","args":[]}}"#);
  let result = engine
    .call(&call_json)
    .map_err(jsval_to_string)
    .expect("call(ping) succeeded");

  // Just assert we got some non-empty JSON back; full result shape is
  // an `arora_types::call::CallResult` (return value + mutated).
  assert!(!result.is_empty(), "result was empty");
}

#[wasm_bindgen_test]
fn load_and_call_vizij_orchestrator_wasm() {
  let header_json = yaml_header_to_json(VIZIJ_ORCHESTRATOR_HEADER_YAML);
  let mut engine = Engine::new();

  let module_id: String = engine
    .load_module(&header_json, VIZIJ_ORCHESTRATOR_WASM_BYTES)
    .map_err(jsval_to_string)
    .expect("loadModule(vizij-orchestrator) succeeded");

  let header: Header = serde_yaml::from_str(VIZIJ_ORCHESTRATOR_HEADER_YAML).unwrap();
  assert_eq!(module_id, header.id.to_string());

  let request_json =
    r#"{"call":"runtime.create","requestId":"browser-runtime","args":{"schedule":"SinglePass"}}"#;
  let call_json = serde_json::json!({
    "id": VIZIJ_ORCHESTRATOR_DISPATCH_FN_ID,
    "args": [
      {
        "id": VIZIJ_ORCHESTRATOR_REQUEST_PARAM_ID,
        "value": { "str": request_json }
      }
    ]
  })
  .to_string();
  let result = engine
    .call(&call_json)
    .map_err(jsval_to_string)
    .expect("call(vizij-orchestrator.dispatch_json) succeeded");
  let result: serde_json::Value = serde_json::from_str(&result).expect("call result json");
  let response_json = result["ret"]["str"].as_str().expect("string return");
  let response: serde_json::Value =
    serde_json::from_str(response_json).expect("vizij facade response json");
  assert_eq!(response["ok"], true, "{response}");
  assert_eq!(response["result"]["runtimeHandle"], "runtime:0");
  assert_eq!(response["result"]["schedule"], "SinglePass");
}

#[wasm_bindgen_test]
fn composed_orchestrator_requires_domain_modules_in_browser() {
  let mut engine = Engine::new();
  load_guest(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_HEADER_YAML,
    VIZIJ_ORCHESTRATOR_COMPOSED_WASM_BYTES,
  );

  let create_response = call_json_dispatch(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_DISPATCH_FN_ID,
    VIZIJ_ORCHESTRATOR_COMPOSED_REQUEST_PARAM_ID,
    r#"{"call":"runtime.create","requestId":"composed-missing-domains","args":{"schedule":"SinglePass"}}"#,
  )
  .expect("runtime.create does not need domain modules");
  assert_eq!(create_response["ok"], true, "{create_response}");

  let request_json = serde_json::json!({
    "call": "graph.register",
    "requestId": "missing-node-graph",
    "args": {
      "id": "graph:missing",
      "spec": {
        "nodes": [
          {
            "id": "source",
            "type": "constant",
            "params": { "value": { "type": "float", "data": 1.0 } }
          },
          {
            "id": "out",
            "type": "output",
            "params": { "path": "face/missing.graph" }
          }
        ],
        "edges": [
          {
            "from": { "node_id": "source", "output": "out" },
            "to": { "node_id": "out", "input": "in" }
          }
        ]
      }
    }
  })
  .to_string();
  let err = call_json_dispatch(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_DISPATCH_FN_ID,
    VIZIJ_ORCHESTRATOR_COMPOSED_REQUEST_PARAM_ID,
    &request_json,
  )
  .expect_err("composed module should fail when its imported domain modules are not loaded");
  assert!(err.contains("call failed"), "{err}");
}

#[wasm_bindgen_test]
fn load_and_call_composed_vizij_orchestrator_wasm() {
  let mut engine = Engine::new();
  load_guest(
    &mut engine,
    VIZIJ_ANIMATION_HEADER_YAML,
    VIZIJ_ANIMATION_WASM_BYTES,
  );
  load_guest(
    &mut engine,
    VIZIJ_NODE_GRAPH_HEADER_YAML,
    VIZIJ_NODE_GRAPH_WASM_BYTES,
  );
  load_guest(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_HEADER_YAML,
    VIZIJ_ORCHESTRATOR_COMPOSED_WASM_BYTES,
  );

  let create_response = call_json_dispatch(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_DISPATCH_FN_ID,
    VIZIJ_ORCHESTRATOR_COMPOSED_REQUEST_PARAM_ID,
    r#"{"call":"runtime.create","requestId":"composed-runtime","args":{"schedule":"SinglePass"}}"#,
  )
  .expect("runtime.create succeeded");
  assert_eq!(create_response["ok"], true, "{create_response}");
  assert_eq!(
    create_response["result"]["composition"],
    "independent-modules"
  );

  let graph_request = serde_json::json!({
    "call": "graph.register",
    "requestId": "composed-graph",
    "args": {
      "id": "graph:browser",
      "spec": {
        "nodes": [
          {
            "id": "source",
            "type": "constant",
            "params": { "value": { "type": "float", "data": 3.0 } }
          },
          {
            "id": "out",
            "type": "output",
            "params": { "path": "face/browser.graph" }
          }
        ],
        "edges": [
          {
            "from": { "node_id": "source", "output": "out" },
            "to": { "node_id": "out", "input": "in" }
          }
        ]
      }
    }
  })
  .to_string();
  let graph_response = call_json_dispatch(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_DISPATCH_FN_ID,
    VIZIJ_ORCHESTRATOR_COMPOSED_REQUEST_PARAM_ID,
    &graph_request,
  )
  .expect("graph.register succeeded");
  assert_eq!(graph_response["ok"], true, "{graph_response}");

  let animation_request = serde_json::json!({
    "call": "animation.register",
    "requestId": "composed-animation",
    "args": {
      "id": "anim:browser",
      "setup": {
        "animation": {
          "id": "browser-animation",
          "name": "Browser Animation",
          "formatVersion": 2,
          "defaultViewportExtent": 1000,
          "groups": [],
          "tracks": [
            {
              "id": "smile-track",
              "name": "Smile",
              "animatableId": "face/browser.smile",
              "points": [
                { "id": "smile-0", "stamp": 0, "value": 0, "transitions": { "out": "linear" } },
                { "id": "smile-1", "stamp": 1000, "value": 1, "transitions": { "in": "linear" } }
              ]
            }
          ]
        },
        "instance": { "weight": 1.0 }
      }
    }
  })
  .to_string();
  let animation_response = call_json_dispatch(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_DISPATCH_FN_ID,
    VIZIJ_ORCHESTRATOR_COMPOSED_REQUEST_PARAM_ID,
    &animation_request,
  )
  .expect("animation.register succeeded");
  assert_eq!(animation_response["ok"], true, "{animation_response}");

  let step_response = call_json_dispatch(
    &mut engine,
    VIZIJ_ORCHESTRATOR_COMPOSED_DISPATCH_FN_ID,
    VIZIJ_ORCHESTRATOR_COMPOSED_REQUEST_PARAM_ID,
    r#"{"call":"orchestrator.step","requestId":"composed-step","args":{"dt":0.5}}"#,
  )
  .expect("orchestrator.step succeeded");
  assert_eq!(step_response["ok"], true, "{step_response}");
  let writes = step_response["result"]["merged_writes"]
    .as_array()
    .expect("writes array");
  assert!(
    writes
      .iter()
      .any(|write| write["path"] == "face/browser.graph"),
    "graph write missing: {writes:?}"
  );
  assert!(
    writes
      .iter()
      .any(|write| write["path"] == "face/browser.smile"),
    "animation write missing: {writes:?}"
  );
}

fn jsval_to_string(v: JsValue) -> String {
  v.as_string().unwrap_or_else(|| format!("{v:?}"))
}
