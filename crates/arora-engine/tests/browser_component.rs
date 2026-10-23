//! Browser-host integration tests for components: the test-rust-component
//! guest (a wasm32-wasip2 component implementing the `arora:module` WIT world)
//! loaded through
//! [`BrowserComponentExecutor`](arora_engine::executor::browser::BrowserComponentExecutor), dispatched, calling back into
//! the engine and reaching the WASI 0.2 interfaces — the same calls
//! `tests/tests/component.rs` makes through the native component executor.
//!
//! Like `browser.rs`, the file only exists on `wasm32` and runs under
//! `wasm-bindgen-test` in a browser.

#![cfg(target_arch = "wasm32")]

use std::pin::Pin;
use std::rc::Rc;

use arora_engine::call::{CallBridge, CallError, Callable, CallableId};
use arora_engine::compiled::CompiledModule;
use arora_engine::engine::{Engine, EngineBuilder};
use arora_engine::executor::browser::{BrowserExecutor, SharedLoaderRc};
use arora_engine::load::load_module_from_parts;
use arora_types::module::low::Header;
use arora_types::value::Value;
use uuid::Uuid;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

// The test-rust-component guest, built for wasm32-wasip2 as a cdylib artifact
// dependency and wired in by Cargo.
const COMPONENT: &[u8] = include_bytes!(env!(
    "CARGO_CDYLIB_FILE_TEST_RUST_COMPONENT_test_rust_component"
));

/// Function ids implemented by modules/test-rust-component/src/lib.rs.
const ECHO_FN: &str = "dd6a4f61-9f88-4376-9d50-1e92e95a73aa";
const CALL_INDIRECT_FN: &str = "1bd1cf09-e94c-4c14-b653-32bd96157c41";
const WASI_FN: &str = "0455052e-bb89-41ec-a963-2bbe82ea583b";
const TRAP_FN: &str = "42681638-6fe3-4004-92fb-bb048dc3bbab";
const ERROR_BUFFER_FN: &str = "2041d791-4440-4dc9-b54d-1004e314cf99";
const CALL_FN: &str = "473a50b2-f275-4767-989c-69aa2c2def5b";

const MODULE_ID: &str = "0cd3f0c6-95d4-4ab1-90fa-7fbcca25c2c6";

/// What the guest's `wasi` function observes on a host that runs it without
/// arguments or an environment, as `tests/tests/component.rs` expects of the
/// native component executor.
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
preopened-directories=0
stdout-writable=true
stdout-flushes=true
stdout-writes-zeroes=true
standard-streams-written=true
poll-ready=[0, 1]
stdout-ready=true
past-instant-waited=true";

