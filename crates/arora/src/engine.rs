use std::{
  collections::HashMap,
  fmt::Debug,
  pin::Pin,
};

use arora_buffers::uuid::deserialize;
use arora_schema::value::Value;
use uuid::Uuid;

use crate::{
  executor::{self, Executor},
  module::{DispatchError, Module},
  schema::module::low::ModuleDefinition, call::{Caller, Call, serialize_to_arg},
};

use derive_more::{Display, Error, From};

pub struct EngineBuilder {
  executors: HashMap<&'static str, Box<dyn Executor>>,
}

impl EngineBuilder {
  pub fn new() -> Self {
    Self {
      executors: HashMap::new(),
    }
  }

  pub fn add_executor<E: Executor + 'static>(mut self, executor: E) -> Self {
    self.executors.insert(executor.name(), Box::new(executor));
    self
  }

  pub fn build(self) -> Pin<Box<Engine>> {
    Engine::new(self.executors)
  }
}

#[derive(Debug, Display, Error, From, Clone)]
pub enum LoadModuleError {
  ExecutorNotFound,
  MalformedExecutable,
  Internal,
}

impl From<executor::LoadModuleError> for LoadModuleError {
  fn from(e: executor::LoadModuleError) -> Self {
    match e {
      executor::LoadModuleError::MalformedExecutable => LoadModuleError::MalformedExecutable,
      executor::LoadModuleError::Internal => LoadModuleError::Internal,
    }
  }
}

#[derive(Debug, Display, Error, From)]
pub enum UnloadModuleError {
  ModuleNotFound,
  Internal,
}

impl From<executor::UnloadModuleError> for UnloadModuleError {
  fn from(e: executor::UnloadModuleError) -> Self {
    match e {
      executor::UnloadModuleError::ModuleNotFound => UnloadModuleError::ModuleNotFound,
      executor::UnloadModuleError::Internal => UnloadModuleError::Internal,
    }
  }
}

/// [`Engine`] is the main encapsulation of the Arora runtime.
/// It consists of a set of [`Executor`]s and [`Module`]s.
pub struct Engine {
  executors: HashMap<&'static str, Box<dyn Executor>>,
  modules: HashMap<Uuid, Box<dyn Module>>,
}

impl Engine {
  /// Create a new [`Engine`] with the given [`Executor`]s.
  fn new(executors: HashMap<&'static str, Box<dyn Executor>>) -> Pin<Box<Engine>> {
    let mut ret = Box::pin(Engine {
      executors,
      modules: HashMap::new(),
    });

    {
      let engine = &mut *ret.as_mut() as *mut Engine;
      for (_, executor) in ret.executors.iter_mut() {
        executor.set_engine(engine as *mut Engine);
      }
    }

    ret
  }

  /// Load a [`Module`] from the given [`ModuleDefinition`].
  pub fn load_module(
    &mut self,
    module_definition: ModuleDefinition,
  ) -> Result<(), LoadModuleError> {
    let module_id = module_definition.header.id;
    let executor_name = module_definition.header.executor.name.as_str();

    if self.modules.contains_key(&module_id) {
      return Ok(());
    }

    // We haven't loaded the module yet.

    // Find the executor for the module.
    let executor = self
      .executors
      .get_mut(executor_name)
      .ok_or_else(|| LoadModuleError::ExecutorNotFound)?;

    self
      .modules
      .insert(module_id, executor.load_module(module_definition)?);

    Ok(())
  }

  /// Dispatch a method call to a module. `arg` must be a raw Arora Buffer.
  pub fn dispatch(
    &mut self,
    module_id: &Uuid,
    function_id: &Uuid,
    arg: &[u8],
  ) -> Result<Box<[u8]>, DispatchError> {
    let module = self
      .modules
      .get_mut(&module_id)
      .ok_or_else(|| DispatchError::ModuleNotFound { id: module_id.clone() })?;

    module.dispatch(&function_id, &arg)
  }
}

pub type EngineRef = *mut Engine;

impl Caller for Engine {
  fn arora_call(&mut self, module: &Uuid, call: Call) -> Result<Value, DispatchError> {
    self.dispatch(&module, &call.id.clone(), serialize_to_arg(call).as_ref())
      .map(|result| deserialize(result.as_ref()))
  }
}