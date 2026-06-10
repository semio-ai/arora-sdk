//! Browser integration test. Loads the test-rust-wasm guest module
//! (built by the workspace's `arora-integration-tests` crate as a
//! bindep, into `target/wasm32-wasip1/debug/test_rust_wasm.wasm`)
//! through arora-web's JS-facing `Engine`, then calls its `ping`
//! function.

#![cfg(target_arch = "wasm32")]

use arora_types::module::low::Header;
use arora_web::Engine;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const HEADER_YAML: &str = include_str!(env!("TEST_RUST_WASM_HEADER_YAML"));
// test-rust-wasm is a cdylib artifact dependency; cargo exposes its built wasm
// path as CARGO_CDYLIB_FILE_<DEP>_<lib>.
const WASM_BYTES: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

// `ping` from modules/test-rust-wasm/src/arora_generated/module.yaml.
const PING_FN_ID: &str = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6";

fn yaml_header_to_json(yaml: &str) -> String {
    let header: Header = serde_yaml::from_str(yaml).expect("parse header yaml");
    serde_json::to_string(&header).expect("re-serialize header to json")
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

/// Same flow through the prepared-module path used for executables larger
/// than Chrome's 8 MB main-thread compile/instantiate limit.
#[wasm_bindgen_test]
async fn prepare_load_and_ping_test_rust_wasm() {
    let header_json = yaml_header_to_json(HEADER_YAML);
    let mut engine = Engine::new();

    wasm_bindgen_futures::JsFuture::from(engine.prepare_module(&header_json, WASM_BYTES.to_vec()))
        .await
        .expect("prepareModule succeeded");

    let module_id: String = engine
        .load_prepared_module(&header_json)
        .map_err(jsval_to_string)
        .expect("loadPreparedModule succeeded");

    let header: Header = serde_yaml::from_str(HEADER_YAML).unwrap();
    assert_eq!(module_id, header.id.to_string());

    let call_json = format!(r#"{{"id":"{PING_FN_ID}","args":[]}}"#);
    let result = engine
        .call(&call_json)
        .map_err(jsval_to_string)
        .expect("call(ping) succeeded");
    assert!(!result.is_empty(), "result was empty");
}

fn jsval_to_string(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}
