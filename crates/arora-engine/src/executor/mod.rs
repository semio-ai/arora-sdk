#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(feature = "wasmtime-host")]
pub mod component;
#[cfg(feature = "native-host")]
pub mod native;
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

    fn unload_module(&mut self, module_id: Uuid) -> Result<(), UnloadModuleError>;
}
