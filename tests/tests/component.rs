//! Engine-level tests for the component-model executor: loads the
//! test-rust-component guest (a wasm32-wasip2 component implementing the
//! `arora:module` WIT world) and dispatches through it, including a
//! host callback via `host.dispatch-indirect`.

use std::rc::Rc;

use arora::call::{CallBridge, Callable, CallableId};
use arora::engine::EngineBuilder;
use arora::executor::component::ComponentExecutor;
use arora::load::load_module_from_parts;
use arora_types::module::low::Header;
use arora_types::value::Value;
use uuid::Uuid;

/// Function ids implemented by modules/test-rust-component/src/lib.rs.
const ECHO_FN: &str = "dd6a4f61-9f88-4376-9d50-1e92e95a73aa";
const CALL_INDIRECT_FN: &str = "1bd1cf09-e94c-4c14-b653-32bd96157c41";

const MODULE_ID: &str = "0cd3f0c6-95d4-4ab1-90fa-7fbcca25c2c6";

fn header() -> Header {
    let yaml = format!(
        r#"
id: {MODULE_ID}
name: test-rust-component
author: ""
description: ~
license: ""
version: {{ major: 0, minor: 0, patch: 0 }}
executor: {{ name: wasm-component, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
  - type: function
    id: {ECHO_FN}
    name: echo
    parameters: []
    ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}
  - type: function
    id: {CALL_INDIRECT_FN}
    name: call-indirect
    parameters: []
    ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}
"#
    );
    serde_yaml::from_str(&yaml).expect("parse test header")
}

fn guest_bytes() -> Box<[u8]> {
    std::fs::read(env!(
        "CARGO_CDYLIB_FILE_TEST_RUST_COMPONENT_test_rust_component"
    ))
    .expect("read test-rust-component wasm")
    .into_boxed_slice()
}

#[test]
fn component_echo_roundtrip() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();

    let loaded = load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    assert_eq!(loaded.id, MODULE_ID.parse::<Uuid>().unwrap());

    let payload = b"hello component".to_vec();
    let result = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &ECHO_FN.parse().unwrap(),
            &payload,
        )
        .unwrap();
    assert_eq!(&*result, payload.as_slice());
}

#[test]
fn component_bad_callable_arg_is_guest_error() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    // The guest rejects call-indirect args shorter than 8 bytes with a
    // typed guest error (no trap).
    let err = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &CALL_INDIRECT_FN.parse().unwrap(),
            b"abc",
        )
        .unwrap_err();
    let message = format!("{err}");
    assert!(
        message.contains("8-byte callable id"),
        "unexpected error: {message}"
    );
}

struct FortyTwo;

impl Callable for FortyTwo {
    fn call(&self, _caller: &mut dyn CallBridge) -> Result<Value, arora::call::CallError> {
        Ok(Value::U64(42))
    }
}

#[test]
fn component_calls_back_through_dispatch_indirect() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let callable: Rc<dyn Callable> = Rc::new(FortyTwo);
    let CallableId { id } = engine.arora_register_callable(callable);

    let result = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &CALL_INDIRECT_FN.parse().unwrap(),
            &id.to_le_bytes(),
        )
        .unwrap();

    let expected = arora_buffers::serde_uuid::serialize(&Value::U64(42));
    assert_eq!(&*result, &*expected);
}
