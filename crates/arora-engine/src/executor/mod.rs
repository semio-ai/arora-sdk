#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(feature = "wasmtime-host")]
pub mod component;
#[cfg(feature = "native-host")]
pub mod native;
mod wasi_limits;
#[cfg(feature = "wasmtime-host")]
pub mod wasm;

use crate::compiled::{Code, CompiledModule};
use crate::load::module_definition_from_parts;
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

use crate::limits::GuestLimits;

pub trait Executor {
    fn set_engine(&mut self, engine: EngineRef);

    fn name(&self) -> &'static str;
    fn load_module(
        &mut self,
        module_definition: ModuleDefinition,
    ) -> Result<Box<dyn Module>, LoadModuleError>;

    /// Instantiate `module`, compiled ahead by [`CompiledModule::new`] for the
    /// executor its header names — this one.
    ///
    /// The default serves an executor that loads its executable as it is: it
    /// hands the module's bytes to [`load_module`](Self::load_module), and
    /// refuses a module compiled for the WebAssembly executor. The WebAssembly
    /// executors override it to instantiate the compiled code; another executor
    /// registered under `"wasm"` receives compiled modules too, and overrides
    /// it to accept them.
    fn load_compiled_module(
        &mut self,
        module: &CompiledModule,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        match &module.code {
            Code::Executable(executable) => self.load_module(module_definition_from_parts(
                module.header.clone(),
                executable.as_ref().into(),
            )),
            #[allow(unreachable_patterns)]
            _ => Err(LoadModuleError::Internal(format!(
                "the {} executor cannot instantiate a module compiled for the WebAssembly \
                 executor",
                self.name()
            ))),
        }
    }

    /// Load `module_definition` under `limits`, the memory and time its
    /// guest may spend ([`crate::limits`]). The engine loads every module
    /// through this; the default loads it as
    /// [`load_module`](Self::load_module) does, unbounded. The wasmtime
    /// executors override it to bound the instance.
    fn load_module_limited(
        &mut self,
        module_definition: ModuleDefinition,
        limits: GuestLimits,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        let _ = limits;
        self.load_module(module_definition)
    }

    /// [`load_compiled_module`](Self::load_compiled_module) under `limits`,
    /// as [`load_module_limited`](Self::load_module_limited) is
    /// [`load_module`](Self::load_module) under them.
    fn load_compiled_module_limited(
        &mut self,
        module: &CompiledModule,
        limits: GuestLimits,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        let _ = limits;
        self.load_compiled_module(module)
    }

    fn unload_module(&mut self, module_id: Uuid) -> Result<(), UnloadModuleError>;

    /// Whether the engine may drop one of this executor's modules while it
    /// runs — to reload or unload it ([`Engine::reload_module`],
    /// [`Engine::unload_module`]). The engine owns each module's instance, so
    /// dropping it is the unload. An executor whose modules cannot be dropped
    /// safely while the process runs says no, and the engine keeps them.
    ///
    /// [`Engine::reload_module`]: crate::engine::Engine::reload_module
    /// [`Engine::unload_module`]: crate::engine::Engine::unload_module
    fn can_unload(&self) -> bool {
        true
    }
}
