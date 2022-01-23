use std::collections::HashMap;

use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::{
  actor::{Actor, Addr, Request},
  executor::{self, Executor, ExecutorMsg},
  module::ModuleMsg,
  schema::module::low::ModuleDefinition,
};

use derive_more::{Display, Error, From};

pub struct EngineBuilder {
  executors: HashMap<&'static str, Addr<ExecutorMsg>>,
}

impl EngineBuilder {
  pub fn new() -> Self {
    Self {
      executors: HashMap::new(),
    }
  }

  pub fn add_executor<E: Executor + 'static>(mut self, executor: E) -> Self {
    self.executors.insert(executor.name(), executor.spawn());
    self
  }

  pub fn build(self) -> Engine {
    Engine {
      executors: self.executors,
      modules: HashMap::new(),
    }
  }
}

#[derive(Debug, Display, Error, From, Clone)]
pub enum LoadModuleError {
  ExecutorNotFound,
  MalformedExecutable,
  Internal,
}

impl From<executor::LoadModuleError> for LoadModuleError {
  fn from(e: executor::LoadModuleError) -> Self {
    match e {
      executor::LoadModuleError::MalformedExecutable => LoadModuleError::MalformedExecutable,
      executor::LoadModuleError::Internal => LoadModuleError::Internal,
    }
  }
}

pub type LoadModuleResult = Result<Addr<ModuleMsg>, LoadModuleError>;

pub struct LoadModule {
  module_definition: ModuleDefinition,
}

pub type LoadModuleRequest = Request<LoadModule, LoadModuleResult>;

#[derive(Debug, Display, Error, From)]
pub enum UnloadModuleError {
  ModuleNotFound,
  Internal,
}

impl From<executor::UnloadModuleError> for UnloadModuleError {
  fn from(e: executor::UnloadModuleError) -> Self {
    match e {
      executor::UnloadModuleError::ModuleNotFound => UnloadModuleError::ModuleNotFound,
      executor::UnloadModuleError::Internal => UnloadModuleError::Internal,
    }
  }
}

pub type UnloadModuleResult = Result<(), UnloadModuleError>;

pub struct UnloadModule {
  module_id: Uuid,
}

pub type UnloadModuleRequest = Request<UnloadModule, UnloadModuleResult>;

pub type LookupResult = Option<Addr<ModuleMsg>>;

pub struct Lookup {
  module_id: Uuid,
}

pub type LookupRequest = Request<Lookup, LookupResult>;

#[derive(From)]
pub enum EngineMsg {
  LoadModule(LoadModuleRequest),
  UnloadModule(UnloadModuleRequest),
  Lookup(LookupRequest),
}

#[derive(From)]
pub enum ModuleState {
  Loaded(Addr<ModuleMsg>),
  Loading(broadcast::Sender<LoadModuleResult>),
}

pub struct Engine {
  executors: HashMap<&'static str, Addr<ExecutorMsg>>,
  modules: HashMap<Uuid, ModuleState>,
}

impl Engine {
  async fn load_module(&mut self, data: LoadModule) -> LoadModuleResult {
    let module_definition = data.module_definition;
    let module_id = module_definition.header.id;
    let module_name = module_definition.header.executor.name.as_str();

    if let Some(module_state) = self.modules.get(&module_id) {
      return match module_state {
        // We've already loaded the module.
        ModuleState::Loaded(module) => Ok(module.clone()),

        // We're loading the module.
        ModuleState::Loading(tx) => {
          // Wait for the module to load.
          tx.subscribe()
            .recv()
            .await
            .map_err(|_| LoadModuleError::Internal)?
        }
      };
    }

    // We haven't loaded the module yet.

    // Find the executor for the module.
    let (name, executor) = self
      .executors
      .get_key_value(module_name)
      .ok_or_else(|| LoadModuleError::ExecutorNotFound)?;

    let (tx, mut rx) = broadcast::channel(1);
    self
      .modules
      .insert(module_id, ModuleState::Loading(tx.clone()));

    let executor = executor.clone();
    tokio::spawn(async move {
      tx.send(
        executor
          .load_module(module_definition)
          .await
          .map_err(|e| e.into()),
      );
    });

    let module = rx.recv().await.map_err(|e| match e {
      _ => LoadModuleError::Internal,
    })??;

    self
      .modules
      .insert(module_id, ModuleState::Loaded(module.clone()));
    Ok(module)
  }

  async fn lookup(&mut self, data: Lookup) -> LookupResult {
    let module_id = data.module_id;
    let module_state = self.modules.get(&module_id)?;

    match module_state {
      ModuleState::Loaded(module) => Some(module.clone()),
      ModuleState::Loading(tx) => {
        let mut rx = tx.subscribe();
        let recv_result = rx.recv().await;
        let load_result = recv_result.ok()?;
        Some(load_result.ok()?)
      }
      _ => None,
    }
  }

  async fn run(mut self, mut rx: mpsc::Receiver<EngineMsg>) {
    while let Some(msg) = rx.recv().await {
      match msg {
        EngineMsg::LoadModule(request) => {
          let (data, reply) = request.split();
          let _ = reply.send(self.load_module(data).await);
        }
        EngineMsg::UnloadModule(request) => {
          // request.reply.send(self.unload_module(request).await).unwrap();
        }
        EngineMsg::Lookup(request) => {
          let (data, reply) = request.split();
          let _ = reply.send(self.lookup(data).await);
        }
      }
    }
  }
}

impl Actor for Engine {
  type Msg = EngineMsg;

  fn spawn(mut self) -> Addr<Self::Msg> {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(Self::run(self, rx));
    Addr::new(tx)
  }
}

impl Addr<EngineMsg> {
  pub async fn load_module(&self, module_definition: ModuleDefinition) -> LoadModuleResult {
    let (tx, rx) = oneshot::channel();

    self
      .send(LoadModuleRequest::new(LoadModule { module_definition }, tx).into())
      .await
      .map_err(|_| LoadModuleError::Internal)?;

    rx.await.map_err(|_| LoadModuleError::Internal)?
  }

  pub async fn unload_module(&self, module_id: Uuid) -> UnloadModuleResult {
    let (tx, rx) = oneshot::channel();

    self
      .send(EngineMsg::UnloadModule(
        UnloadModuleRequest::new(UnloadModule { module_id }, tx).into(),
      ))
      .await
      .map_err(|_| UnloadModuleError::Internal)?;

    rx.await.map_err(|_| UnloadModuleError::Internal)?
  }

  pub async fn lookup(&self, module_id: Uuid) -> LookupResult {
    let (tx, rx) = oneshot::channel();

    self
      .send(EngineMsg::Lookup(
        LookupRequest::new(Lookup { module_id }, tx).into(),
      ))
      .await
      .map_err(|_| UnloadModuleError::Internal)
      .ok()?;

    rx.await.ok()?
  }
}
