pub mod wasm;

use std::{cell::RefCell, rc::Rc};

use crate::{
  actor::{Actor, Addr, Request},
  module::{Module},
  schema::module::low::ModuleDefinition, engine::{Engine, EngineRef},
};
use derive_more::{Display, Error, From};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Display, Error, From, Clone)]
pub enum LoadModuleError {
  MalformedExecutable,
  Internal,
}

#[derive(Debug, Display, Error, From)]
pub enum UnloadModuleError {
  ModuleNotFound,
  Internal,
}

pub trait Executor {
  fn set_engine(&mut self, engine: EngineRef);

  fn name(&self) -> &'static str;
  fn load_module(&mut self, module_definition: ModuleDefinition) -> Result<Box<dyn Module>, LoadModuleError>;
  fn unload_module(&mut self, module_id: Uuid) -> Result<(), UnloadModuleError>;
}
