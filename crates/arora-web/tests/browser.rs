//! Browser integration tests. One loads the test-rust-wasm guest module
//! (built by the workspace's `arora-integration-tests` crate as a
//! bindep, into `target/wasm32-wasip1/debug/test_rust_wasm.wasm`)
//! through arora-web's JS-facing `Engine`, then calls its `ping`
//! function; the others drive a device's client operations through
//! `AroraRuntime`, stepping it by hand as a page does.

#![cfg(target_arch = "wasm32")]

use arora::{Arora, ModuleBuilder};
use arora_behavior::Status;
use arora_types::call::{CallError, CallResult};
use arora_types::module::low::Header;
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::record::{FrozenReference, Version};
use arora_types::value::Value;
use arora_web::{AroraWeb, Engine};
use uuid::Uuid;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

// test-rust-wasm is a cdylib artifact dependency; cargo exposes its built wasm
// path as CARGO_CDYLIB_FILE_<DEP>_<lib>.
const WASM_BYTES: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

// `ping`, as the guest's Rust declaration pins it.
const PING_FN_ID: &str = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6";

/// The guest's header, from its declaration — what an export step would write
/// as a `module.yaml`, here handed straight to the engine.
fn header() -> Header {
    test_rust_wasm::test_rust_wasm::header(arora_types::module::low::Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    })
}

fn header_json() -> String {
    serde_json::to_string(&header()).expect("the declared header serializes to json")
}

#[wasm_bindgen_test]
fn load_and_ping_test_rust_wasm() {
    let header_json = header_json();
    let mut engine = Engine::new();

    let module_id: String = engine
        .load_module(&header_json, WASM_BYTES)
        .map_err(jsval_to_string)
        .expect("loadModule succeeded");

    // Sanity: returned ID matches the header's id.
    assert_eq!(module_id, header().id.to_string());

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
    let header_json = header_json();
    let mut engine = Engine::new();

    wasm_bindgen_futures::JsFuture::from(engine.prepare_module(&header_json, WASM_BYTES.to_vec()))
        .await
        .expect("prepareModule succeeded");

    let module_id: String = engine
        .load_prepared_module(&header_json)
        .map_err(jsval_to_string)
        .expect("loadPreparedModule succeeded");

    assert_eq!(module_id, header().id.to_string());

    let call_json = format!(r#"{{"id":"{PING_FN_ID}","args":[]}}"#);
    let result = engine
        .call(&call_json)
        .map_err(jsval_to_string)
        .expect("call(ping) succeeded");
    assert!(!result.is_empty(), "result was empty");
}

// =============================================================================
// The device's client operations
// =============================================================================

const TOOLS: Uuid = Uuid::from_u128(0x7001);
const DOUBLE: Uuid = Uuid::from_u128(0x7002);
const DOUBLE_X: Uuid = Uuid::from_u128(0x7003);
const WAVE: Uuid = Uuid::from_u128(0x7004);
const TOOLS_STOP: Uuid = Uuid::from_u128(0x7005);
const PLAYER: Uuid = Uuid::from_u128(0x7101);
const PLAYER_STOP: Uuid = Uuid::from_u128(0x7102);

fn answer(ret: Value) -> Result<CallResult, CallError> {
    Ok(CallResult {
        ret,
        mutated: Vec::new(),
    })
}

/// A device whose `tools` module exports `double(x: f64) -> f64`, the
/// task-shaped `wave() -> Status` (running until halted) and `stop()`, and whose
/// `player` module exports a `stop()` of its own.
fn device() -> AroraWeb {
    let signature = |parameters: &[(Uuid, &str)], return_ty: FrozenTy| Function {
        parameters: parameters
            .iter()
            .map(|(id, name)| {
                (
                    *id,
                    Parameter {
                        name: name.to_string(),
                        ty: FrozenTy::from(PrimitiveKind::F64),
                        mutable: false,
                    },
                )
            })
            .collect(),
        parameter_ordering: parameters.iter().map(|(id, _)| *id).collect(),
        return_ty,
    };
    let status = FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: arora_behavior::STATUS_ENUMERATION_ID,
            version: Version::parse("1.0.0").expect("a valid version"),
        },
    });
    let unit = || FrozenTy::from(PrimitiveKind::Unit);
    let tools = ModuleBuilder::new(TOOLS)
        .described_function(
            DOUBLE,
            "double",
            signature(&[(DOUBLE_X, "x")], FrozenTy::from(PrimitiveKind::F64)),
            |call| match call.args.first().map(|field| field.value.as_ref()) {
                Some(Value::F64(x)) => answer(Value::F64(2.0 * x)),
                _ => Err(CallError::Generic {
                    message: "double takes an f64".to_string(),
                }),
            },
        )
        .described_function(WAVE, "wave", signature(&[], status), |_call| {
            answer(Status::Running.into())
        })
        .described_function(TOOLS_STOP, "stop", signature(&[], unit()), |_call| {
            answer(Value::Unit)
        })
        .build();
    let player = ModuleBuilder::new(PLAYER)
        .described_function(PLAYER_STOP, "stop", signature(&[], unit()), |_call| {
            answer(Value::String("player stopped".to_string()))
        })
        .build();
    AroraWeb::from(
        Arora::builder()
            .with_host_module(tools)
            .with_host_module(player)
            .build()
            .expect("build the device"),
    )
}