/// The guest's header, under `executor`, declaring `functions`.
fn header_with(executor: &str, functions: &[(&str, &str)]) -> Header {
    let exports: String = functions
        .iter()
        .map(|(id, name)| {
            format!(
                "  - type: function\n    id: {id}\n    name: {name}\n    parameters: []\n    \
                 ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}\n"
            )
        })
        .collect();
    serde_yaml::from_str(&format!(
        r#"
id: {MODULE_ID}
name: test-rust-component
author: ""
description: ~
license: ""
version: {{ major: 0, minor: 0, patch: 0 }}
executor: {{ name: {executor}, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
{exports}"#
    ))
    .expect("parse the test header")
}

fn header() -> Header {
    header_with(
        "wasm-component",
        &[
            (ECHO_FN, "echo"),
            (CALL_INDIRECT_FN, "call-indirect"),
            (WASI_FN, "wasi"),
            (TRAP_FN, "trap"),
            (ERROR_BUFFER_FN, "error-buffer"),
            (CALL_FN, "call"),
        ],
    )
}

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("valid uuid")
}

/// An engine with both browser executors, sharing one loader.
fn engine() -> (Pin<Box<Engine>>, SharedLoaderRc) {
    let executor = BrowserExecutor::new();
    let loader = executor.shared();
    let components = executor.component_executor();
    let engine = EngineBuilder::new()
        .add_executor(executor)
        .add_executor(components)
        .build();
    (engine, loader)
}

/// An engine with the guest loaded from its bytes.
fn loaded() -> Pin<Box<Engine>> {
    let (mut engine, _) = engine();
    load_module_from_parts(&mut engine, header(), COMPONENT.into()).expect("load the component");
    engine
}

fn dispatch(engine: &mut Engine, function: &str, arg: &[u8]) -> Result<Box<[u8]>, String> {
    engine
        .dispatch(&id(MODULE_ID), &id(function), arg)
        .map_err(|e| format!("{e}"))
}

#[wasm_bindgen_test]
fn a_component_dispatches_in_browser() {
    console_error_panic_hook::set_once();
    let mut engine = loaded();
    let payload = b"hello component";
    let result = dispatch(&mut engine, ECHO_FN, payload).expect("dispatch echo");
    assert_eq!(&*result, payload);
}

/// A guest's `err` result reaches the caller as the guest's error, not a trap.
#[wasm_bindgen_test]
fn a_guest_error_reaches_the_caller_in_browser() {
    console_error_panic_hook::set_once();
    let mut engine = loaded();
    let message = dispatch(&mut engine, CALL_INDIRECT_FN, b"abc").unwrap_err();
    assert!(
        message.contains("8-byte callable id"),
        "unexpected error: {message}"
    );
}

struct FortyTwo;

impl Callable for FortyTwo {
    fn call(&self, _caller: &mut dyn CallBridge) -> Result<Value, CallError> {
        Ok(Value::U64(42))
    }
}

#[wasm_bindgen_test]
fn a_component_calls_back_through_dispatch_indirect_in_browser() {
    console_error_panic_hook::set_once();
    let mut engine = loaded();
    let callable: Rc<dyn Callable> = Rc::new(FortyTwo);
    let CallableId { id: callable } = engine.arora_register_callable(callable);

    let result =
        dispatch(&mut engine, CALL_INDIRECT_FN, &callable.to_le_bytes()).expect("call back");
    let expected = arora_buffers::serde_uuid::serialize(&Value::U64(42));
    assert_eq!(&*result, &*expected);
}

/// The browser lays out every WASI argument and result as the native executor
/// does: the guest observes the same.
#[wasm_bindgen_test]
fn a_component_reaches_the_wasi_interfaces_in_browser() {
    console_error_panic_hook::set_once();
    let mut engine = loaded();
    let report = dispatch(&mut engine, WASI_FN, &[]).expect("dispatch wasi");
    assert_eq!(std::str::from_utf8(&report).expect("utf-8"), WASI_REPORT);
}

/// The asynchronous loader path (`WebAssembly.compile` +
/// `WebAssembly.instantiate`) stages a component as it stages a core module.
#[wasm_bindgen_test]
async fn a_component_prepares_asynchronously_then_dispatches() {
    console_error_panic_hook::set_once();
    let (mut engine, loader) = engine();
    loader
        .prepare(id(MODULE_ID), COMPONENT.to_vec())
        .await
        .expect("async prepare");
    load_module_from_parts(&mut engine, header(), Box::new([]))
        .expect("load the asynchronously-prepared component");
    let result = dispatch(&mut engine, ECHO_FN, b"prepared").expect("dispatch echo");
    assert_eq!(&*result, b"prepared");
}

/// A component staged by the loader is not loaded as a core module.
#[wasm_bindgen_test]
async fn a_prepared_component_is_not_loaded_as_a_core_module() {
    console_error_panic_hook::set_once();
    let (mut engine, loader) = engine();
    loader
        .prepare(id(MODULE_ID), COMPONENT.to_vec())
        .await
        .expect("async prepare");
    let error = load_module_from_parts(
        &mut engine,
        header_with("wasm", &[(ECHO_FN, "echo")]),
        Box::new([]),
    )
    .expect_err("a component staged for the core executor fails to load");
    assert!(
        format!("{error}").contains("prepared as a component"),
        "unexpected error: {error}"
    );
}

/// One compiled component loads into several engines, each with an instance
/// of its own, and reloads in place.
#[wasm_bindgen_test]
fn a_compiled_component_loads_into_several_engines_and_reloads() {
    console_error_panic_hook::set_once();
    let compiled = CompiledModule::new(header(), COMPONENT).expect("compile the component");
    let load = || {
        let (mut engine, _) = engine();
        engine
            .load_compiled_module(&compiled)
            .expect("load the compiled component");
        engine
    };
    let first = load();
    let mut second = load();
    drop(first);

    for _ in 0..100 {
        second.reload_module(&compiled).expect("reload");
        let result = dispatch(&mut second, ECHO_FN, b"again").expect("dispatch echo");
        assert_eq!(&*result, b"again");
    }
    second.unload_module(&id(MODULE_ID)).expect("unload");
    assert!(!second.contains_module(&id(MODULE_ID)));
}

#[wasm_bindgen_test]
fn an_undeclared_function_is_not_found_in_a_component() {
    console_error_panic_hook::set_once();
    let (mut engine, _) = engine();
    load_module_from_parts(
        &mut engine,
        header_with("wasm-component", &[(ECHO_FN, "echo")]),
        COMPONENT.into(),
    )
    .expect("load");
    // The guest serves call-indirect, but this header does not declare it.
    let message = dispatch(&mut engine, CALL_INDIRECT_FN, &[]).unwrap_err();
    assert!(
        message.contains(CALL_INDIRECT_FN),
        "unexpected error: {message}"
    );
}

/// A function the header declares and the component's `functions` does not
/// return fails the load, as a core module missing an export does.
#[wasm_bindgen_test]
fn a_component_missing_a_declared_function_fails_to_load() {
    console_error_panic_hook::set_once();
    let (mut engine, _) = engine();
    let missing = "00000000-0000-0000-0000-000000000009";
    let error = load_module_from_parts(
        &mut engine,
        header_with("wasm-component", &[(ECHO_FN, "echo"), (missing, "missing")]),
        COMPONENT.into(),
    )
    .expect_err("the load fails");
    assert!(
        format!("{error}").contains(&format!("failed to find function export {missing}")),
        "unexpected error: {error}"
    );
}

/// A component of the `arora:module` world in WebAssembly text: the core
/// module `guest` (named `$guest`), its instance `$main`, and its `functions`
/// and `dispatch` lifted as `wit-component` lifts them, followed by `more` —
/// imports, modules and their instances.
fn world_component(guest: &str, more: &str) -> String {
    format!(
        r#"(component
            (type $types (instance
                (type (record (field "hi" u64) (field "lo" u64)))
                (export "id" (type (eq 0)))))
            (import "arora:module/types@0.2.0" (instance $types (type $types)))
            (alias export $types "id" (type $id))
            {guest}
            (core instance $main (instantiate $guest))
            (alias core export $main "memory" (core memory $memory))
            (alias core export $main "cabi_realloc" (core func $realloc))
            (type $ids (list $id))
            (type $functions (func (result $ids)))
            (func $functions (type $functions)
                (canon lift (core func $main "functions") (memory $memory)))
            (export "functions" (func $functions))
            (type $bytes (list u8))
            (type $result (result $bytes (error string)))
            (type $dispatch (func (param "method" $id) (param "arg" $bytes) (result $result)))
            (func $dispatch (type $dispatch)
                (canon lift (core func $main "dispatch") (memory $memory) (realloc $realloc)))
            (export "dispatch" (func $dispatch))
            {more})"#
    )
}

