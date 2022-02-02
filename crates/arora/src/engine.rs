use std::{collections::HashMap, fmt::Debug, sync::{Arc, RwLock, atomic::AtomicPtr}, pin::Pin};

use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::{
  actor::{Actor, Addr, Request},
  executor::{self, Executor},
  module::{Module, DispatchError},
  schema::module::low::ModuleDefinition,
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

pub type UnloadModuleResult = Result<(), UnloadModuleError>;

#[derive(Debug)]
pub struct UnloadModule {
  module_id: Uuid,
}

pub type UnloadModuleRequest = Request<UnloadModule, UnloadModuleResult>;

pub struct Lookup {
  module_id: Uuid,
}

pub struct Engine {
  executors: HashMap<&'static str, Box<dyn Executor>>,
  modules: HashMap<Uuid, Box<dyn Module>>,
}

impl Engine {
  fn new(executors: HashMap<&'static str, Box<dyn Executor>>) -> Pin<Box<Engine>> {
    let mut ret = Box::pin(Engine {
      executors,
      modules: HashMap::new(),
    });

    {
      let engine = &mut *ret.as_mut() as *mut Engine;
      for (id, executor) in ret.executors.iter_mut() {
        executor.set_engine(Arc::new(AtomicPtr::new(engine)));
      }
    }
    
    ret
  }

  pub fn load_module(&mut self, module_definition: ModuleDefinition) -> Result<(), LoadModuleError> {
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

    self.modules.insert(module_id, executor.load_module(module_definition)?);

    Ok(())
  }

  pub fn dispatch(&mut self, module_id: &Uuid, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
    let module = self
      .modules
      .get_mut(&module_id)
      .ok_or_else(|| DispatchError::MethodNotFound)?;

    module.dispatch(&method_id, &arg)
  }
}

pub type EngineRef = Arc<AtomicPtr<Engine>>;