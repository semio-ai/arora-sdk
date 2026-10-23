use std::{collections::HashMap, fmt::Debug, ops::DerefMut, pin::Pin, rc::Rc};

use arora_buffers::serde_uuid::deserialize;
use arora_types::{
    call::{Call, CallResult},
    value::Value,
};
use uuid::Uuid;

use crate::arguments::Signatures;
use crate::limits::GuestLimits;
use crate::{
    call::{serialize_to_arg, CallBridge, CallError, Callable, CallableId, CallableRegistry},
    compiled::CompiledModule,
    executor::{self, Executor},
    module::{DispatchError, Module},
    schema::module::low::ModuleDefinition,
};

use derive_more::{Display, From};

pub struct EngineBuilder {
    executors: HashMap<&'static str, Box<dyn Executor>>,
    guest_limits: GuestLimits,
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
            guest_limits: GuestLimits::default(),
        }
    }

    /// The limits of every guest module the engine loads whose own limits
    /// are not set ([`Engine::set_guest_limits`]); [`GuestLimits::default`]
    /// unless given.
    pub fn with_guest_limits(mut self, limits: GuestLimits) -> Self {
        self.guest_limits = limits;
        self
    }

    pub fn add_executor<E: Executor + 'static>(mut self, executor: E) -> Self {
        self.executors.insert(executor.name(), Box::new(executor));
        self
    }

    pub fn build(self) -> Pin<Box<Engine>> {
        Engine::new(self.executors, self.guest_limits)
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
    /// The executor that loaded the module cannot unload it
    /// ([`Executor::can_unload`]): the module stays loaded.
    #[display("the {executor} executor cannot unload a module it loaded")]
    #[from(ignore)]
    Unsupported {
        executor: &'static str,
    },
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

/// [`Engine::reload_module`] failed: nothing was replaced.
#[derive(Debug, Display, Clone)]
pub enum ReloadModuleError {
    /// No module is loaded under the id.
    ModuleNotFound,
    /// The executor that loaded the module cannot unload it
    /// ([`Executor::can_unload`]): the module stays loaded.
    #[display("the {executor} executor cannot unload a module it loaded")]
    Unsupported { executor: &'static str },
    /// The new version did not load; the old one is still loaded.
    Load(LoadModuleError),
}

impl std::error::Error for ReloadModuleError {}

impl From<LoadModuleError> for ReloadModuleError {
    fn from(e: LoadModuleError) -> Self {
        ReloadModuleError::Load(e)
    }
}

impl From<executor::LoadModuleError> for ReloadModuleError {
    fn from(e: executor::LoadModuleError) -> Self {
        ReloadModuleError::Load(e.into())
    }
}

/// [`Engine`] is the main encapsulation of the Arora runtime.
/// It consists of a set of [`Executor`]s and [`Module`]s.
pub struct Engine {
    executors: HashMap<&'static str, Box<dyn Executor>>,
    modules: HashMap<Uuid, Loaded>,
    callables: CallableRegistry,
    /// The limits of a guest module whose own are not set.
    guest_limits: GuestLimits,
    /// Each module's own limits, set with [`Engine::set_guest_limits`].
    module_limits: HashMap<Uuid, GuestLimits>,
    /// When the step under way ends, which no guest call outlasts.
    #[cfg(not(target_arch = "wasm32"))]
    step_deadline: Option<std::time::Instant>,
    /// The guest calls under way, outermost first, each with its deadline and
    /// budget: a guest calling another, or itself, through the host, and a
    /// guest loading, whose initializer can call others. A nested call ends
    /// by its caller's deadline, and a module called again while its call is
    /// under way ends by the earlier of that call's deadline and its caller's.
    #[cfg(not(target_arch = "wasm32"))]
    guest_calls: Vec<(Uuid, std::time::Instant, std::time::Duration)>,
}

/// A module the engine dispatches to, and the executor that loaded it —
/// `None` for one registered already instantiated.
struct Loaded {
    executor: Option<&'static str>,
    module: Box<dyn Module>,
    /// A guest module's declared functions, against which each call's
    /// arguments are checked; `None` for a registered host module.
    signatures: Option<Signatures>,
}

impl Engine {
    /// Create a new [`Engine`] with the given [`Executor`]s.
    fn new(
        executors: HashMap<&'static str, Box<dyn Executor>>,
        guest_limits: GuestLimits,
    ) -> Pin<Box<Engine>> {
        let mut ret = Box::pin(Engine {
            executors,
            modules: HashMap::new(),
            callables: CallableRegistry::new(),
            guest_limits,
            module_limits: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            step_deadline: None,
            #[cfg(not(target_arch = "wasm32"))]
            guest_calls: Vec::new(),
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

        let signatures = Signatures::of(&module_definition.header);
        let limits = self.guest_limits(&module_id);
        // Find the executor for the module.
        let executor = self
            .executors
            .get_mut(executor_name)
            .ok_or(LoadModuleError::ExecutorNotFound)?;
        #[cfg(not(target_arch = "wasm32"))]
        self.guest_calls
            .push(loading(module_id, self.guest_calls.last()));
        let module = executor.load_module_limited(module_definition, limits);
        #[cfg(not(target_arch = "wasm32"))]
        self.guest_calls.pop();
        let module = module?;
        self.modules.insert(
            module_id,
            Loaded {
                executor: Some(executor.name()),
                module,
                signatures: Some(signatures),
            },
        );

        Ok(())
    }

    /// Load a [`Module`] from `module`, compiled ahead: the executor its header
    /// names instantiates the compiled code instead of compiling the
    /// executable, so loading one [`CompiledModule`] into many engines compiles
    /// it once. Otherwise as [`load_module`](Self::load_module): an id already
    /// loaded is answered with Ok and left as it is.
    pub fn load_compiled_module(&mut self, module: &CompiledModule) -> Result<(), LoadModuleError> {
        let module_id = module.header().id;
        if self.modules.contains_key(&module_id) {
            return Ok(());
        }
        let limits = self.guest_limits(&module_id);
        let executor = self
            .executors
            .get_mut(module.header().executor.name.as_str())
            .ok_or(LoadModuleError::ExecutorNotFound)?;
        #[cfg(not(target_arch = "wasm32"))]
        self.guest_calls
            .push(loading(module_id, self.guest_calls.last()));
        let instance = executor.load_compiled_module_limited(module, limits);
        #[cfg(not(target_arch = "wasm32"))]
        self.guest_calls.pop();
        let instance = instance?;
        self.modules.insert(
            module_id,
            Loaded {
                executor: Some(executor.name()),
                module: instance,
                signatures: Some(Signatures::of(module.header())),
            },
        );
        Ok(())
    }

    /// Replace the module loaded under `module`'s id with a fresh instance of
    /// `module`: from the next dispatch, its functions run the new code, in a
    /// new instance with its own memory and globals.
    ///
    /// The new instance is created before the old one is touched, so a module
    /// that fails to load leaves the old one loaded and serving, and the error
    /// is the load's. The old instance is dropped once replaced, which releases
    /// its memory. An id with no module loaded fails with
    /// [`ReloadModuleError::ModuleNotFound`]: loading is
    /// [`load_compiled_module`](Self::load_compiled_module)'s job. A module
    /// whose executor cannot unload it ([`Executor::can_unload`]) is refused
    /// with [`ReloadModuleError::Unsupported`], before anything loads.
    ///
    /// Not reachable from a dispatch: a module cannot reload itself (or any
    /// other) while a call into it is on the stack, since the instance it runs
    /// in would be dropped under it. Call it between dispatches.
    pub fn reload_module(&mut self, module: &CompiledModule) -> Result<(), ReloadModuleError> {
        let module_id = module.header().id;
        let loaded = self
            .modules
            .get(&module_id)
            .ok_or(ReloadModuleError::ModuleNotFound)?;
        if let Some(executor) = self.refusing_unload(loaded) {
            return Err(ReloadModuleError::Unsupported { executor });
        }
        let limits = self.guest_limits(&module_id);
        let executor = self
            .executors
            .get_mut(module.header().executor.name.as_str())
            .ok_or(LoadModuleError::ExecutorNotFound)?;
        #[cfg(not(target_arch = "wasm32"))]
        self.guest_calls
            .push(loading(module_id, self.guest_calls.last()));
        let instance = executor.load_compiled_module_limited(module, limits);
        #[cfg(not(target_arch = "wasm32"))]
        self.guest_calls.pop();
        let instance = instance?;
        self.modules.insert(
            module_id,
            Loaded {
                executor: Some(executor.name()),
                module: instance,
                signatures: Some(Signatures::of(module.header())),
            },
        );
        Ok(())
    }

    /// Whether a module — loaded or registered — dispatches under `id`.
    pub fn contains_module(&self, id: &Uuid) -> bool {
        self.modules.contains_key(id)
    }

    /// Remove the module loaded under `id` from dispatch and drop its
    /// instance: a call to it then fails with
    /// [`DispatchError::ModuleNotFound`]. Like
    /// [`reload_module`](Self::reload_module), not reachable from a dispatch.
    ///
    /// Any module leaves this way, a host module registered with
    /// [`register_module`](Self::register_module) included: which modules a
    /// host lets go is the host's decision. A module whose executor cannot
    /// unload it ([`Executor::can_unload`]) is refused with
    /// [`UnloadModuleError::Unsupported`] and stays loaded.
    pub fn unload_module(&mut self, id: &Uuid) -> Result<(), UnloadModuleError> {
        let loaded = self
            .modules
            .get(id)
            .ok_or(UnloadModuleError::ModuleNotFound)?;
        if let Some(executor) = self.refusing_unload(loaded) {
            return Err(UnloadModuleError::Unsupported { executor });
        }
        self.modules.remove(id);
        Ok(())
    }

    /// The executor that loaded `loaded`, when it cannot unload it.
    fn refusing_unload(&self, loaded: &Loaded) -> Option<&'static str> {
        let executor = loaded.executor?;
        let can_unload = self
            .executors
            .get(executor)
            .is_none_or(|executor| executor.can_unload());
        (!can_unload).then_some(executor)
    }

    /// Set the limits of the guest module `module`, loaded or not: its memory
    /// cap applies to the instances it loads from then on, its call budget
    /// to its next call.
    pub fn set_guest_limits(&mut self, module: Uuid, limits: GuestLimits) {
        self.module_limits.insert(module, limits);
    }

    /// The limits of the guest module `module`: its own if set, else the
    /// engine's ([`EngineBuilder::with_guest_limits`]).
    pub fn guest_limits(&self, module: &Uuid) -> GuestLimits {
        self.module_limits
            .get(module)
            .copied()
            .unwrap_or(self.guest_limits)
    }

    /// Bound every guest call from now on to end within `budget`, the time
    /// left in the step a device is running; `None` when the step ends. A
    /// device's run loop sets it to its step period before each step, so the
    /// guest calls of one step cannot together outlast it. In the browser,
    /// where nothing interrupts a guest, it does nothing.
    pub fn set_step_budget(&mut self, budget: Option<std::time::Duration>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.step_deadline = budget.map(|budget| std::time::Instant::now() + budget);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = budget;
    }

    /// When a call to `module` starting now must end, and the budget that
    /// gives it: the earliest of the step's end, the module's own budget and,
    /// outside a step, [`DEFAULT_CALL_BUDGET`](crate::limits::DEFAULT_CALL_BUDGET).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn call_deadline(&self, module: &Uuid) -> (std::time::Instant, std::time::Duration) {
        let now = std::time::Instant::now();
        let own = self
            .guest_limits(module)
            .call_budget
            .map(|budget| now + budget);
        let deadline = match (self.step_deadline, own) {
            (Some(step), Some(own)) => step.min(own),
            (Some(step), None) => step,
            (None, Some(own)) => own,
            (None, None) => now + crate::limits::DEFAULT_CALL_BUDGET,
        };
        // A guest's call to another ends by its caller's deadline.
        let deadline = match self.guest_calls.last() {
            Some((_, caller, _)) => deadline.min(*caller),
            None => deadline,
        };
        (deadline, deadline.saturating_duration_since(now))
    }

    /// Register an already-instantiated [`Module`] under `id`, making its
    /// functions reachable through [`CallBridge::arora_call`] exactly like a
    /// loaded module's. Where [`load_module`](Self::load_module) instantiates a
    /// module from a definition through an [`Executor`], this hands one over
    /// directly — how host-side modules (e.g. a
    /// [`HostModule`](crate::module::HostModule) built from closures)
    /// enter the same dispatch as guest code. Registering an id that is
    /// already present replaces the module.
    pub fn register_module(&mut self, id: Uuid, module: Box<dyn Module>) {
        self.modules.insert(
            id,
            Loaded {
                executor: None,
                module,
                signatures: None,
            },
        );
    }

    /// Dispatch a method call to a module. `arg` must be a raw Arora Buffer.
    ///
    /// A call to a guest module whose arguments name a parameter the function
    /// does not declare, or carry a value not of its parameter's type, fails
    /// with [`DispatchError::Argument`] and reaches no guest. `unit` passes for
    /// any parameter, as callers pass it for an unset argument, and a record-
    /// typed argument is checked by its record's id. Left to the guest: an
    /// `arg` that is not an Arora Buffer or not a structure, a function the
    /// header does not declare, and a parameter the call leaves out.
    pub fn dispatch(
        &mut self,
        module_id: &Uuid,
        function_id: &Uuid,
        arg: &[u8],
    ) -> Result<Box<[u8]>, DispatchError> {
        #[cfg(not(target_arch = "wasm32"))]
        let (deadline, budget) = self.call_deadline(module_id);
        let module = self
            .modules
            .get_mut(module_id)
            .ok_or(DispatchError::ModuleNotFound { id: *module_id })?;
        let guest = match &module.signatures {
            Some(signatures) => {
                signatures
                    .check(function_id, arg)
                    .map_err(|message| DispatchError::Argument { message })?;
                true
            }
            None => false,
        };
        #[cfg(not(target_arch = "wasm32"))]
        if guest {
            // A module called again while its call is under way ends by that
            // call's deadline, or its caller's if earlier: its own budget,
            // counted anew, would extend the call under way. A tighter
            // deadline holds for this call only; the call under way gets its
            // own back when this one returns.
            let under_way = self
                .guest_calls
                .iter()
                .rev()
                .find(|(id, _, _)| id == module_id)
                .map(|(_, deadline, budget)| (*deadline, *budget));
            let (deadline, budget, restore) = match under_way {
                None => {
                    module.module.set_call_deadline(deadline, budget);
                    (deadline, budget, None)
                }
                Some((outer, outer_budget)) if deadline < outer => {
                    module.module.set_call_deadline(deadline, budget);
                    (deadline, budget, Some((outer, outer_budget)))
                }
                Some((outer, outer_budget)) => (outer, outer_budget, None),
            };
            self.guest_calls.push((*module_id, deadline, budget));
            let result = module.module.dispatch(function_id, arg);
            self.guest_calls.pop();
            if let Some((outer, outer_budget)) = restore {
                // Reloading is not reachable from a dispatch: the module is
                // still the one called.
                if let Some(module) = self.modules.get_mut(module_id) {
                    module.module.set_call_deadline(outer, outer_budget);
                }
            }
            return result;
        }
        let _ = guest;
        module.module.dispatch(function_id, arg)
    }
}

/// The call under way while the module `module` loads: its initializer runs
/// to [`DEFAULT_CALL_BUDGET`](crate::limits::DEFAULT_CALL_BUDGET), as the
/// executors bound it, or to its caller's deadline if earlier, so a guest it
/// calls ends by then too.
#[cfg(not(target_arch = "wasm32"))]
fn loading(
    module: Uuid,
    caller: Option<&(Uuid, std::time::Instant, std::time::Duration)>,
) -> (Uuid, std::time::Instant, std::time::Duration) {
    let now = std::time::Instant::now();
    let mut deadline = now + crate::limits::DEFAULT_CALL_BUDGET;
    if let Some((_, caller, _)) = caller {
        deadline = deadline.min(*caller);
    }
    (module, deadline, deadline.saturating_duration_since(now))
}

pub type EngineRef = *mut Engine;

impl CallBridge for Engine {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        let module = call.module_id.ok_or_else(|| CallError::Generic {
            message: "call is missing its module id".to_string(),
        })?;
        let call_id = call.id;
        let result_data = self
            .dispatch(&module, &call_id, serialize_to_arg(call).as_ref())
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
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        self.deref_mut().arora_call(call)
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
    use crate::module::ModuleBuilder;

    #[test]
    fn function_modules_dispatch_like_loaded_modules() {
        let mut engine = EngineBuilder::new().build();
        let module_id = Uuid::from_u128(1);
        let function = Uuid::from_u128(2);
        let module = ModuleBuilder::new(module_id)
            .function(function, |call: Call| {
                let Value::I32(n) = call.args[0].value.as_ref() else {
                    return Err(CallError::Guest {
                        message: "expected an i32".to_string(),
                    });
                };
                Ok(CallResult {
                    ret: Value::I32(n * 2),
                    mutated: Vec::new(),
                })
            })
            .build();
        engine.register_module(module.id(), Box::new(module));

        let call = Call {
            module_id: Some(module_id),
            id: function,
            args: vec![arora_types::value::StructureField {
                id: Uuid::from_u128(3),
                value: Box::new(Value::I32(21)),
            }],
        };
        let result = engine.arora_call(call).unwrap();
        assert_eq!(result.ret, Value::I32(42));

        // An unattached function id fails like a missing guest function.
        let miss = Call {
            module_id: Some(module_id),
            id: Uuid::from_u128(4),
            args: Vec::new(),
        };
        assert!(matches!(
            engine.arora_call(miss),
            Err(CallError::FunctionNotFound { .. })
        ));

        // An unloaded module is gone from dispatch.
        assert!(engine.contains_module(&module_id));
        engine.unload_module(&module_id).expect("unload");
        assert!(!engine.contains_module(&module_id));
        assert!(matches!(
            engine.arora_call(Call {
                module_id: Some(module_id),
                id: function,
                args: Vec::new(),
            }),
            Err(CallError::ModuleNotFound { .. })
        ));
        assert!(matches!(
            engine.unload_module(&module_id),
            Err(UnloadModuleError::ModuleNotFound)
        ));

        // An unregistered module id fails as before.
        let other = Uuid::from_u128(9);
        let elsewhere = Call {
            module_id: Some(other),
            id: function,
            args: Vec::new(),
        };
        assert!(matches!(
            engine.arora_call(elsewhere),
            Err(CallError::ModuleNotFound { .. })
        ));
    }
}