/// Let the promise tasks the last step woke run: an `invoke` reads the
/// method's signature on one step and queues its call from its promise task.
async fn yield_to_tasks() {
    for _ in 0..4 {
        JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL))
            .await
            .expect("a resolved promise");
    }
}

fn json(value: JsValue) -> serde_json::Value {
    serde_wasm_bindgen::from_value(value).expect("a JSON-shaped answer")
}

fn status_json(status: Status) -> serde_json::Value {
    serde_json::to_value(Value::from(status)).expect("a status serializes")
}

/// Each operation is queued before its method returns: a `step()` in the same
/// turn applies it, with no await in between.
#[wasm_bindgen_test]
async fn an_operation_is_applied_by_a_step_in_the_same_turn() {
    let web = device();
    web.set_value("face/mouth", r#"{"f64": 0.5}"#)
        .expect("write a key");

    let keys = web.list_keys(Some("face/".to_string()));
    let methods = web.describe_methods(Some("dou".to_string()));
    web.step(10.0).expect("step");

    let keys = json(JsFuture::from(keys).await.expect("listKeys resolves"));
    assert_eq!(keys[0]["path"], "face/mouth", "{keys}");
    assert!(keys[0]["__meta"].is_object(), "{keys}");
    assert_eq!(keys.as_array().map(Vec::len), Some(1), "{keys}");

    let methods = json(
        JsFuture::from(methods)
            .await
            .expect("describeMethods resolves"),
    );
    assert_eq!(methods[0]["path"], "double", "{methods}");
    assert_eq!(methods[0]["module"], TOOLS.to_string(), "{methods}");
    assert_eq!(methods[0]["params"][0]["name"], "x", "{methods}");
    assert_eq!(methods[0]["task"], false, "{methods}");
}

/// `invoke` of a plain method resolves to its return value; of a task-shaped
/// one, to the run's handle, whose status key reads Running once a step ticks
/// the run, and which `halt` ends.
#[wasm_bindgen_test]
async fn invoke_answers_a_value_or_starts_a_run() {
    let web = device();

    let doubled = web.invoke("double", r#"{"x": {"f64": 2.5}}"#, None);
    web.step(10.0).expect("step");
    yield_to_tasks().await;
    web.step(10.0).expect("step");
    let doubled = json(JsFuture::from(doubled).await.expect("double answers"));
    // A JS number carries no int/float distinction, so compare it as one.
    assert_eq!(doubled["f64"].as_f64(), Some(5.0), "{doubled}");

    let started = web.invoke("wave", "{}", None);
    web.step(10.0).expect("step");
    yield_to_tasks().await;
    web.step(10.0).expect("step");
    let handle = json(JsFuture::from(started).await.expect("wave starts"));
    // A plain object, keys as paths.
    let run = handle["run"]
        .as_str()
        .unwrap_or_else(|| panic!("the handle names its run: {handle}"))
        .to_string();
    assert!(Uuid::parse_str(&run).is_ok(), "{handle}");
    let status_key = handle["status"]
        .as_str()
        .unwrap_or_else(|| panic!("the handle names its status key: {handle}"))
        .to_string();
    for keys in ["feedback", "result", "update"] {
        assert!(handle[keys].is_array(), "{keys} is a path array: {handle}");
    }

    web.step(10.0).expect("step");
    let read = |web: &AroraWeb| {
        let paths = serde_wasm_bindgen::to_value(&vec![status_key.clone()]).expect("paths");
        json(web.read_values(paths).expect("read the status"))[&status_key].clone()
    };
    assert_eq!(read(&web), status_json(Status::Running));

    let halted = web.halt(&run);
    web.step(10.0).expect("step");
    JsFuture::from(halted).await.expect("the halt is applied");
    web.step(10.0).expect("step");
    assert_eq!(
        read(&web),
        status_json(Status::Failure),
        "a halted run ends Failure"
    );
}

/// A name two modules export rejects naming both, until the module is named.
#[wasm_bindgen_test]
async fn invoke_of_a_shared_name_needs_its_module() {
    let web = device();

    let ambiguous = web.invoke("stop", "{}", None);
    web.step(10.0).expect("step");
    let error = jsval_to_string(
        JsFuture::from(ambiguous)
            .await
            .expect_err("stop is ambiguous"),
    );
    assert!(error.contains(&TOOLS.to_string()), "{error}");
    assert!(error.contains(&PLAYER.to_string()), "{error}");

    let chosen = web.invoke("stop", "{}", Some(PLAYER.to_string()));
    web.step(10.0).expect("step");
    yield_to_tasks().await;
    web.step(10.0).expect("step");
    let stopped = json(
        JsFuture::from(chosen)
            .await
            .expect("the module disambiguates"),
    );
    assert_eq!(stopped, serde_json::json!({"str": "player stopped"}));
}

fn jsval_to_string(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}