/// A guest module exporting what the world's lifts call, and importing
/// `imports`.
fn guest(imports: &str) -> String {
    format!(
        r#"(core module $guest
            {imports}
            (memory (export "memory") 1)
            (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 8)
            (func (export "functions") (result i32) i32.const 0)
            (func (export "dispatch") (param i64 i64 i32 i32) (result i32) i32.const 0))"#
    )
}

/// The error loading `component` (WebAssembly text) under the guest's header.
fn load_error(component: &str) -> String {
    let (mut engine, _) = engine();
    let bytes = wat::parse_str(component).expect("assemble the component");
    let error =
        load_module_from_parts(&mut engine, header(), bytes.into()).expect_err("the load fails");
    format!("{error}")
}

/// A component importing an interface the hosts do not provide is refused,
/// as the native executor's linker refuses it.
#[wasm_bindgen_test]
fn a_component_importing_what_the_browser_does_not_provide_is_refused() {
    console_error_panic_hook::set_once();
    let error = load_error(&world_component(
        &guest(""),
        r#"(import "wasi:sockets/tcp@0.2.0" (instance))"#,
    ));
    assert!(
        error.contains("imports wasi:sockets/tcp@0.2.0, which is not an interface"),
        "unexpected error: {error}"
    );
}

/// A guest module importing a function the browser does not implement fails
/// to load, naming it, whatever the component wires to it.
#[wasm_bindgen_test]
fn a_guest_module_importing_what_the_browser_does_not_provide_fails_to_load() {
    console_error_panic_hook::set_once();
    let error = load_error(&world_component(
        &guest(
            r#"(import "wasi:sockets/network@0.2.0" "[resource-drop]network" (func (param i32)))"#,
        ),
        "",
    ));
    assert!(
        error.contains("'[resource-drop]network' from 'wasi:sockets/network@0.2.0'"),
        "unexpected error: {error}"
    );
}

