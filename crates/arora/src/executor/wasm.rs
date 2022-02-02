use std::{collections::HashMap, future::Future, pin::Pin, cell::RefCell, rc::Rc};

use arora_schema::module::low::{ModuleDefinition, ImportSymbol, ExportSymbol};
use uuid::Uuid;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};
use bytes::{Buf, BufMut};

use crate::{
  module::{Module, DispatchError}, engine::{Engine, EngineRef},
};

use super::{Executor, LoadModuleError,UnloadModuleError,};
use derive_more::{Display, Error, From};

use tokio::sync::mpsc;
use wasmtime::{
  Caller, Config, Engine as WasmEngine, Extern, Func, FuncType, Instance as WasmInstance,
  Module as WasmModule, Store, WasmParams, Linker, TypedFunc, Memory, InstanceLimits, ModuleLimits, LinearMemory, AsContext, 
  AsContextMut, StoreContextMut
};

#[derive(Debug, Error, Display, From)]
pub enum InitializationError {
  Internal(#[error(not(source))] anyhow::Error),
}

pub struct WebAssemblyExecutor {
  engine: WasmEngine,
  arora_engine: Option<EngineRef>,
}

impl WebAssemblyExecutor {
  pub fn new() -> Result<Self, InitializationError> {
    let mut config = Config::new();
    // config.async_support(true);
    config.cranelift_opt_level(wasmtime::OptLevel::Speed);
    config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling {
      instance_limits: InstanceLimits {
        ..Default::default()
      },
      module_limits: ModuleLimits {
        ..Default::default()
      },
      strategy: wasmtime::PoolingAllocationStrategy::NextAvailable
    });
    // config.profiler(wasmtime::ProfilingStrategy::VTune).unwrap();

