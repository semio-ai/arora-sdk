use std::{collections::HashMap, future::Future, pin::Pin};

use arora_schema::module::low::{ModuleDefinition, ImportSymbol, ExportSymbol};
use uuid::Uuid;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};
use bytes::{Buf, BufMut};

use crate::{
  module::{Module, DispatchError},
};

use super::{Executor, LoadModuleError,UnloadModuleError,};
use derive_more::{Display, Error, From};

use tokio::sync::mpsc;
use wasmtime::{
  Caller, Config, Engine as WasmEngine, Extern, Func, FuncType, Instance as WasmInstance,
  Module as WasmModule, Store, WasmParams, Linker, TypedFunc, Memory, InstanceLimits, ModuleLimits, 
};

#[derive(Debug, Error, Display, From)]
pub enum InitializationError {
  Internal(#[error(not(source))] anyhow::Error),
}

pub struct WebAssemblyExecutor {
  engine: WasmEngine,
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
    })
  }

  fn call<'a>(caller: Caller<'a, WasiCtx>, module_id: Uuid, method_id: Uuid, addr: u32, length: u32) -> Box<dyn Future<Output = ()> + Send + 'a> {
    Box::new(async move {

    })
  }
}

impl Executor for WebAssemblyExecutor {
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
    
    
    for import in module_definition.header.imports.iter() {
      match import {
        ImportSymbol::Function(f) => {
          let module_id = f.module.clone();
          let method_id = f.id.clone();
          linker.func_wrap2_async(&"", &f.id.to_string().replace('-', "_"), 
          move |caller, args, length| Self::call(caller, module_id.clone(), method_id.clone(), args, length))
            .map_err(|_| LoadModuleError::Internal)?;
          
        }
      }
    }


    let instance = linker.instantiate(&mut store, &module)
      .map_err(|e| {
        println!("{:?}", e);
        LoadModuleError::Internal
      })?;

    Ok(
      Box::new(WebAssemblyModule::new(
        module_definition.header.exports,
        module,
        store,
        instance
      ).map_err(|_| LoadModuleError::Internal)?),
    )
  }

  fn unload_module(&mut self, module_id: Uuid) -> Result<(), UnloadModuleError> {
    Ok(())
  }
}

struct WebAssemblyModule {
  module: WasmModule,
  store: Store<WasiCtx>,
  instance: WasmInstance,

  malloc: TypedFunc<(u32,), u32>,
  free: TypedFunc<(u32,), ()>,
  arora_functions: HashMap<Uuid, TypedFunc<(u32, u32), u32>>,
  memory: Memory,

  current_arg_memory: Option<(usize, usize)>,
}

impl WebAssemblyModule {
  pub fn new(exports: Vec<ExportSymbol>, module: WasmModule, mut store: Store<WasiCtx>, instance: WasmInstance) -> Result<Self, wasmtime_wasi::Error> {
    let malloc = instance.get_typed_func::<(u32,), u32, _>(&mut store, "malloc")?;
    let free = instance.get_typed_func::<(u32,), (), _>(&mut store, "free")?;

    let mut arora_functions = HashMap::new();
    for export in exports {
        let arora_function = instance.get_typed_func::<(u32, u32), u32, _>(&mut store, &format!("arora_function_{}", export.id().to_string().replace('-', "_")))?;
        arora_functions.insert(export.id().clone(), arora_function);
    }

    let memory = instance.get_memory(&mut store, "memory").unwrap();

    Ok(Self {
      module,
      store,
      instance,
      malloc,
      free,
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
      .call(&mut self.store, (addr as u32, arg.len() as u32))
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Trap
      })?;

    let mut size_buffer = [0u8; 4];
    self.memory.read(&self.store, result as usize, &mut size_buffer)
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Internal
      })?;

    let size = size_buffer.as_slice().get_u32();

    let mut result_buffer = Vec::with_capacity(size as usize + 4);

    result_buffer.resize(size as usize + 4, 0u8);
    self.memory.read(&self.store, result as usize, &mut result_buffer)
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Internal
      })?;

    // Free the result
    self.free.call(&mut self.store, (result,))
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Trap
      })?;

    Ok(result_buffer.into_boxed_slice())
  }
}