/// The filesystem has no directory, so no descriptor exists: a call on one
/// traps, as natively a call on an unknown handle does.
#[wasm_bindgen_test]
fn a_call_on_a_descriptor_traps_in_browser() {
    console_error_panic_hook::set_once();
    let function = "00000000-0000-0000-0000-000000000003";
    let (mut engine, _) = engine();
    let guest = r#"(core module $guest
        (import "wasi:filesystem/types@0.2.0" "[method]descriptor.stat" (func $stat (param i32 i32)))
        (memory (export "memory") 1)
        (data (i32.const 1024) "\00\00\00\00\00\00\00\00\03\00\00\00\00\00\00\00")
        (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096)
        (func (export "functions") (result i32)
            (i32.store (i32.const 2048) (i32.const 1024))
            (i32.store (i32.const 2052) (i32.const 1))
            (i32.const 2048))
        (func (export "dispatch") (param i64 i64 i32 i32) (result i32)
            (call $stat (i32.const 0) (i32.const 3072))
            (i32.const 2048)))"#;
    let bytes = wat::parse_str(world_component(guest, "")).expect("assemble the component");
    load_module_from_parts(
        &mut engine,
        header_with("wasm-component", &[(function, "f")]),
        bytes.into(),
    )
    .expect("load");
    let error = dispatch(&mut engine, function, &[]).unwrap_err();
    assert!(error.contains("no descriptor exists"), "{error}");
}

/// A component that does not export the world's `functions` is refused,
/// as the native executor refuses it.
#[wasm_bindgen_test]
fn a_component_without_the_world_s_exports_is_refused() {
    console_error_panic_hook::set_once();
    let component =
        world_component(&guest(""), "").replace(r#"(export "functions" (func $functions))"#, "");
    let error = load_error(&component);
    assert!(
        error.contains("where the world exports functions and dispatch"),
        "unexpected error: {error}"
    );
}

#[wasm_bindgen_test]
fn a_component_of_two_guest_modules_is_refused() {
    console_error_panic_hook::set_once();
    let error = load_error(
        r#"(component
            (core module (func (export "a")))
            (core module (func (export "b"))))"#,
    );
    assert!(
        error.contains("2 guest module(s)"),
        "unexpected error: {error}"
    );
}

