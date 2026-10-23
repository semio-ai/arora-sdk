//! Engine-level tests for the component-model executor: loads the
//! test-rust-component guest (a wasm32-wasip2 component implementing the
//! `arora:module` WIT world) and dispatches through it, including a
//! host callback via `host.dispatch-indirect` and the WASI 0.2 interfaces the
//! executor links.

use std::rc::Rc;

use arora_engine::call::{CallBridge, Callable, CallableId};
use arora_engine::engine::EngineBuilder;
use arora_engine::executor::component::ComponentExecutor;
use arora_engine::load::load_module_from_parts;
use arora_types::module::low::Header;
use arora_types::value::Value;
use uuid::Uuid;

/// Function ids implemented by modules/test-rust-component/src/lib.rs.
const ECHO_FN: &str = "dd6a4f61-9f88-4376-9d50-1e92e95a73aa";
const CALL_INDIRECT_FN: &str = "1bd1cf09-e94c-4c14-b653-32bd96157c41";
const WASI_FN: &str = "0455052e-bb89-41ec-a963-2bbe82ea583b";
const TRAP_FN: &str = "42681638-6fe3-4004-92fb-bb048dc3bbab";
const ERROR_BUFFER_FN: &str = "2041d791-4440-4dc9-b54d-1004e314cf99";
const CALL_FN: &str = "473a50b2-f275-4767-989c-69aa2c2def5b";

/// What the guest's `wasi` function observes on a host that runs it without
/// arguments or an environment. The browser executor's test
/// (`crates/arora-engine/tests/browser_component.rs`) expects the same.
const WASI_REPORT: &str = "\
monotonic-advances=true
monotonic-resolution-positive=true
wall-after-2020=true
wall-nanoseconds-below-a-second=true
wall-resolution-positive=true
random-bytes=40
insecure-random-bytes=3
environment=0
arguments=0
initial-cwd=None
stdout-writable=true
stdout-flushes=true
stdout-writes-zeroes=true
standard-streams-written=true
poll-ready=[0, 1]
stdout-ready=true
past-instant-waited=true";

const MODULE_ID: &str = "0cd3f0c6-95d4-4ab1-90fa-7fbcca25c2c6";

fn header() -> Header {
    let exports: String = [
        (ECHO_FN, "echo"),
        (CALL_INDIRECT_FN, "call-indirect"),
        (WASI_FN, "wasi"),
        (TRAP_FN, "trap"),
        (ERROR_BUFFER_FN, "error-buffer"),
        (CALL_FN, "call"),
    ]
    .iter()
    .map(|(id, name)| {
        format!(
            "  - type: function\n    id: {id}\n    name: {name}\n    parameters: []\n    \
             ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}\n"
        )
    })
    .collect();
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
{exports}"#
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
    fn call(&self, _caller: &mut dyn CallBridge) -> Result<Value, arora_engine::call::CallError> {
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

#[test]
fn component_reaches_the_wasi_interfaces() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let report = engine
        .dispatch(&MODULE_ID.parse().unwrap(), &WASI_FN.parse().unwrap(), &[])
        .unwrap();
    assert_eq!(std::str::from_utf8(&report).unwrap(), WASI_REPORT);
}

/// Calls the guest's `echo` through the engine, from inside a call into the
/// same guest: the echoed argument, a structure under the function's id,
/// reads back as the return value.
struct EchoThroughTheEngine;

impl Callable for EchoThroughTheEngine {
    fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, arora_engine::call::CallError> {
        let echo: Uuid = ECHO_FN.parse().unwrap();
        let echoed = caller.arora_call(arora_types::call::Call {
            module_id: Some(MODULE_ID.parse().unwrap()),
            id: echo,
            args: vec![arora_types::value::StructureField {
                id: echo,
                value: Box::new(Value::U64(42)),
            }],
        })?;
        Ok(echoed.ret)
    }
}

/// A guest that calls back into the engine can be called again before that
/// call returns — a behavior tree's node running a child of the same module
/// does.
#[test]
fn component_is_reentered_through_the_host() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let callable: Rc<dyn Callable> = Rc::new(EchoThroughTheEngine);
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

/// A guest that returns an error buffer, as a core module writes its errors,
/// has it read as its error, as a core module's is.
#[test]
fn component_error_buffer_is_a_guest_error() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let error = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &ERROR_BUFFER_FN.parse().unwrap(),
            b"no such thing",
        )
        .unwrap_err();
    assert!(
        matches!(&error, arora_engine::module::DispatchError::Guest { message } if message == "no such thing"),
        "{error:?}"
    );
}

/// After a trap a component runs no call, saying why, until it is reloaded.
#[test]
fn component_that_traps_refuses_calls_until_it_is_reloaded() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let compiled = arora_engine::compiled::CompiledModule::new(header(), &guest_bytes()).unwrap();
    engine.load_compiled_module(&compiled).unwrap();
    let (module, echo) = (MODULE_ID.parse().unwrap(), ECHO_FN.parse().unwrap());

    let trap = engine
        .dispatch(&module, &TRAP_FN.parse().unwrap(), &[])
        .unwrap_err();
    assert!(format!("{trap}").contains("dispatch trapped"), "{trap}");
    let refused = engine.dispatch(&module, &echo, b"after").unwrap_err();
    assert!(
        format!("{refused}").contains("trapped in an earlier call"),
        "{refused}"
    );

    engine.reload_module(&compiled).unwrap();
    assert_eq!(
        &*engine.dispatch(&module, &echo, b"after").unwrap(),
        b"after"
    );
}

/// A guest calls a function of a module — here its own — through
/// `host.dispatch`.
#[test]
fn component_calls_through_host_dispatch() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let module: Uuid = MODULE_ID.parse().unwrap();
    let echo: Uuid = ECHO_FN.parse().unwrap();
    let mut arg = module.as_bytes().to_vec();
    arg.extend_from_slice(echo.as_bytes());
    arg.extend_from_slice(b"through the host");
    let result = engine
        .dispatch(&module, &CALL_FN.parse().unwrap(), &arg)
        .unwrap();
    assert_eq!(&*result, b"through the host");
}

/// One artifact serving two modules, loaded under one's header, serves that
/// module's functions only.
#[test]
fn component_serves_only_the_functions_its_header_declares() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let header = test_rust_wasm::test_rust_wasm::header(arora_types::module::low::Executor {
        name: "wasm-component".to_string(),
        min_version: None,
        max_version: None,
    });
    let module = header.id;
    let artifact = std::fs::read(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm")).unwrap();
    load_module_from_parts(&mut engine, header, artifact.into()).unwrap();

    // compass's `bearing`, which the same artifact serves under compass's id.
    let bearing: Uuid = "c4396a6b-600e-480e-adcd-7dd091a41a99".parse().unwrap();
    assert!(matches!(
        engine.dispatch(&module, &bearing, &[]),
        Err(arora_engine::module::DispatchError::FunctionNotFound { id }) if id == bearing
    ));
}
