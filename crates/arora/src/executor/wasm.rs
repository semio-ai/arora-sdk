use std::collections::HashMap;

use arora_schema::module::low::{ModuleDefinition, Symbol};

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
  Module as WasmModule, Store, WasmParams,
};

#[derive(Debug, Error, Display, From)]
pub enum InitializationError {
  Internal(#[error(not(source))] anyhow::Error),
}

pub struct WebAssemblyExecutor {
  arora: WasmEngine,
}

impl WebAssemblyExecutor {
  pub fn new() -> Result<Self, InitializationError> {
    let mut config = Config::new();
    config.async_support(true);

    Ok(Self {
      arora: WasmEngine::new(&config)?,
    })
  }

  fn call(caller: Caller<()>, addr: u32) {}

  async fn load_module(
    &mut self,
    data: LoadModule,
    self_addr: &Addr<ExecutorMsg>,
  ) -> LoadModuleResult {
    let module_definition = data.module_definition;

    let module = WasmModule::from_binary(&self.arora, &module_definition.executable)
      .map_err(|e| LoadModuleError::MalformedExecutable)?;

    let mut store = Store::new(&self.arora, ());

    let mut externs = Vec::new();
    for import in module_definition.header.imports.iter() {
      match import {
        Symbol::Function(f) => {
          let func = Func::wrap(&mut store, Self::call);
          externs.push(Extern::Func(func));
        }
        Symbol::Node(n) => {}
      }
    }

    let instance = WasmInstance::new_async(&mut store, &module, &externs)
      .await
      .map_err(|_| LoadModuleError::Internal)?;

    Ok(
      WebAssemblyModule {
        executor: self_addr.clone(),
        module,
        store,
        instance,
      }
      .spawn(),
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
  store: Store<()>,
  instance: WasmInstance,
}

impl WebAssemblyModule {
  async fn dispatch(&mut self, data: Dispatch) -> DispatchResult {
    let func = self
      .instance
      .get_typed_func::<(), i32, _>(&mut self.store, "asd")
      .map_err(|_| DispatchError::MethodNotFound)?;
    let result = func
      .call_async(&mut self.store, ())
      .await
      .map_err(|_| DispatchError::Trap)?;

    println!("{:?}", result);
    Ok(())
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
