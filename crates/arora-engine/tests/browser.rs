//! Browser-host integration tests for the engine's own web executor.
//!
//! These instantiate a guest module through the browser's native
//! `WebAssembly` runtime ([`arora_engine::executor::browser::BrowserExecutor`]),
//! dispatch its functions, and read the results back — so regressions in
//! the web host are caught here, at the engine level, instead of being
//! rediscovered by a downstream binding.
//!
//! Following the wasm-bindgen-test guidance, the file is a crate root,
//! uses `#[wasm_bindgen_test]`, and is pinned to the browser with
//! `wasm_bindgen_test_configure!(run_in_browser)`. It only exists on
//! `wasm32`; on the host it compiles to nothing, so `cargo test` skips it.
//!
//! Run it (locally or in CI) with:
//!   wasm-pack test --headless --firefox crates/arora --no-default-features

#![cfg(target_arch = "wasm32")]

use arora_engine::call::CallBridge;
use arora_engine::engine::EngineBuilder;
use arora_engine::executor::browser::BrowserExecutor;
use arora_engine::load::load_module_from_parts;
use arora_types::call::Call;
use arora_types::module::low::Header;
use arora_types::value::{StructureField, Value};
use uuid::Uuid;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

// The test-rust-wasm guest (built for wasm32-wasip1 as a cdylib artifact
// dependency) plus its low-level header, wired in by Cargo + build.rs.
const HEADER_YAML: &str = include_str!(env!("TEST_RUST_WASM_HEADER_YAML"));
const WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

// Function + parameter ids from modules/test-rust-wasm/src/arora_generated/module.yaml.
const SUCCEED: &str = "00cd31a8-2cf4-48e6-a957-69a55de90424"; // () -> bool
const ADD: &str = "e4b0a2f3-6c7d-4e8f-9a0b-1c2d3e4f5a6b"; // (f32, f32) -> f32
const ADD_A: &str = "a1b2c3d4-e5f6-4a8b-9c0d-e1f2a3b4c5d6";
const ADD_B: &str = "b2c3d4e5-f6a7-4b9c-8d1e-f2a3b4c5d6e7";

fn header() -> Header {
    serde_yaml::from_str(HEADER_YAML).expect("parse test-rust-wasm header yaml")
}

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("valid uuid")
}

/// Synchronous load (`WebAssembly.Instance`) + dispatch of a no-argument
/// function whose `bool` result travels back through a guest buffer.
#[wasm_bindgen_test]
fn loads_and_dispatches_in_browser() {
    console_error_panic_hook::set_once();
    let header = header();
    let module_id = header.id;
    let mut engine = EngineBuilder::new()
        .add_executor(BrowserExecutor::new())
        .build();

    let loaded = load_module_from_parts(&mut engine, header, WASM.to_vec().into_boxed_slice())
        .expect("load test-rust-wasm into the browser executor");
    assert_eq!(loaded.id, module_id);

    let result = engine
        .arora_call(
            &module_id,
            Call {
                module_id: Some(module_id),
                id: id(SUCCEED),
                args: vec![],
            },
        )
        .expect("dispatch succeed()");
    assert_eq!(result.ret, Value::Boolean(true));
}

/// Marshal arguments into the guest buffer ABI and read a numeric result.
#[wasm_bindgen_test]
fn marshals_arguments_in_browser() {
    console_error_panic_hook::set_once();
    let header = header();
    let module_id = header.id;
    let mut engine = EngineBuilder::new()
        .add_executor(BrowserExecutor::new())
        .build();
    load_module_from_parts(&mut engine, header, WASM.to_vec().into_boxed_slice()).expect("load");

    let result = engine
        .arora_call(
            &module_id,
            Call {
                module_id: Some(module_id),
                id: id(ADD),
                args: vec![
                    StructureField {
                        id: id(ADD_A),
                        value: Box::new(Value::F32(2.0)),
                    },
                    StructureField {
                        id: id(ADD_B),
                        value: Box::new(Value::F32(3.0)),
                    },
                ],
            },
        )
        .expect("dispatch add(2, 3)");
    assert_eq!(result.ret, Value::F32(5.0));
}

/// The asynchronous loader path (`WebAssembly.instantiate`) the executor
/// uses for guests above Chrome's 8 MB main-thread instantiation limit.
#[wasm_bindgen_test]
async fn prepares_asynchronously_then_dispatches() {
    console_error_panic_hook::set_once();
    let header = header();
    let module_id = header.id;
    let executor = BrowserExecutor::new();
    let loader = executor.shared();
    let mut engine = EngineBuilder::new().add_executor(executor).build();

    loader
        .prepare(module_id, WASM.to_vec())
        .await
        .expect("async prepare");
    let loaded = load_module_from_parts(&mut engine, header, Box::new([]))
        .expect("load the asynchronously-prepared module");
    assert_eq!(loaded.id, module_id);

    let result = engine
        .arora_call(
            &module_id,
            Call {
                module_id: Some(module_id),
                id: id(SUCCEED),
                args: vec![],
            },
        )
        .expect("dispatch succeed()");
    assert_eq!(result.ret, Value::Boolean(true));
}
