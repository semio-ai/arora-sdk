use derive_more::{Display, Error, From};
use uuid::Uuid;

#[derive(Display, Debug, From, Error)]
pub enum DispatchError {
  MethodNotFound,
  Trap,
  Internal,
}

pub trait Module {
  fn dispatch(&mut self, method_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError>;
}
