use arora_buffers::serde_uuid::serialize;
use arora_schema::value::{Structure, StructureField, Value};
use derive_more::Display;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, rc::Rc};
use uuid::Uuid;

use crate::module::DispatchError;

/// A call is described like a structure in arora engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Call {
  pub id: Uuid,
  #[serde(default)]
  pub args: Vec<StructureField>,
}

pub fn serialize_to_arg(call: Call) -> Box<[u8]> {
  return serialize(&Value::Structure(Structure {
    id: call.id,
    fields: call.args,
  }));
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallResult {
  pub ret: Value,
  #[serde(default)]
  pub mutated: Vec<StructureField>,
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
}

impl From<DispatchError> for CallError {
  fn from(e: DispatchError) -> Self {
    match e {
      DispatchError::ModuleNotFound { id } => CallError::ModuleNotFound { id },
      DispatchError::FunctionNotFound { id } => CallError::FunctionNotFound { id },
      DispatchError::Trap { message } => CallError::Trap { message },
      DispatchError::Internal { message } => CallError::Internal { message },
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
    let mut rng = thread_rng();
    let mut tick_id = CallableId {
      id: rng.gen::<u64>(),
    };
    while self.callables_by_id.contains_key(&tick_id) {
      tick_id.id = rng.gen::<u64>();
    }
    tick_id
  }
}

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::{bail, Result};
  use std::str::FromStr;
  use uuid::Uuid;

  #[test]
  pub fn parse_call_test() -> Result<()> {
    let call: Call = serde_yaml::from_str(CALL_TEST)?;
    assert_eq!(
      call.id,
      Uuid::from_str("07f5740c-ba4a-45af-8ec5-bedde5737e99")?
    );
    if let Value::Structure(Structure { id, fields }) = &call.args[1].value.as_ref() {
      assert_eq!(*id, Uuid::from_str("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c")?);
      if let Value::I32(v) = fields[1].value.as_ref() {
        assert_eq!(*v, 113);
      } else {
        bail!("expected i32 value under second field of struct arg");
      }
    } else {
      bail!("expected a string under arg 55dbec70-1c3a-433e-a6e6-27446b7f065e");
    }
    Ok(())
  }

  #[test]
  pub fn parse_call_test_2() -> Result<()> {
    let call: Call = serde_yaml::from_str(CALL_TEST_2)?;
    assert_eq!(
      call.id,
      Uuid::from_str("b213a552-77ad-465a-a26d-352e8eccfd63")?
    );
    assert_eq!(call.args.len(), 2);
    Ok(())
  }

  pub const CALL_TEST: &'static str = "\
id: 07f5740c-ba4a-45af-8ec5-bedde5737e99
args:
- id: b41899c3-66dc-40d4-ab61-d1ccf5231c88
  value:
    enum:
      id: 325a5767-e344-4532-860e-0749bcf2e428
      variant_id: 766e9e9a-446d-4e46-83e6-14b7ca101169
      value: unit
- id: 63086e48-804f-403a-8862-3358ddedc08d
  value:
    struct:
      id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c
      fields:
      - id: 7d94a956-e50d-4cc4-9714-f62e1f9b134e
        value:
          enum[]:
            id: 325a5767-e344-4532-860e-0749bcf2e428
            elements:
              - variant_id: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
                value: unit
      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563
        value:
          i32: 113
";

  pub const CALL_TEST_2: &'static str = "\
id: b213a552-77ad-465a-a26d-352e8eccfd63
args:
- id: 55dbec70-1c3a-433e-a6e6-27446b7f065e
  value:
    u32: 42
- id: abf9ca4e-e03f-431a-a32b-4911f809c399
  value:
    u32: 64
";
}