#[wasm_bindgen_test]
fn a_component_nesting_a_component_is_refused() {
    console_error_panic_hook::set_once();
    let error = load_error("(component (component))");
    assert!(
        error.contains("nests a component"),
        "unexpected error: {error}"
    );
}

/// A reactor guest: its `_initialize` adds one to a count its one function
/// returns. Its component holds `wit-component`'s initializer
/// ([`INITIALIZER`]), which calls `_initialize`.
const REACTOR: &str = r#"(core module $guest
    (memory (export "memory") 1)
    (global $count (mut i32) (i32.const 0))
    (global $heap (mut i32) (i32.const 4096))
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
        (local $at i32)
        (local.set $at (global.get $heap))
        (global.set $heap (i32.add (global.get $heap) (local.get 3)))
        (local.get $at))
    (func (export "_initialize")
        (global.set $count (i32.add (global.get $count) (i32.const 1))))
    ;; The id of the one function: { hi: 0, lo: 3 }.
    (data (i32.const 1024) "\00\00\00\00\00\00\00\00\03\00\00\00\00\00\00\00")
    (func (export "functions") (result i32)
        (i32.store (i32.const 2048) (i32.const 1024))
        (i32.store (i32.const 2052) (i32.const 1))
        (i32.const 2048))
    (func (export "dispatch") (param i64 i64 i32 i32) (result i32)
        (i32.store8 (i32.const 3072) (global.get $count))
        (i32.store8 (i32.const 2048) (i32.const 0))
        (i32.store (i32.const 2052) (i32.const 3072))
        (i32.store (i32.const 2056) (i32.const 1))
        (i32.const 2048)))"#;

/// `wit-component`'s initializer — a module whose start function is the
/// import it is given — instantiated as `wit-component` instantiates it,
/// given the guest instance's `_initialize`.
const INITIALIZER: &str = r#"(core module $initializer
    (import "" "" (func $initialize))
    (start $initialize))
(alias core export $main "_initialize" (core func $initialize))
(core instance $initialize-args (export "" (func $initialize)))
(core instance (instantiate $initializer (with "" (instance $initialize-args))))"#;

/// A component that defines an initializer and never instantiates it, so that
/// natively nothing runs the guest's `_initialize`, is refused rather than
/// initialized.
#[wasm_bindgen_test]
fn a_component_with_an_initializer_it_never_instantiates_is_refused() {
    console_error_panic_hook::set_once();
    let unused = r#"(core module (import "" "" (func $initialize)) (start $initialize))"#;
    let error = load_error(&world_component(REACTOR, unused));
    assert!(error.contains("initializer"), "unexpected error: {error}");
}

/// A guest writing `len` zeroes to standard output with the function `write`
/// imports, under the function id whose low half is `len`: it serves `lens`.
fn zeroes_writer(write: &str, [a, b]: [u64; 2]) -> String {
    format!(
        r#"(core module $guest
            (import "wasi:cli/stdout@0.2.0" "get-stdout" (func $stdout (result i32)))
            (import "wasi:io/streams@0.2.0" "[method]output-stream.{write}"
                (func $write (param i32 i64 i32)))
            (memory (export "memory") 1)
            (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096)
            (func (export "functions") (result i32)
                (i64.store (i32.const 1024) (i64.const 0))
                (i64.store (i32.const 1032) (i64.const {a}))
                (i64.store (i32.const 1040) (i64.const 0))
                (i64.store (i32.const 1048) (i64.const {b}))
                (i32.store (i32.const 2048) (i32.const 1024))
                (i32.store (i32.const 2052) (i32.const 2))
                (i32.const 2048))
            (func (export "dispatch") (param i64 i64 i32 i32) (result i32)
                (call $write (call $stdout) (local.get 1) (i32.const 3072))
                (i32.store8 (i32.const 2048) (i32.const 0))
                (i32.store (i32.const 2052) (i32.const 0))
                (i32.store (i32.const 2056) (i32.const 0))
                (i32.const 2048)))"#
    )
}

