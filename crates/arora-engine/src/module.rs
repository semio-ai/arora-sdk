use std::collections::HashMap;

use arora_types::call::{Call, CallError, CallResult};
use derive_more::{Display, Error};
use uuid::Uuid;

use crate::call::{decode_arg, encode_call_result};

#[derive(Display, Debug, Error)]
pub enum DispatchError {
    ModuleNotFound {
        id: Uuid,
    },
    FunctionNotFound {
        id: Uuid,
    },
    Trap {
        message: String,
    },
    Internal {
        message: String,
    },
    /// The guest returned a TYPE_ERROR buffer instead of a result.
    Guest {
        message: String,
    },
}

pub trait Module {
    fn dispatch(&mut self, function_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError>;
}

/// One function of a [`FunctionModule`]: the decoded [`Call`] in, the
/// [`CallResult`] out. The buffer codec is the module's job, not the
/// function's.
pub type ModuleFn = Box<dyn FnMut(Call) -> Result<CallResult, CallError>>;

/// A [`Module`] assembled from plain functions — how host-side code enters
/// the engine's dispatch without a guest executor. Build one with
/// [`ModuleBuilder`] and hand it to
/// [`Engine::register_module`](crate::engine::Engine::register_module); its
/// functions are then reachable through `arora_call` exactly like a loaded
/// module's, buffers and all.
pub struct FunctionModule {
    id: Uuid,
    functions: HashMap<Uuid, ModuleFn>,
}

impl FunctionModule {
    /// The module id this was built for (the id to register it under).
    pub fn id(&self) -> Uuid {
        self.id
    }
}

impl Module for FunctionModule {
    fn dispatch(&mut self, function_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
        let function = self
            .functions
            .get_mut(function_id)
            .ok_or(DispatchError::FunctionNotFound { id: *function_id })?;
        let call =
            decode_arg(*function_id, arg).map_err(|message| DispatchError::Internal { message })?;
        let result = function(call).map_err(|e| match e {
            CallError::Guest { message } => DispatchError::Guest { message },
            other => DispatchError::Guest {
                message: other.to_string(),
            },
        })?;
        Ok(encode_call_result(*function_id, result))
    }
}

/// Assembles a [`FunctionModule`]: a generic module with an id, and arbitrary
/// functions attached to it — each under its own function id.
///
/// ```ignore
/// let module = ModuleBuilder::new(module_id)
///     .function(load_id, move |call| { /* ... */ })
///     .function(edit_id, move |call| { /* ... */ })
///     .build();
/// engine.register_module(module.id(), Box::new(module));
/// ```
pub struct ModuleBuilder {
    id: Uuid,
    functions: HashMap<Uuid, ModuleFn>,
}

impl ModuleBuilder {
    /// Start a module under `id`.
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
            functions: HashMap::new(),
        }
    }

    /// Attach `function` under `function_id`. Attaching to an id that is
    /// already taken replaces the function.
    pub fn function(
        mut self,
        function_id: Uuid,
        function: impl FnMut(Call) -> Result<CallResult, CallError> + 'static,
    ) -> Self {
        self.functions.insert(function_id, Box::new(function));
        self
    }

    /// The finished module.
    pub fn build(self) -> FunctionModule {
        FunctionModule {
            id: self.id,
            functions: self.functions,
        }
    }
}
