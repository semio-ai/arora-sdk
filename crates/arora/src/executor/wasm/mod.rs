use std::collections::HashMap;
use std::convert::TryFrom;

use anyhow::Result;
use bytes::Buf;
use derive_more::{Display, Error, From};
use uuid::Uuid;
use wasmtime::{
    AsContextMut, Caller, Config, Engine as WasmEngine, Extern, Linker, Memory,
    Module as WasmModule, Store, TypedFunc,
};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::WasiCtxBuilder;

use arora_buffers::serde_uuid::serialize;
use arora_buffers::{BUFFER_SIZE_SIZE, TYPE_ERROR};
use arora_types::module::low::{ExportSymbol, ModuleDefinition};

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::call::{CallBridge, CallableId};
use crate::{
    engine::EngineRef,
    executor::wasm::guest::AroraBuffer,
    module::{DispatchError, Module},
};

mod guest;
use guest::ReadWasmMemory;

#[derive(Debug, Error, Display, From)]
pub enum InitializationError {
    Internal(#[error(not(source))] anyhow::Error),
}

/// [`EngineRef`] wrapper satisfying the `Send` bound that
/// [`wasmtime_wasi::p1::add_to_linker_sync`] puts on store data.
///
/// SAFETY: the engine and the modules holding this pointer live and run on a
/// single thread; `Send` is only claimed to satisfy the linker bound, the
/// pointer never actually crosses threads.
struct EnginePtr(EngineRef);
unsafe impl Send for EnginePtr {}

/// Host-side state held by each module's [`Store`].
struct HostState {
    wasi: WasiP1Ctx,

    /// The [`crate::engine::Engine`] this module belongs to. Guest calls to
    /// `arora_dispatch` re-enter the engine while it is already mutably
    /// borrowed further up the stack, which is why this is a raw pointer
    /// rather than a safe reference (see [`EngineRef`]).
    engine: EnginePtr,

    /// The guest's `arora_buffer_alloc` export, used by dispatch handlers to
    /// allocate result buffers in guest memory. Set right after instantiation.
    malloc: Option<TypedFunc<(u32,), u32>>,
}

pub struct WebAssemblyExecutor {
    engine: WasmEngine,
    arora_engine: Option<EngineRef>,
}

impl WebAssemblyExecutor {
    pub fn new() -> Result<Self, InitializationError> {
        let mut config = Config::new();
        // config.async_support(true);
        config.debug_info(cfg!(debug_assertions));
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::pooling());
        // config.profiler(wasmtime::ProfilingStrategy::VTune).unwrap();

        Ok(Self {
            engine: WasmEngine::new(&config).map_err(anyhow::Error::from)?,
            arora_engine: None,
        })
    }
}

impl Executor for WebAssemblyExecutor {
    fn set_engine(&mut self, engine: EngineRef) {
        self.arora_engine = Some(engine);
    }

    fn name(&self) -> &'static str {
        "wasm"
    }

    fn load_module(
        &mut self,
        module_definition: ModuleDefinition,
    ) -> Result<Box<dyn Module>, LoadModuleError> {
        let module = WasmModule::new(&self.engine, &module_definition.executable)
            .map_err(|_| LoadModuleError::MalformedExecutable)?;
        let state = HostState {
            wasi: WasiCtxBuilder::new().inherit_stdio().build_p1(),
            engine: EnginePtr(self.arora_engine.unwrap()),
            malloc: None,
        };

        let store = Store::new(&self.engine, state);

        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |s: &mut HostState| &mut s.wasi)
            .map_err(|err| {
                LoadModuleError::Internal(format!("failed to map to wasm linker: {}", err))
            })?;

        Ok(Box::new(
            WebAssemblyModule::new(module_definition.header.exports, module, store, linker)
                .unwrap(),
        ))
    }

    fn unload_module(&mut self, _: Uuid) -> Result<(), UnloadModuleError> {
        unimplemented!("unload_module");
    }
}

