use std::{collections::HashMap, future::Future, pin::Pin};

use arora_schema::module::low::{ModuleDefinition, ImportSymbol, ExportSymbol};
use uuid::Uuid;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};
use bytes::{Buf, BufMut};

use crate::{
  actor::{Actor, Addr},
  module::{Dispatch, DispatchError, DispatchResult, ModuleMsg},
};

use super::{
  Executor, ExecutorMsg, LoadModule, LoadModuleError, LoadModuleResult, UnloadModule,
  UnloadModuleError,
};
use derive_more::{Display, Error, From};

use tokio::sync::mpsc;
use wasmtime::{
  Caller, Config, Engine as WasmEngine, Extern, Func, FuncType, Instance as WasmInstance,
  Module as WasmModule, Store, WasmParams, Linker, TypedFunc, Memory, 
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
    config.async_support(true);
    config.cranelift_opt_level(wasmtime::OptLevel::Speed);
    config.allocation_strategy(wasmtime::InstanceAllocationStrategy::OnDemand);

    Ok(Self {
      engine: WasmEngine::new(&config)?,
    })
  }

  fn call<'a>(caller: Caller<'a, WasiCtx>, module_id: Uuid, method_id: Uuid, addr: u32, length: u32) -> Box<dyn Future<Output = ()> + Send + 'a> {
    Box::new(async move {

    })
  }

  async fn load_module(
    &mut self,
    data: LoadModule,
    self_addr: &Addr<ExecutorMsg>,
  ) -> LoadModuleResult {
    let module_definition = data.module_definition;

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
        ImportSymbol::Node(n) => {}
      }
    }


    let instance = linker.instantiate_async(&mut store, &module).await
      .map_err(|e| {
        println!("{:?}", e);
        LoadModuleError::Internal
      })?;

    Ok(
      WebAssemblyModule::new(
        module_definition.header.exports,
        self_addr.clone(),
        module,
        store,
        instance
      ).map_err(|_| LoadModuleError::Internal)?.spawn(),
    )
  }

  async fn unload_module(&mut self, data: UnloadModule) -> Result<(), UnloadModuleError> {
    Ok(())
  }

  async fn run(mut self, mut rx: mpsc::Receiver<ExecutorMsg>, self_addr: Addr<ExecutorMsg>) {
    while let Some(msg) = rx.recv().await {
      match msg {
        ExecutorMsg::LoadModule(request) => {
          let (data, reply) = request.split();
          let _ = reply.send(self.load_module(data, &self_addr).await);
        }
        ExecutorMsg::UnloadModule(request) => {
          let (data, reply) = request.split();
          let _ = reply.send(self.unload_module(data).await);
        }
      }
    }
  }
}

impl Executor for WebAssemblyExecutor {
  fn name(&self) -> &'static str {
    "wasm"
  }
}

impl Actor for WebAssemblyExecutor {
  type Msg = ExecutorMsg;

  fn spawn(self) -> Addr<Self::Msg> {
    let (tx, rx) = mpsc::channel(100);
    let addr = Addr::new(tx);
    tokio::spawn(Self::run(self, rx, addr.clone()));
    addr
  }
}

struct WebAssemblyModule {
  executor: Addr<ExecutorMsg>,
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
  pub fn new(exports: Vec<ExportSymbol>, executor: Addr<ExecutorMsg>, module: WasmModule, mut store: Store<WasiCtx>, instance: WasmInstance) -> Result<Self, wasmtime_wasi::Error> {
    let malloc = instance.get_typed_func::<(u32,), u32, _>(&mut store, "malloc")?;
    let free = instance.get_typed_func::<(u32,), (), _>(&mut store, "free")?;

    let mut arora_functions = HashMap::new();
    for export in exports {
        let arora_function = instance.get_typed_func::<(u32, u32), u32, _>(&mut store, &format!("arora_function_{}", export.id().to_string().replace('-', "_")))?;
        arora_functions.insert(export.id().clone(), arora_function);
    }

    let memory = instance.get_memory(&mut store, "memory").unwrap();

    Ok(Self {
      executor,
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

  async fn malloc(&mut self, size: u32) -> Result<u32, DispatchError> {
    Ok(self.malloc.call_async(&mut self.store, (size,)).await
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Trap
      })?
    )
  }
  
  async fn dispatch(&mut self, data: Dispatch) -> DispatchResult {

    if let Some((addr, size)) = self.current_arg_memory {
      // Allocate memory for the argument in the WASM module
      if size < data.arg.len() {
        self.current_arg_memory = Some((
          self.malloc(data.arg.len() as u32).await? as usize,
          data.arg.len(),
        ));
      }
    } else {
      self.current_arg_memory = Some((
        self.malloc(data.arg.len() as u32).await? as usize,
        data.arg.len(),
      ));
    }
    
    let (addr, _) = self.current_arg_memory.unwrap();

    // Copy the argument into the WASM module
    self.memory.write(&mut self.store, addr as usize, &data.arg)
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Trap
      })?;


    let func = self.arora_functions.get(&data.method_id).unwrap();
    
    let result = func
      .call_async(&mut self.store, (addr as u32, data.arg.len() as u32))
      .await
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
    self.free.call_async(&mut self.store, (result,)).await
      .map_err(|e| {
        println!("{:?}", e);
        DispatchError::Trap
      })?;

    Ok(result_buffer.into_boxed_slice())
  }

  async fn run(mut self, mut rx: mpsc::Receiver<ModuleMsg>) {
    while let Some(msg) = rx.recv().await {
      match msg {
        ModuleMsg::Dispatch(request) => {
          let (data, reply) = request.split();
          let _ = reply.send(self.dispatch(data).await);
        }
      }
    }
  }
}

impl Actor for WebAssemblyModule {
  type Msg = ModuleMsg;

  fn spawn(self) -> Addr<Self::Msg> {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(Self::run(self, rx));
    Addr::new(tx)
  }
}
