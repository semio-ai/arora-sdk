use derive_more::{Display, Error, From};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::actor::{Actor, Addr, Request};

#[derive(Display, Debug, From, Error)]
pub enum DispatchError {
  MethodNotFound,
  Trap,
  Internal,
}

pub type DispatchResult = Result<Box<[u8]>, DispatchError>;

#[derive(Debug)]
pub struct Dispatch {
  pub method_id: Uuid,
  pub arg: Box<[u8]>,
}

pub type DispatchRequest = Request<Dispatch, DispatchResult>;

#[derive(From)]
pub enum ModuleMsg {
  Dispatch(DispatchRequest),
}

pub trait Module: Send + Actor<Msg = ModuleMsg> {}

impl Addr<ModuleMsg> {
  pub async fn dispatch(&self, data: Dispatch) -> DispatchResult {
    let (tx, rx) = oneshot::channel();

    self
      .send(DispatchRequest::new(data, tx).into())
      .await
      .map_err(|_| DispatchError::Internal)?;

    rx.await.map_err(|_| DispatchError::Internal)?
  }
}
