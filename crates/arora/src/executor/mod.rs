#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(feature = "native-host")]
pub mod native;
#[cfg(feature = "wasmtime-host")]
pub mod wasm;

use crate::{engine::EngineRef, module::Module, schema::module::low::ModuleDefinition};
use derive_more::{Display, From};
use uuid::Uuid;

#[derive(Debug, Display, From, Clone)]
pub enum LoadModuleError {
  MalformedExecutable,
  Internal(String),
}

impl std::error::Error for LoadModuleError {}

#[derive(Debug, Display, From)]
pub enum UnloadModuleError {
  ModuleNotFound,
  Internal(String),
}
impl std::error::Error for UnloadModuleError {}

pub trait Executor {
  fn set_engine(&mut self, engine: EngineRef);

  fn name(&self) -> &'static str;
  fn load_module(
    &mut self,
    module_definition: ModuleDefinition,
  ) -> Result<Box<dyn Module>, LoadModuleError>;
  fn unload_module(&mut self, module_id: Uuid) -> Result<(), UnloadModuleError>;
}
