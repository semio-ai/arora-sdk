use std::{collections::HashMap, rc::Rc};

use derive_more::Display;
use rand::{rng, Rng};
use uuid::Uuid;

use arora_buffers::serde_uuid::serialize;
pub use arora_types::call::{Call, CallResult};
use arora_types::value::{Structure, Value};

use crate::module::DispatchError;

pub fn serialize_to_arg(call: Call) -> Box<[u8]> {
  return serialize(&Value::Structure(Structure {
    id: call.id,
    fields: call.args,
  }));
}

pub trait Callable {
  fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError>;
}

pub trait CallBridge {
  /// Calls the given function, with the arguments provided via `call`.
  fn arora_call(&mut self, module: &Uuid, call: Call) -> Result<CallResult, CallError>;

  /// Registers the given function in the executor and
  /// associates it to an identified generated on the fly.
  /// The function is made available to every module by calling
  /// `arora_dispatch_indirect(id: u64) -> Value`.
  fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId;

  /// Unregisters the function associated to the given identifier.
  fn arora_unregister_callable(&mut self, callable_id: &CallableId);

  /// Calls a callable that was registered.
  fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError>;
}

#[derive(Display, Debug)]
pub enum CallError {
  Generic { message: String },
  ModuleNotFound { id: Uuid },
  FunctionNotFound { id: Uuid },
  Trap { message: String },
  Internal { message: String },
  /// The guest returned a structured error via TYPE_ERROR instead of trapping.
  Guest { message: String },
}

impl From<DispatchError> for CallError {
  fn from(e: DispatchError) -> Self {
    match e {
      DispatchError::ModuleNotFound { id } => CallError::ModuleNotFound { id },
      DispatchError::FunctionNotFound { id } => CallError::FunctionNotFound { id },
      DispatchError::Trap { message } => CallError::Trap { message },
      DispatchError::Internal { message } => CallError::Internal { message },
      DispatchError::Guest { message } => CallError::Guest { message },
    }
  }
}

impl std::error::Error for CallError {}

lazy_static::lazy_static! {
  pub static ref CALLABLE_ID_TYPE_ID: Uuid = Uuid::parse_str("6dd7f535-8245-4bf2-b081-81fd4636fa90").unwrap();
  pub static ref CALLABLE_ID_ID_FIELD_ID: Uuid = Uuid::parse_str("d799be14-e12a-4539-ac56-f7dc4634161b").unwrap();
}

#[derive(Clone, Hash, Eq, PartialEq)]
pub struct CallableId {
  pub id: u64,
}

impl From<u64> for CallableId {
  fn from(id: u64) -> Self {
    Self { id }
  }
}

impl Callable for CallableId {
  fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError> {
    caller.arora_call_indirect(self)
  }
}

pub struct CallableRegistry {
  callables_by_id: HashMap<CallableId, Rc<dyn Callable>>,
}

impl CallableRegistry {
  pub fn new() -> Self {
    Self {
      callables_by_id: HashMap::new(),
    }
  }

  pub fn register_callable(&mut self, callable: Rc<dyn Callable>) -> Result<CallableId, CallError> {
    let tick_id: CallableId = self.generate_unique_callable_id();
    if let Some(existing_one) = self.callables_by_id.insert(tick_id.clone(), callable) {
      self.callables_by_id.insert(tick_id.clone(), existing_one);
      return Err(CallError::Generic {
        message: "another callable is already associated to the id".to_string(),
      });
    }
    Ok(tick_id)
  }

  pub fn find_callable(&self, id: &CallableId) -> Result<Rc<dyn Callable>, CallError> {
    let callable = self.callables_by_id.get(id).ok_or(CallError::Generic {
      message: "cannot find callable".to_string(),
    })?;
    Ok(callable.clone())
  }

  pub fn unregister_callable(&mut self, id: &CallableId) -> Result<(), CallError> {
    self.callables_by_id.remove(id).ok_or(CallError::Generic {
      message: "cannot find callable".to_string(),
    })?;
    Ok(())
  }

  fn generate_unique_callable_id(&self) -> CallableId {
    let mut rng = rng();
    let mut tick_id = CallableId {
      id: rng.random::<u64>(),
    };
    while self.callables_by_id.contains_key(&tick_id) {
      tick_id.id = rng.random::<u64>();
    }
    tick_id
  }
}