/// An engine with `component` loaded under a header declaring the functions
/// whose ids' low halves are `functions`.
fn loaded_with(component: &str, functions: [u64; 2]) -> (Pin<Box<Engine>>, Vec<String>) {
    let (mut engine, _) = engine();
    let ids: Vec<String> = functions
        .iter()
        .map(|lo| Uuid::from_u64_pair(0, *lo).to_string())
        .collect();
    let declared: Vec<(&str, &str)> = ids.iter().map(|id| (id.as_str(), "f")).collect();
    let bytes = wat::parse_str(component).expect("assemble the component");
    load_module_from_parts(
        &mut engine,
        header_with("wasm-component", &declared),
        bytes.into(),
    )
    .expect("load");
    (engine, ids)
}

/// Writing more zeroes than a stream writes at once traps, as natively, where
/// wasmtime writes none of them, rather than writing some.
#[wasm_bindgen_test]
fn writing_more_zeroes_than_a_stream_takes_at_once_traps_in_browser() {
    console_error_panic_hook::set_once();
    let lens = [1 << 16, 100_000];
    let component = world_component(&zeroes_writer("write-zeroes", lens), "");
    let (mut engine, ids) = loaded_with(&component, lens);
    dispatch(&mut engine, &ids[0], &[]).expect("as many as check-write permits");
    let error = dispatch(&mut engine, &ids[1], &[]).unwrap_err();
    assert!(error.contains("more than the 65536"), "{error}");

    let lens = [4096, 4097];
    let component = world_component(&zeroes_writer("blocking-write-zeroes-and-flush", lens), "");
    let (mut engine, ids) = loaded_with(&component, lens);
    dispatch(&mut engine, &ids[0], &[]).expect("as many as a blocking write takes");
    let error = dispatch(&mut engine, &ids[1], &[]).unwrap_err();
    assert!(error.contains("more than the 4096"), "{error}");
}

/// A reactor component's `_initialize` runs once, when it loads, before its
/// first call — not again before each call.
#[wasm_bindgen_test]
fn a_reactor_component_is_initialized_once_when_it_loads_in_browser() {
    console_error_panic_hook::set_once();
    let count = "00000000-0000-0000-0000-000000000003";
    let (mut engine, _) = engine();
    let bytes =
        wat::parse_str(world_component(REACTOR, INITIALIZER)).expect("assemble the component");
    load_module_from_parts(
        &mut engine,
        header_with("wasm-component", &[(count, "count")]),
        bytes.into(),
    )
    .expect("load");
    for _ in 0..2 {
        assert_eq!(&*dispatch(&mut engine, count, &[]).expect("dispatch"), &[1]);
    }
}

/// Calls the guest's `echo` through the engine, from inside a call into the
/// same guest: the echoed argument, a structure under the function's id,
/// reads back as the return value.
struct EchoThroughTheEngine;

impl Callable for EchoThroughTheEngine {
    fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError> {
        let echoed = caller.arora_call(arora_types::call::Call {
            module_id: Some(id(MODULE_ID)),
            id: id(ECHO_FN),
            args: vec![arora_types::value::StructureField {
                id: id(ECHO_FN),
                value: Box::new(Value::U64(42)),
            }],
        })?;
        Ok(echoed.ret)
    }
}

/// A guest that calls back into the engine can be called again before that
/// call returns, as the native component executor allows.
#[wasm_bindgen_test]
fn a_component_is_reentered_through_the_host_in_browser() {
    console_error_panic_hook::set_once();
    let mut engine = loaded();
    let callable: Rc<dyn Callable> = Rc::new(EchoThroughTheEngine);
    let CallableId { id: callable } = engine.arora_register_callable(callable);

    let result =
        dispatch(&mut engine, CALL_INDIRECT_FN, &callable.to_le_bytes()).expect("re-enter");
    let expected = arora_buffers::serde_uuid::serialize(&Value::U64(42));
    assert_eq!(&*result, &*expected);
}

