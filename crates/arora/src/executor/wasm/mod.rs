use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use bytes::Buf;
use derive_more::{Display, Error, From};
use uuid::Uuid;
use wasmtime::component::Resource;
use wasmtime::{
  AsContextMut, Caller, Config, Engine as WasmEngine, Extern, Linker, Memory, Module as WasmModule,
  Store, TypedFunc,
};
use wasmtime_wasi::IoView;
use wasmtime_wasi::{preview1::WasiP1Ctx, WasiCtxBuilder};

use arora_buffers::serde_uuid::serialize;
use arora_buffers::{BUFFER_SIZE_SIZE, TYPE_ERROR};
use arora_types::module::low::{ExportSymbol, ModuleDefinition};

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::call::{CallBridge, CallableId};
use crate::{
  engine::{Engine, EngineRef},
  executor::wasm::guest::AroraBuffer,
  module::{DispatchError, Module},
};

mod guest;
use guest::ReadWasmMemory;

#[derive(Debug, Error, Display, From)]
pub enum InitializationError {
  Internal(#[error(not(source))] anyhow::Error),
}

/// Shared handle to the guest's `malloc` export, resolved lazily on first use.
type MallocResourceRc = Arc<RwLock<Option<Resource<TypedFunc<(u32,), u32>>>>>;

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
      engine: WasmEngine::new(&config)?,
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
    let ctx = WasiCtxBuilder::new().inherit_stdio().build_p1();

    let store = Store::new(&self.engine, ctx);

    let mut linker = Linker::new(&self.engine);
    wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |s| s)
      .map_err(|err| LoadModuleError::Internal(format!("failed to map to wasm linker: {}", err)))?;

    Ok(Box::new(
      WebAssemblyModule::new(
        self.arora_engine.unwrap(),
        module_definition.header.exports,
        module,
        store,
        linker,
      )
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

  store: Store<WasiP1Ctx>,
}

impl WebAssemblyModule {
  fn arora_dispatch(
    engine: usize,
    malloc_resource_rc: &MallocResourceRc,
    mut caller: Caller<'_, WasiP1Ctx>,
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

    // yuck yuck yuck
    // All of this shouldn't necessary. We should fix it.
    let engine = unsafe { &mut *(engine as *mut Engine) };

    let malloc_read_guard = malloc_resource_rc.try_read().unwrap();
    let malloc_resource = malloc_read_guard.as_ref().unwrap();
    let malloc = caller
      .data_mut()
      .table()
      .get_mut::<TypedFunc<(u32,), u32>>(malloc_resource)
      .unwrap()
      .clone();

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

  fn arora_dispatch_indirect(
    engine: usize,
    malloc_resource_rc: &MallocResourceRc,
    mut caller: Caller<'_, WasiP1Ctx>,
    callable_id: u64,
  ) -> u32 {
    // more yucks
    let arora_caller: &mut dyn CallBridge = unsafe { &mut *(engine as *mut Engine) };
    let result_value = arora_caller
      .arora_call_indirect(&CallableId { id: callable_id })
      .unwrap();
    let result_buffer = serialize(&result_value);

    let malloc_read_guard = malloc_resource_rc.try_read().unwrap();
    let malloc_resource = malloc_read_guard.as_ref().unwrap();
    let malloc = caller
      .data_mut()
      .table()
      .get_mut::<TypedFunc<(u32,), u32>>(malloc_resource)
      .unwrap()
      .clone();

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
    engine: EngineRef,
    exports: Vec<ExportSymbol>,
    module: WasmModule,
    mut store: Store<WasiP1Ctx>,
    mut linker: Linker<WasiP1Ctx>,
  ) -> Result<Self> {
    // Convert the pointer into a usize to ensure it passes through the WASM ABI.
    // TODO: do we really need to do this?
    let arora_engine_addr = engine as usize;

    let malloc_resource_rc = Arc::new(RwLock::new(None));

    let malloc_resource_rc_clone = malloc_resource_rc.clone();
    linker.func_wrap(
      "env",
      "arora_dispatch",
      move |caller: Caller<'_, WasiP1Ctx>, module_id, method_id, arg| {
        WebAssemblyModule::arora_dispatch(
          arora_engine_addr,
          &malloc_resource_rc_clone,
          caller,
          module_id,
          method_id,
          arg,
        )
      },
    )?;

    let malloc_resource_rc_clone = malloc_resource_rc.clone();
    linker.func_wrap(
      "env",
      "arora_dispatch_indirect",
      move |caller: Caller<'_, WasiP1Ctx>, callable_id: u64| {
        WebAssemblyModule::arora_dispatch_indirect(
          arora_engine_addr,
          &malloc_resource_rc_clone,
          caller,
          callable_id,
        )
      },
    )?;

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
    let malloc_resource = store.data_mut().table().push(arora_buffer_alloc.clone())?;
    *malloc_resource_rc.try_write().unwrap() = Some(malloc_resource);

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
    self
      .malloc
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
    self
      .free
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
    self
      .memory
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
    self
      .memory
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
    self
      .memory
      .read(&self.store, result as usize, &mut result_buffer)
      .map_err(|e| DispatchError::Internal {
        message: format!(
          "failed to read the result for module {}: {:#?}",
          self.name(),
          e
        ),
      })?;

    // Free the result.
    self
      .free
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
          u32::from_le_bytes(result_buffer[msg_start..msg_start + 4].try_into().unwrap()) as usize;
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
