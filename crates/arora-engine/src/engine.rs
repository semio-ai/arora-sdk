use std::{collections::HashMap, fmt::Debug, ops::DerefMut, pin::Pin, rc::Rc};

use arora_buffers::serde_uuid::deserialize;
use arora_types::{
    call::{Call, CallResult},
    value::Value,
};
use uuid::Uuid;

use crate::{
    call::{serialize_to_arg, CallBridge, CallError, Callable, CallableId, CallableRegistry},
    executor::{self, Executor},
    module::{DispatchError, Module},
    schema::module::low::ModuleDefinition,
};

use derive_more::{Display, From};

pub struct EngineBuilder {
    executors: HashMap<&'static str, Box<dyn Executor>>,
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
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

#[derive(Debug, Display, From, Clone)]
pub enum LoadModuleError {
    ExecutorNotFound,
    MalformedExecutable,
    Internal(String),
}

impl std::error::Error for LoadModuleError {}

impl From<executor::LoadModuleError> for LoadModuleError {
    fn from(e: executor::LoadModuleError) -> Self {
        match e {
            executor::LoadModuleError::MalformedExecutable => LoadModuleError::MalformedExecutable,
            executor::LoadModuleError::Internal(message) => LoadModuleError::Internal(message),
        }
    }
}

#[derive(Debug, Display, From)]
pub enum UnloadModuleError {
    ModuleNotFound,
    Internal(String),
}

impl std::error::Error for UnloadModuleError {}

impl From<executor::UnloadModuleError> for UnloadModuleError {
    fn from(e: executor::UnloadModuleError) -> Self {
        match e {
            executor::UnloadModuleError::ModuleNotFound => UnloadModuleError::ModuleNotFound,
            executor::UnloadModuleError::Internal(message) => UnloadModuleError::Internal(message),
        }
    }
}

/// A host-side function addressable through [`CallBridge::arora_call`] like a
/// loaded module's function: registered under a module + function id pair, it
/// receives the whole [`Call`] (arguments included) and returns the
/// [`CallResult`] directly — no guest executor, no buffer round-trip.
///
/// This is how the runtime exposes its *golden* functions (e.g.
/// arora-behavior's behavior edit) to remotes and behaviors alike: the same
/// dispatch that reaches guest code reaches these.
pub trait HostFunction {
    fn call(&self, call: Call) -> Result<CallResult, CallError>;
}

/// [`Engine`] is the main encapsulation of the Arora runtime.
/// It consists of a set of [`Executor`]s and [`Module`]s.
pub struct Engine {
    executors: HashMap<&'static str, Box<dyn Executor>>,
    modules: HashMap<Uuid, Box<dyn Module>>,
    callables: CallableRegistry,
    host_functions: HashMap<(Uuid, Uuid), Rc<dyn HostFunction>>,
}

impl Engine {
    /// Create a new [`Engine`] with the given [`Executor`]s.
    fn new(executors: HashMap<&'static str, Box<dyn Executor>>) -> Pin<Box<Engine>> {
        let mut ret = Box::pin(Engine {
            executors,
            modules: HashMap::new(),
            callables: CallableRegistry::new(),
            host_functions: HashMap::new(),
        });

        {
            let engine = &mut *ret.as_mut() as *mut Engine;
            for executor in ret.executors.values_mut() {
                executor.set_engine(engine);
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
            .ok_or(LoadModuleError::ExecutorNotFound)?;

        self.modules
            .insert(module_id, executor.load_module(module_definition)?);

        Ok(())
    }

    /// Register a [`HostFunction`] under a module + function id, making it
    /// reachable through [`CallBridge::arora_call`] exactly like a loaded
    /// module's function. A host function registered under an id pair shadows
    /// nothing: guest dispatch is only consulted when no host function
    /// matches.
    pub fn register_host_function(
        &mut self,
        module_id: Uuid,
        function_id: Uuid,
        function: Rc<dyn HostFunction>,
    ) {
        self.host_functions
            .insert((module_id, function_id), function);
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
            .get_mut(module_id)
            .ok_or(DispatchError::ModuleNotFound { id: *module_id })?;

        module.dispatch(function_id, arg)
    }
}

pub type EngineRef = *mut Engine;

impl CallBridge for Engine {
    fn arora_call(&mut self, module: &Uuid, call: Call) -> Result<CallResult, CallError> {
        if let Some(host) = self.host_functions.get(&(*module, call.id)).cloned() {
            return host.call(call);
        }
        let call_id = call.id;
        let result_data = self
            .dispatch(module, &call_id, serialize_to_arg(call).as_ref())
            .map_err(Into::<CallError>::into)?;
        if let Value::Structure(structure) = deserialize(result_data.as_ref()) {
            if call_id != structure.id {
                Err(CallError::Internal {
                    message: format!(
                        "result id {} differs from function id {}",
                        structure.id, call_id
                    ),
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
                message: "call result did not contain a return value".to_string(),
            })?;
            Ok(CallResult { ret, mutated })
        } else {
            Err(CallError::Internal {
                message: "returned data was not a structure".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Doubler;
    impl HostFunction for Doubler {
        fn call(&self, call: Call) -> Result<CallResult, CallError> {
            let Value::I32(n) = call.args[0].value.as_ref() else {
                return Err(CallError::Guest {
                    message: "expected an i32".to_string(),
                });
            };
            Ok(CallResult {
                ret: Value::I32(n * 2),
                mutated: Vec::new(),
            })
        }
    }

    #[test]
    fn host_functions_dispatch_like_module_functions() {
        let mut engine = EngineBuilder::new().build();
        let module = Uuid::from_u128(1);
        let function = Uuid::from_u128(2);
        engine.register_host_function(module, function, Rc::new(Doubler));

        let call = Call {
            module_id: Some(module),
            id: function,
            args: vec![arora_types::value::StructureField {
                id: Uuid::from_u128(3),
                value: Box::new(Value::I32(21)),
            }],
        };
        let result = engine.arora_call(&module, call).unwrap();
        assert_eq!(result.ret, Value::I32(42));

        // An unregistered id still falls through to guest dispatch (and fails
        // there, since no module is loaded).
        let miss = Call {
            module_id: Some(module),
            id: Uuid::from_u128(4),
            args: Vec::new(),
        };
        assert!(matches!(
            engine.arora_call(&module, miss),
            Err(CallError::ModuleNotFound { .. })
        ));
    }
}
