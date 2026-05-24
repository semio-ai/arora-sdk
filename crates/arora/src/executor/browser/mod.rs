//! Browser-hosted wasm executor. Stub for Phase 1 — actual instantiation
//! via `js_sys::WebAssembly` is implemented in Phase 4.

use uuid::Uuid;

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::{engine::EngineRef, module::Module};

pub struct BrowserExecutor {
  engine: Option<EngineRef>,
}

impl BrowserExecutor {
  pub fn new() -> Self {
    Self { engine: None }
  }
}

impl Default for BrowserExecutor {
  fn default() -> Self {
    Self::new()
  }
}

impl Executor for BrowserExecutor {
  fn set_engine(&mut self, engine: EngineRef) {
    self.engine = Some(engine);
  }

  fn name(&self) -> &'static str {
    "wasm"
  }

  fn load_module(
    &mut self,
    _module_definition: arora_types::module::low::ModuleDefinition,
  ) -> Result<Box<dyn Module>, LoadModuleError> {
    Err(LoadModuleError::Internal(
      "BrowserExecutor::load_module not yet implemented".into(),
    ))
  }

  fn unload_module(&mut self, _module_id: Uuid) -> Result<(), UnloadModuleError> {
    Err(UnloadModuleError::Internal(
      "BrowserExecutor::unload_module not yet implemented".into(),
    ))
  }
}