/// A guest that returns an error buffer, as a core module writes its errors,
/// has it read as its error, as natively.
#[wasm_bindgen_test]
fn a_component_s_error_buffer_is_its_error_in_browser() {
    console_error_panic_hook::set_once();
    let mut engine = loaded();
    let error = engine
        .dispatch(&id(MODULE_ID), &id(ERROR_BUFFER_FN), b"no such thing")
        .unwrap_err();
    assert!(
        matches!(&error, arora_engine::module::DispatchError::Guest { message } if message == "no such thing"),
        "{error:?}"
    );
}

/// After a trap a component runs no call, saying why, until it is reloaded,
/// as natively.
#[wasm_bindgen_test]
fn a_component_that_traps_refuses_calls_until_it_is_reloaded_in_browser() {
    console_error_panic_hook::set_once();
    let compiled = CompiledModule::new(header(), COMPONENT).expect("compile the component");
    let (mut engine, _) = engine();
    engine.load_compiled_module(&compiled).expect("load");

    let trap = dispatch(&mut engine, TRAP_FN, &[]).unwrap_err();
    assert!(trap.contains("dispatch trapped"), "{trap}");
    let refused = dispatch(&mut engine, ECHO_FN, b"after").unwrap_err();
    assert!(refused.contains("trapped in an earlier call"), "{refused}");

    engine.reload_module(&compiled).expect("reload");
    assert_eq!(
        &*dispatch(&mut engine, ECHO_FN, b"after").expect("echo"),
        b"after"
    );
}

/// A guest calls a function of a module — here its own — through
/// `host.dispatch`, whose lowered form the browser implements.
#[wasm_bindgen_test]
fn a_component_calls_through_host_dispatch_in_browser() {
    console_error_panic_hook::set_once();
    let mut engine = loaded();
    let mut arg = id(MODULE_ID).as_bytes().to_vec();
    arg.extend_from_slice(id(ECHO_FN).as_bytes());
    arg.extend_from_slice(b"through the host");
    let result = dispatch(&mut engine, CALL_FN, &arg).expect("call through the host");
    assert_eq!(&*result, b"through the host");
}

/// A guest whose `err` is not UTF-8, which traps, as wasmtime's lift does.
const NOT_UTF_8: &str = r#"(core module $guest
    (memory (export "memory") 1)
    (data (i32.const 1024) "\00\00\00\00\00\00\00\00\03\00\00\00\00\00\00\00")
    (data (i32.const 3072) "\ff\fe")
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096)
    (func (export "functions") (result i32)
        (i32.store (i32.const 2048) (i32.const 1024))
        (i32.store (i32.const 2052) (i32.const 1))
        (i32.const 2048))
    (func (export "dispatch") (param i64 i64 i32 i32) (result i32)
        (i32.store8 (i32.const 2048) (i32.const 1))
        (i32.store (i32.const 2052) (i32.const 3072))
        (i32.store (i32.const 2056) (i32.const 2))
        (i32.const 2048)))"#;

#[wasm_bindgen_test]
fn a_component_s_error_that_is_not_utf_8_traps_in_browser() {
    console_error_panic_hook::set_once();
    let function = "00000000-0000-0000-0000-000000000003";
    let (mut engine, _) = engine();
    let bytes = wat::parse_str(world_component(NOT_UTF_8, "")).expect("assemble the component");
    load_module_from_parts(
        &mut engine,
        header_with("wasm-component", &[(function, "f")]),
        bytes.into(),
    )
    .expect("load");
    let error = dispatch(&mut engine, function, &[]).unwrap_err();
    assert!(error.contains("not UTF-8"), "{error}");
}
