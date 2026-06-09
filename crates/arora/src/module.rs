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
  Trap {
    message: String,
  },
  Internal {
    message: String,
  },
  /// The guest returned a TYPE_ERROR buffer instead of a result.
  Guest {
    message: String,
  },
}

pub trait Module {
  fn dispatch(&mut self, function_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError>;
}
