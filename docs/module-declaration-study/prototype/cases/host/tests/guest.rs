//! The declared module as a **wasm guest**: the shims `#[export]` emits under
//! `cfg(target_arch = "wasm32")` are what the real `WebAssemblyExecutor` looks
//! up and calls, with the header the declaration produces.
//!
//! Needs the artifact: `cargo build -p case-bt-nodes --target wasm32-wasip1`
//! (see `prototype/test.sh`).

use arora_engine::engine::EngineBuilder;
use arora_engine::executor::wasm::WebAssemblyExecutor;
use arora_types::call::{Call, CallBridge};
use arora_types::module::low::ModuleDefinition;
use arora_types::value::{StructureField, Value};
use case_bt_nodes::nodes::{self, ids};
use case_bt_nodes::Status;
use std::path::PathBuf;

fn guest_wasm() -> Vec<u8> {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../../target/wasm32-wasip1/debug/case_bt_nodes.wasm");
  std::fs::read(&path).unwrap_or_else(|e| {
    panic!(
      "{}: {e}\nbuild the guest first: cargo build -p case-bt-nodes --target wasm32-wasip1",
      path.display()
    )
  })
}

fn engine_with_guest() -> arora_engine::engine::PinnedEngine {
  let mut engine = EngineBuilder::new()
    .add_executor(WebAssemblyExecutor::new().expect("wasm executor"))
    .build();
  engine
    .load_module(ModuleDefinition {
      schema_version: 0,
      header: nodes::header(arora_types::module::low::Executor {
        name: "wasm".into(),
        min_version: None,
        max_version: None,
      }),
      executable: guest_wasm().into_boxed_slice(),
    })
    .expect("load the declared module as a guest");
  engine
}

fn field(id: uuid::Uuid, value: Value) -> StructureField {
  StructureField {
    id,
    value: Box::new(value),
  }
}

#[test]
fn the_guest_shim_dispatches_through_the_wasm_executor() {
  let mut engine = engine_with_guest();
  let result = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::status_identity::FUNCTION,
      args: vec![field(
        ids::status_identity::VALUE,
        Value::from(Status::Running),
      )],
    })
    .expect("status_identity through the guest");
  assert_eq!(Status::try_from(result.ret).unwrap(), Status::Running);
}

#[test]
fn a_mutable_parameter_comes_back_from_the_guest() {
  let mut engine = engine_with_guest();
  let result = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::set_str::FUNCTION,
      args: vec![
        field(ids::set_str::VARIABLE, Value::String("old".into())),
        field(ids::set_str::VALUE, Value::String("new".into())),
      ],
    })
    .expect("set_str through the guest");
  assert_eq!(Status::try_from(result.ret).unwrap(), Status::Success);
  assert_eq!(
    result.mutated,
    vec![field(ids::set_str::VARIABLE, Value::String("new".into()))]
  );
}

#[test]
fn a_guest_error_is_reported_as_a_call_error() {
  let mut engine = engine_with_guest();
  let error = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::set_str::FUNCTION,
      args: vec![],
    })
    .expect_err("set_str without arguments");
  assert!(
    error
      .to_string()
      .contains("missing parameter `variable` of `set_str`"),
    "{error}"
  );
}
