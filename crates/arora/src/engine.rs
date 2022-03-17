use std::{collections::HashMap, fmt::Debug, ops::DerefMut, pin::Pin, rc::Rc};

use arora_buffers::serde_uuid::deserialize;
use arora_schema::value::Value;
use uuid::Uuid;

use crate::{
  call::{serialize_to_arg, Call, CallBridge, CallError, Callable, CallableId, CallableRegistry, CallResult},
  executor::{self, Executor},
  module::{DispatchError, Module},
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

/// [`Engine`] is the main encapsulation of the Arora runtime.
/// It consists of a set of [`Executor`]s and [`Module`]s.
pub struct Engine {
  executors: HashMap<&'static str, Box<dyn Executor>>,
  modules: HashMap<Uuid, Box<dyn Module>>,
  callables: CallableRegistry,
}

impl Engine {
  /// Create a new [`Engine`] with the given [`Executor`]s.
  fn new(executors: HashMap<&'static str, Box<dyn Executor>>) -> Pin<Box<Engine>> {
    let mut ret = Box::pin(Engine {
      executors,
      modules: HashMap::new(),
      callables: CallableRegistry::new(),
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
      .ok_or_else(|| DispatchError::ModuleNotFound {
        id: module_id.clone(),
      })?;

    module.dispatch(&function_id, &arg)
  }
}

pub type EngineRef = *mut Engine;

impl CallBridge for Engine {
  fn arora_call(&mut self, module: &Uuid, call: Call) -> Result<CallResult, CallError> {
    let call_id = call.id.clone();
    let result_data = self
      .dispatch(&module, &call_id, serialize_to_arg(call).as_ref())
      .map_err(Into::<CallError>::into)?;
    if let Value::Structure(structure) = deserialize(result_data.as_ref()) {
      if call_id != structure.id {
        Err(CallError::Internal {
          message: format!("result id {} differs from function id {}", structure.id, call_id)
        })?
      }
      let mut ret = None;
      let mut mutated = Vec::with_capacity(structure.fields.len() - 1);
      for field in structure.fields {
        // First field must be the return value.
        if ret.is_none() {
          assert_eq!(field.id, call_id);
          ret = Some(*field.value);
        } else {
          mutated.push(field);
        }
      }
      let ret = ret.ok_or(CallError::Internal {
        message: "call result did not contain a return value".to_string()
      })?;
      Ok(CallResult {
        ret,
        mutated,
      })
    } else {
      Err(CallError::Internal {
        message: "returned data was not a structure".to_string()
      })
    }
  }

  fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
    self.callables.register_callable(callable).unwrap()
  }

  fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
    self.callables.unregister_callable(callable_id).unwrap()
  }

  fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
    self.callables.find_callable(callable_id)?.call(self)
  }
}

pub type PinnedEngine = Pin<Box<Engine>>;

impl CallBridge for PinnedEngine {
  fn arora_call(&mut self, module: &Uuid, call: Call) -> Result<CallResult, CallError> {
    self.deref_mut().arora_call(module, call)
  }

  fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
    self.deref_mut().arora_register_callable(callable)
  }

  fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
    self.deref_mut().arora_unregister_callable(callable_id)
  }

  fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
    self.deref_mut().arora_call_indirect(callable_id)
  }
}
