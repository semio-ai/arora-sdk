use std::collections::HashMap;

use arora_types::call::{Call, CallError, CallResult};
use arora_types::module::declared::{AroraFunction, AroraModule};
use arora_types::record::module::frozen::Function;
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
    /// The guest returned an error: a TYPE_ERROR buffer instead of a result,
    /// or a component's `err`.
    Guest {
        message: String,
    },
    /// The call's arguments do not match the parameters the guest module's
    /// header declares for the function: one it does not declare, or a value
    /// not of its parameter's type. The call reached no guest.
    Argument {
        message: String,
    },
}

/// The message of a guest's error buffer — what a guest returns for an error
/// in the Arora Buffers format (`BufferWriter::add_error`): after the size
/// prefix, the error type, then the message's length and bytes. `None` for any
/// other buffer. A message that runs past the buffer is cut where the buffer
/// ends.
pub(crate) fn guest_error(buffer: &[u8]) -> Option<String> {
    use arora_buffers::{BUFFER_SIZE_SIZE, TYPE_ERROR};
    if buffer.get(BUFFER_SIZE_SIZE) != Some(&TYPE_ERROR) {
        return None;
    }
    let start = BUFFER_SIZE_SIZE + 1 + 4;
    let message = buffer.get(BUFFER_SIZE_SIZE + 1..start).map(|len| {
        let len = u32::from_le_bytes(len.try_into().expect("4 bytes")) as usize;
        let message = &buffer[start..buffer.len().min(start.saturating_add(len))];
        String::from_utf8_lossy(message).into_owned()
    });
    Some(message.unwrap_or_else(|| "guest returned error (no message)".to_string()))
}

pub trait Module {
    fn dispatch(&mut self, function_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError>;
}

/// One function of a [`HostModule`]: the decoded [`Call`] in, the
/// [`CallResult`] out. The buffer codec is the module's job, not the
/// function's.
pub type ModuleFn = Box<dyn FnMut(Call) -> Result<CallResult, CallError>>;

/// The declared signature of one [`HostModule`] function — what a device
/// publishes for it over method introspection (`DescribeMethods`), the way a
/// guest module's header declares its exports. A function without one still
/// dispatches; it is just not discoverable.
#[derive(Debug, Clone)]
pub struct FunctionDescription {
    /// The function id calls target.
    pub id: Uuid,
    /// The method name introspection lists.
    pub name: String,
    /// The frozen signature: parameters (name, type, order) and return type.
    pub function: Function,
}

/// A [`Module`] assembled from plain functions — how host-side code enters
/// the engine's dispatch without a guest executor. Build one with
/// [`ModuleBuilder`] and hand it to
/// [`Engine::register_module`](crate::engine::Engine::register_module); its
/// functions are then reachable through `arora_call` exactly like a loaded
/// module's, buffers and all.
pub struct HostModule {
    id: Uuid,
    functions: HashMap<Uuid, ModuleFn>,
    descriptions: Vec<FunctionDescription>,
}

impl HostModule {
    /// The host module a declared one describes: its id, and every export of
    /// [`AroraModule::exports`] attached under its own id with its frozen
    /// signature, so the functions dispatch by parameter id and method
    /// introspection lists them.
    ///
    /// ```ignore
    /// engine.register_module(polly::ids::MODULE, Box::new(HostModule::of::<polly::Module>()));
    /// ```
    pub fn of<M: AroraModule>() -> HostModule {
        HostModule::from_exports(M::id(), M::exports())
    }

    /// The host module that serves `exports` under `id`: each function
    /// attached under its own id with its frozen signature, as
    /// [`of`](Self::of) does for a declared module — for functions whose
    /// module id is not theirs to name, such as a contract's, served under
    /// the id of the module implementing it.
    ///
    /// ```ignore
    /// let module = HostModule::from_exports(CLOUD_ID, say::exports(Cloud::new()));
    /// ```
    pub fn from_exports(id: Uuid, exports: Vec<AroraFunction>) -> HostModule {
        let mut builder = ModuleBuilder::new(id);
        for function in exports {
            builder = builder.described_function(
                function.id,
                function.name,
                function.signature,
                function.invoke,
            );
        }
        builder.build()
    }

    /// The module id this was built for (the id to register it under).
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The declared signatures of this module's described functions — what a
    /// host feeds its method index so the functions are discoverable.
    pub fn descriptions(&self) -> &[FunctionDescription] {
        &self.descriptions
    }
}

impl Module for HostModule {
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

/// Assembles a [`HostModule`]: a generic module with an id, and arbitrary
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
    descriptions: Vec<FunctionDescription>,
}

impl ModuleBuilder {
    /// Start a module under `id`.
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
            functions: HashMap::new(),
            descriptions: Vec::new(),
        }
    }

    /// Attach `function` under `function_id`. Attaching to an id that is
    /// already taken replaces the function.
    ///
    /// The function dispatches but is not discoverable: method introspection
    /// only lists functions attached with
    /// [`described_function`](Self::described_function). Fit for internal
    /// entry points (an interpreter's LOAD/EDIT); anything a remote should
    /// find and call deserves a description.
    pub fn function(
        mut self,
        function_id: Uuid,
        function: impl FnMut(Call) -> Result<CallResult, CallError> + 'static,
    ) -> Self {
        self.functions.insert(function_id, Box::new(function));
        self
    }

    /// Attach `function` under `function_id` with its declared signature, so
    /// method introspection (`DescribeMethods`) lists it — the host-side
    /// equivalent of a guest header's export. `name` is the method name
    /// introspection lists; `signature` freezes the parameters and return
    /// type a caller builds a typed call from.
    pub fn described_function(
        mut self,
        function_id: Uuid,
        name: impl Into<String>,
        signature: Function,
        function: impl FnMut(Call) -> Result<CallResult, CallError> + 'static,
    ) -> Self {
        self.descriptions.push(FunctionDescription {
            id: function_id,
            name: name.into(),
            function: signature,
        });
        self.function(function_id, function)
    }

    /// The finished module.
    pub fn build(self) -> HostModule {
        HostModule {
            id: self.id,
            functions: self.functions,
            descriptions: self.descriptions,
        }
    }
}
