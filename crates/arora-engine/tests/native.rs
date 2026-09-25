//! Native-host integration tests: a guest built for the host as a shared
//! library, loaded through [`NativeExecutor`] and dispatched.

#![cfg(all(not(target_arch = "wasm32"), feature = "native-host"))]

use arora_engine::call::CallBridge;
use arora_engine::engine::{Engine, EngineBuilder};
use arora_engine::executor::native::NativeExecutor;
use arora_types::call::Call;
use arora_types::module::low::{Executor, ModuleDefinition};
use arora_types::value::{Structure, StructureField, Value};
use std::pin::Pin;
use uuid::Uuid;

// The test-rust-wasm guest, built for the host as a cdylib artifact dependency.
const DYLIB: &str = env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm");

// `text(length: u32) -> String`, as the guest's Rust declaration pins it.
const TEXT: Uuid = Uuid::from_u128(0x431fb111_d750_4470_b1c8_94b0ad8c6e6b);
const TEXT_LENGTH: Uuid = Uuid::from_u128(0x64351cbc_d84b_4df3_bd52_fd23edd943cf);

fn engine_with_guest() -> (Pin<Box<Engine>>, Uuid) {
    let header = test_rust_wasm::test_rust_wasm::header(Executor {
        name: "native".to_string(),
        min_version: None,
        max_version: None,
    });
    let module_id = header.id;
    let executable = std::fs::read(DYLIB)
        .expect("guest library")
        .into_boxed_slice();
    let mut engine = EngineBuilder::new()
        .add_executor(NativeExecutor::new())
        .build();
    engine
        .load_module(ModuleDefinition {
            schema_version: 0,
            header,
            executable,
        })
        .expect("guest loads");
    (engine, module_id)
}

/// The result buffer the executor hands back spans exactly the size its prefix
/// states. Lengths 0..=300 give the size's low byte every value, so a prefix
/// read in the wrong byte order cannot match by accident.
#[test]
fn result_buffer_spans_its_size_prefix() {
    let (mut engine, module_id) = engine_with_guest();
    for length in 0..=300u32 {
        let arg = arora_buffers::serde_uuid::serialize(&Value::Structure(Structure {
            id: TEXT,
            fields: vec![StructureField {
                id: TEXT_LENGTH,
                value: Box::new(Value::U32(length)),
            }],
        }));
        let result = engine.dispatch(&module_id, &TEXT, &arg).expect("dispatch");
        let prefix = u32::from_le_bytes(result[..4].try_into().unwrap());
        assert_eq!(result.len(), prefix as usize, "text({length})");
    }
}

/// A call through the engine's `CallBridge` reaches the native guest and
/// decodes its result.
#[test]
fn call_returns_the_guest_result() {
    let (mut engine, module_id) = engine_with_guest();
    let result = engine
        .arora_call(Call {
            module_id: Some(module_id),
            id: TEXT,
            args: vec![StructureField {
                id: TEXT_LENGTH,
                value: Box::new(Value::U32(200)),
            }],
        })
        .expect("call");
    assert_eq!(result.ret, Value::String("x".repeat(200)));
}
