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

pub trait Module {
  fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError>;
}