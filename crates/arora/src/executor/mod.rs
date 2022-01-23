pub mod wasm;

use crate::{
  actor::{Actor, Addr, Request},
  module::ModuleMsg,
  schema::module::low::ModuleDefinition,
};
use derive_more::{Display, Error, From};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Display, Error, From, Clone)]
pub enum LoadModuleError {
  MalformedExecutable,
  Internal,
}

#[derive(Debug, Display, Error, From)]
pub enum UnloadModuleError {
  ModuleNotFound,
  Internal,
}

pub type LoadModuleResult = Result<Addr<ModuleMsg>, LoadModuleError>;

pub struct LoadModule {
  pub module_definition: ModuleDefinition,
}

pub type LoadModuleRequest = Request<LoadModule, LoadModuleResult>;

pub type UnloadModuleResult = Result<(), UnloadModuleError>;

pub struct UnloadModule {
  pub module_id: Uuid,
}

pub type UnloadModuleRequest = Request<UnloadModule, UnloadModuleResult>;

#[derive(From)]
pub enum ExecutorMsg {
  LoadModule(LoadModuleRequest),
  UnloadModule(UnloadModuleRequest),
}

pub trait Executor: Send + Actor<Msg = ExecutorMsg> {
  fn name(&self) -> &'static str;
}

/// Convenience methods for constructing and sending messages
impl Addr<ExecutorMsg> {
  pub async fn load_module(&self, module_definition: ModuleDefinition) -> LoadModuleResult {
    let (tx, rx) = oneshot::channel();

    self
      .send(LoadModuleRequest::new(LoadModule { module_definition }, tx).into())
      .await
      .map_err(|_| LoadModuleError::Internal)?;

    rx.await.unwrap()
  }

  pub async fn unload_module(&self, module_id: Uuid) -> UnloadModuleResult {
    let (tx, rx) = oneshot::channel();

    self
      .send(UnloadModuleRequest::new(UnloadModule { module_id }, tx).into())
      .await
      .map_err(|_| UnloadModuleError::Internal)?;

    rx.await.unwrap()
  }
}