/// Maintains references to a web assembly module,
/// and to the buffers used to exchange data with it.
struct WebAssemblyModule {
    /// The WASM module.
    module: WasmModule,

    /// Malloc function provided by the module.
    malloc: TypedFunc<(u32,), u32>,

    /// Free function provided by the module.
    free: TypedFunc<(u32,), ()>,

    /// Map of functions exported for arora.
    arora_functions: HashMap<Uuid, TypedFunc<(u32,), u32>>,

    /// The chunk of memory currently allocated to pass arguments as (ptr, size).
    current_arg_memory: Option<(u32, u32)>,

    /// The memory pool taken by the WASM module.
    memory: Memory,

    store: Store<HostState>,
}

impl WebAssemblyModule {
    fn arora_dispatch(
        mut caller: Caller<'_, HostState>,
        module_id: u32,
        method_id: u32,
        arg: u32,
    ) -> u32 {
        println!(
            "arora_dispatch: module_id: {}, method_id: {}, arg: {}",
            module_id, method_id, arg
        );

        let memory = caller
            .get_export("memory")
            .and_then(Extern::into_memory)
            .unwrap();

        // SAFETY: re-entrant access to the engine; see `HostState::engine`.
        let engine = unsafe { &mut *caller.data().engine.0 };
        let malloc = caller.data().malloc.as_ref().unwrap().clone();

        let mut context = caller.as_context_mut();

        let module_id = Uuid::read_wasm_memory(&context, memory, module_id);
        let method_id = Uuid::read_wasm_memory(&context, memory, method_id);
        let arg = AroraBuffer::read_wasm_memory(&context, memory, arg);

        let result = engine
            .dispatch(&module_id, &method_id, arg.as_ref())
            .unwrap();

        let result_addr = malloc.call(&mut context, (result.len() as u32,)).unwrap();

        memory
            .write(&mut context, result_addr as usize, &result)
            .unwrap();

        result_addr
    }

    fn arora_dispatch_indirect(mut caller: Caller<'_, HostState>, callable_id: u64) -> u32 {
        // SAFETY: re-entrant access to the engine; see `HostState::engine`.
        let arora_caller: &mut dyn CallBridge = unsafe { &mut *caller.data().engine.0 };
        let result_value = arora_caller
            .arora_call_indirect(&CallableId { id: callable_id })
            .unwrap();
        let result_buffer = serialize(&result_value);

        let malloc = caller.data().malloc.as_ref().unwrap().clone();

        let memory = caller
            .get_export("memory")
            .and_then(Extern::into_memory)
            .unwrap();

        let mut context = caller.as_context_mut();

        let result_addr = malloc
            .call(&mut context, (result_buffer.len() as u32,))
            .unwrap();

        memory
            .write(&mut context, result_addr as usize, &result_buffer)
            .unwrap();

        result_addr
    }

    pub fn new(
        exports: Vec<ExportSymbol>,
        module: WasmModule,
        mut store: Store<HostState>,
        mut linker: Linker<HostState>,
    ) -> Result<Self> {
        linker.func_wrap("env", "arora_dispatch", Self::arora_dispatch)?;
        linker.func_wrap("env", "arora_dispatch_indirect", Self::arora_dispatch_indirect)?;

        let instance = linker.instantiate(&mut store, &module)?;

        let arora_buffer_free =
            instance.get_typed_func::<(u32,), ()>(&mut store, "arora_buffer_free")?;

        let arora_buffer_alloc =
            instance.get_typed_func::<(u32,), u32>(&mut store, "arora_buffer_alloc")?;

        let mut arora_functions = HashMap::new();
        for export in exports {
            let arora_function = instance.get_typed_func::<(u32,), u32>(
                &mut store,
                &format!(
                    "arora_function_{}",
                    export.id().to_string().replace('-', "_")
                ),
            )?;
            arora_functions.insert(*export.id(), arora_function);
        }

        let memory = instance.get_memory(&mut store, "memory").unwrap();
        store.data_mut().malloc = Some(arora_buffer_alloc.clone());

        Ok(Self {
            module,
            store,
            malloc: arora_buffer_alloc,
            free: arora_buffer_free,
            arora_functions,
            memory,
            current_arg_memory: None,
        })
    }

