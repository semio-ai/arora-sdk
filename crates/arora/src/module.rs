use derive_more::{Display, Error};
use uuid::Uuid;

#[derive(Display, Debug, Error)]
pub enum DispatchError {
  ModuleNotFound {
    id: Uuid,
  },
  FunctionNotFound {
    id: Uuid,
  },
  Trap,
  Internal,
}

pub trait Module {
  fn dispatch(&mut self, function_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError>;
}
