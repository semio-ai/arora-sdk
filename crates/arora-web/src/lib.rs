//! Browser-facing wrapper around the Arora engine.
//!
//! Provides a [`Engine`] type that JavaScript can construct, load
//! modules into (header JSON + executable bytes), and call functions
//! on. All module hosting is done via the browser's native
//! `WebAssembly` runtime — see `arora::executor::browser`.
//!
//! This crate only carries non-trivial content when built for
//! `wasm32-*` targets. On the host it is an empty shim so it can sit
//! in the workspace and participate in `cargo build --workspace`
//! without pulling wasm-only deps into a host link.

#![cfg(target_arch = "wasm32")]

use std::collections::HashMap;

use arora::{
  call::{Call, CallBridge},
  engine::EngineBuilder,
  executor::browser::BrowserExecutor,
  load::load_module_from_parts,
  schema::module::low::Header,
};
use uuid::Uuid;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn _start() {
  console_error_panic_hook::set_once();
}

/// JS-callable handle to a configured Arora engine.
#[wasm_bindgen]
pub struct Engine {
  inner: std::pin::Pin<Box<arora::engine::Engine>>,
  function_module: HashMap<Uuid, Uuid>,
}

#[wasm_bindgen]
impl Engine {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Engine {
    let inner = EngineBuilder::new().add_executor(BrowserExecutor::new()).build();
    Engine {
      inner,
      function_module: HashMap::new(),
    }
  }

  /// Load a module given its header (as JSON) and executable bytes.
  /// Returns the module's UUID as a string.
  #[wasm_bindgen(js_name = loadModule)]
  pub fn load_module(&mut self, header_json: &str, executable: &[u8]) -> Result<String, JsValue> {
    let header: Header =
      serde_json::from_str(header_json).map_err(|e| JsValue::from_str(&format!("invalid header json: {e}")))?;
    let loaded = load_module_from_parts(
      &mut *self.inner,
      header,
      executable.to_vec().into_boxed_slice(),
    )
    .map_err(|e| JsValue::from_str(&format!("load_module failed: {e}")))?;
    for fn_id in &loaded.function_ids {
      self.function_module.insert(*fn_id, loaded.id);
    }
    Ok(loaded.id.to_string())
  }

  /// Call a function. `call_json` is a JSON document matching
  /// `arora::call::Call`. Returns the result as a JSON string.
  #[wasm_bindgen]
  pub fn call(&mut self, call_json: &str) -> Result<String, JsValue> {
    let call: Call = serde_json::from_str(call_json)
      .map_err(|e| JsValue::from_str(&format!("invalid call json: {e}")))?;
    let module_id = if let Some(m) = call.module_id {
      m
    } else {
      *self
        .function_module
        .get(&call.id)
        .ok_or_else(|| JsValue::from_str("no module known for function"))?
    };
    let result = self
      .inner
      .arora_call(&module_id, call)
      .map_err(|e| JsValue::from_str(&format!("call failed: {e}")))?;
    serde_json::to_string(&result).map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))
  }
}

impl Default for Engine {
  fn default() -> Self {
    Self::new()
  }
}