    fn malloc(&mut self, size: u32) -> Result<u32, DispatchError> {
        self.malloc
            .call(&mut self.store, (size,))
            .map_err(|e| DispatchError::Trap {
                message: format!(
                    "failed to allocate memory for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })
    }

    fn free(&mut self, addr: u32) -> Result<(), DispatchError> {
        self.free
            .call(&mut self.store, (addr,))
            .map_err(|e| DispatchError::Trap {
                message: format!("failed to free memory for module {}: {:#?}", self.name(), e),
            })
    }

    fn allocate_arg_memory(&mut self, size: u32) -> Result<u32, DispatchError> {
        let ptr = self.malloc(size)?;
        self.current_arg_memory = Some((ptr, size));
        Ok(ptr)
    }

    /// Returns the module name if specified, else <unknown>.
    fn name(&self) -> &str {
        self.module.name().unwrap_or("<unknown>")
    }
}

impl Module for WebAssemblyModule {
    fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
        let arg_size = u32::try_from(arg.len()).map_err(|_| DispatchError::Internal {
            message: format!("failed to cast args size to u32 in module {}", self.name()),
        })?;

        // Let the WASM module allocate a buffer,
        // and copy the argument into it.
        let arg_addr = self.allocate_arg_memory(arg_size)?;
        self.memory
            .write(&mut self.store, arg_addr as usize, arg)
            .map_err(|e| DispatchError::Trap {
                message: format!(
                    "failed to write to memory for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;

        // Calling the function. It returns the address of the buffer of the result.
        let func = self.arora_functions.get(method_id).unwrap();
        let result = func
            .call(&mut self.store, (arg_addr,))
            .map_err(|e| DispatchError::Trap {
                message: format!("error calling {}.{}: {:#?}", self.name(), method_id, e),
            })?;

        // Free the buffer allocated for the argument.
        self.free(arg_addr)?;

        // Read the size of the result.
        let mut size_buffer = [0u8; 4];
        self.memory
            .read(&self.store, result as usize, &mut size_buffer)
            .map_err(|e| DispatchError::Internal {
                message: format!(
                    "failed to read the size of the result for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;
        let size = size_buffer.as_slice().get_u32_le();

        // Read the result into a local buffer.
        let mut result_buffer = vec![0u8; size as usize];
        self.memory
            .read(&self.store, result as usize, &mut result_buffer)
            .map_err(|e| DispatchError::Internal {
                message: format!(
                    "failed to read the result for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;

        // Free the result.
        self.free
            .call(&mut self.store, (result,))
            .map_err(|e| DispatchError::Trap {
                message: format!(
                    "failed to free the result for module {}: {:#?}",
                    self.name(),
                    e
                ),
            })?;

        // Check for a guest-side error (TYPE_ERROR at payload start).
        if result_buffer.get(BUFFER_SIZE_SIZE) == Some(&TYPE_ERROR) {
            let msg_start = BUFFER_SIZE_SIZE + 1;
            let message = if result_buffer.len() >= msg_start + 4 {
                let len =
                    u32::from_le_bytes(result_buffer[msg_start..msg_start + 4].try_into().unwrap())
                        as usize;
                let str_start = msg_start + 4;
                std::str::from_utf8(&result_buffer[str_start..str_start + len])
                    .unwrap_or("<invalid utf-8>")
                    .to_string()
            } else {
                "guest returned error (no message)".to_string()
            };
            return Err(DispatchError::Guest { message });
        }

        Ok(result_buffer.into_boxed_slice())
    }
}