    Ok(Self {
      engine: WasmEngine::new(&config)?,
      arora_engine: None
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

  fn load_module(&mut self, module_definition: ModuleDefinition) -> Result<Box<dyn Module>, LoadModuleError> {

    let module = WasmModule::from_binary(&self.engine, &module_definition.executable)
      .map_err(|e| LoadModuleError::MalformedExecutable)?;

    let ctx = WasiCtxBuilder::new()
      .inherit_stdio()
      .build();

    let mut store = Store::new(&self.engine, ctx);

    
    let mut linker = Linker::new(&self.engine);
    wasmtime_wasi::add_to_linker(&mut linker, |s| s)
      .map_err(|e| LoadModuleError::Internal)?;
    
    


    

    Ok(
      Box::new(WebAssemblyModule::new(
        self.arora_engine.unwrap(),
        module_definition.header.exports,
        module,
        store,
        linker
      ).unwrap()),
    )
  }

  fn unload_module(&mut self, module_id: Uuid) -> Result<(), UnloadModuleError> {
    Ok(())
  }
}

struct WebAssemblyModule {
  engine: EngineRef,
  module: WasmModule,
  store: Store<WasiCtx>,
  instance: WasmInstance,

  malloc: TypedFunc<(u32,), u32>,
  free: TypedFunc<(u32,), ()>,
  arora_buffer_free: TypedFunc<(u32,), ()>,
  arora_functions: HashMap<Uuid, TypedFunc<(u32,), u32>>,
  memory: Memory,

  current_arg_memory: Option<(usize, usize)>,
}

impl WebAssemblyModule {
  fn arora_dispatch(engine: usize, mut caller: Caller<'_, WasiCtx>, module_id: u32, method_id: u32, arg: u32) -> u32 {
    println!("arora_dispatch: module_id: {}, method_id: {}, arg: {}", module_id, method_id, arg);

    // yuck yuck yuck
    // All of this shouldn't necessary. We should fix it.
    let engine = unsafe { &mut *(engine as *mut Engine) };
    let caller2 = unsafe { &mut *(&mut caller as *mut Caller<'_, WasiCtx>) };
    let caller3 = unsafe { &mut *(&mut caller as *mut Caller<'_, WasiCtx>) };

    let context = caller.as_context_mut();

    let memory = caller2.data_mut().table().get_mut::<Memory>(8374).unwrap();
    let malloc = caller3.data_mut().table().get_mut::<TypedFunc<(u32,), u32>>(8375).unwrap();
    
    // Extract module_uuid
    let mut module_uuid = [0u8; 16];
    memory.read(&caller.as_context(), module_id as usize, &mut module_uuid).unwrap();
    let module_uuid = Uuid::from_slice(&module_uuid).unwrap();

    // Extract method_uuid
    let mut method_uuid = [0u8; 16];
    memory.read(&caller.as_context(), method_id as usize, &mut method_uuid).unwrap();
    let method_uuid = Uuid::from_slice(&method_uuid).unwrap();

    let mut arg_size_buffer = [0u8; 4];
    memory.read(&caller.as_context(), arg as usize, &mut arg_size_buffer).unwrap();

    let arg_size = arg_size_buffer.as_slice().get_u32_le();

    let mut arg_buffer = Vec::with_capacity(arg_size as usize + 4);

    arg_buffer.resize(arg_size as usize + 4, 0u8);
    memory.read(&caller.as_context(), arg as usize, &mut arg_buffer).unwrap();

    let result = engine.dispatch(&module_uuid, &method_uuid, arg_buffer.as_slice()).unwrap();

    let result_addr = malloc.call(&mut caller.as_context_mut(), (result.len() as u32,)).unwrap();

    memory.write(&mut caller.as_context_mut(), result_addr as usize, &result).unwrap();

    result_addr
  }

  pub fn new(engine: EngineRef, exports: Vec<ExportSymbol>, module: WasmModule, mut store: Store<WasiCtx>, mut linker: Linker<WasiCtx>) -> Result<Self, wasmtime_wasi::Error> {
    let arora_dispatch_engine = engine as usize;
    linker.func_wrap(
      "env",
      "arora_dispatch",
      move |caller: Caller<'_, WasiCtx>, module_id, method_id, arg|
        WebAssemblyModule::arora_dispatch(arora_dispatch_engine, caller, module_id, method_id, arg)
    )?;
    
    let instance = linker.instantiate(&mut store, &module)?;
    
    let malloc = instance.get_typed_func::<(u32,), u32, _>(&mut store, "malloc")?;
    let free = instance.get_typed_func::<(u32,), (), _>(&mut store, "free")?;
    let arora_buffer_free = instance.get_typed_func::<(u32,), (), _>(&mut store, "arora_buffer_free")?;

    let mut arora_functions = HashMap::new();
    for export in exports {
        let arora_function = instance.get_typed_func::<(u32,), u32, _>(&mut store, &format!("arora_function_{}", export.id().to_string().replace('-', "_")))?;
        arora_functions.insert(export.id().clone(), arora_function);
    }

    let memory = instance.get_memory(&mut store, "memory").unwrap();
    store.data_mut().table().insert_at(8374, Box::new(memory));
    store.data_mut().table().insert_at(8375, Box::new(malloc));



    Ok(Self {
      engine,
      module,
      store,
      instance,
      malloc,
      free,
      arora_buffer_free,
      arora_functions,
      memory,
      current_arg_memory: None,
    })
  }

  fn malloc(&mut self, size: u32) -> Result<u32, DispatchError> {
    Ok(self.malloc.call(&mut self.store, (size,))
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Trap
      })?
    )
  }
}

impl Module for WebAssemblyModule {
  fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {

    if let Some((addr, size)) = self.current_arg_memory {
      // Allocate memory for the argument in the WASM module
      if size < arg.len() {
        self.current_arg_memory = Some((
          self.malloc(arg.len() as u32)? as usize,
          arg.len(),
        ));
      }
    } else {
      self.current_arg_memory = Some((
        self.malloc(arg.len() as u32)? as usize,
        arg.len(),
      ));
    }
    
    let (addr, _) = self.current_arg_memory.unwrap();

    // Copy the argument into the WASM module
    self.memory.write(&mut self.store, addr as usize, &arg)
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Trap
      })?;


    let func = self.arora_functions.get(method_id).unwrap();
    
    let result = func
      .call(&mut self.store, (addr as u32,))
      .map_err(|e| {
        println!("call {:#?}", e);
        DispatchError::Trap
      })?;

    let mut size_buffer = [0u8; 4];
    self.memory.read(&self.store, result as usize, &mut size_buffer)
      .map_err(|e| {
        println!("{:#?}", e);
        DispatchError::Internal
      })?;

    let size = size_buffer.as_slice().get_u32_le();

    let mut result_buffer = Vec::with_capacity(size as usize + 4);

    result_buffer.resize(size as usize + 4, 0u8);
    self.memory.read(&self.store, result as usize, &mut result_buffer)
      .map_err(|e| {
        println!("read {:#?}", e);
        DispatchError::Internal
      })?;

    // Free the result
    self.arora_buffer_free.call(&mut self.store, (result,))
      .map_err(|e| {
        println!("arora_buffer_free {:#?}", e);
        DispatchError::Trap
      })?;

    Ok(result_buffer.into_boxed_slice())
  }
}

