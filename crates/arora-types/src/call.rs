use std::rc::Rc;

use derive_more::Display;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::{StructureField, Value};

/// A call is described like a structure in arora engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Call {
  /// The ID of the module where to find the function ID.
  /// If absent, look for it locally.
  #[serde(default)]
  pub module_id: Option<Uuid>,
  /// The function ID to call.
  pub id: Uuid,
  /// Arguments to call the functions with.
  #[serde(default)]
  pub args: Vec<StructureField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallResult {
  pub ret: Value,
  #[serde(default)]
  pub mutated: Vec<StructureField>,
}

/// Anything that can be invoked through a [`CallBridge`], e.g. a
/// behavior-tree tick registered as an indirect callable.
pub trait Callable {
  fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError>;
}

/// The interface a module uses to call back into its host (the engine, or a
/// mock in tests). It lives here, in the interface layer, so module-shaped
/// libraries can make host calls without depending on the engine crate.
pub trait CallBridge {
  /// Dispatch `call` to the module it names: the call is the full description
  /// of the invocation, and one naming no module is refused.
  fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError>;

  /// Registers the given function in the executor and associates it to an
  /// identifier generated on the fly. The function is made available to
  /// every module by calling `arora_dispatch_indirect(id: u64) -> Value`.
  fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId;

  /// Unregisters the function associated to the given identifier.
  fn arora_unregister_callable(&mut self, callable_id: &CallableId);

  /// Calls a callable that was registered.
  fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError>;
}

#[derive(Display, Debug)]
pub enum CallError {
  Generic {
    message: String,
  },
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
  /// The guest returned a structured error via TYPE_ERROR instead of trapping.
  Guest {
    message: String,
  },
}

impl std::error::Error for CallError {}

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

#[cfg(test)]
mod tests {
  use crate::value::{Structure, Value};

  use super::*;
  use std::str::FromStr;
  use uuid::Uuid;

  #[test]
  pub fn parse_call_test() {
    // The call format keeps values in their singleton-map YAML form.
    let call: Call = serde_yaml::with::singleton_map_recursive::deserialize(
      serde_yaml::Deserializer::from_str(CALL_TEST),
    )
    .unwrap();
    assert_eq!(
      call.id,
      Uuid::from_str("07f5740c-ba4a-45af-8ec5-bedde5737e99").unwrap()
    );
    if let Value::Structure(Structure { id, fields }) = &call.args[1].value.as_ref() {
      assert_eq!(
        *id,
        Uuid::from_str("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c").unwrap()
      );
      if let Value::I32(v) = fields[1].value.as_ref() {
        assert_eq!(*v, 113);
      } else {
        panic!("expected i32 value under second field of struct arg");
      }
    } else {
      panic!("expected a string under arg 55dbec70-1c3a-433e-a6e6-27446b7f065e");
    }
  }

  #[test]
  pub fn parse_call_test_2() {
    let call: Call = serde_yaml::with::singleton_map_recursive::deserialize(
      serde_yaml::Deserializer::from_str(CALL_TEST_2),
    )
    .unwrap();
    assert_eq!(
      call.id,
      Uuid::from_str("b213a552-77ad-465a-a26d-352e8eccfd63").unwrap()
    );
    assert_eq!(call.args.len(), 2);
  }

  /// Proves the call-bridge interface stands on its own: a module-shaped
  /// library can register and invoke callables with no engine present.
  #[test]
  fn callable_round_trips_through_a_mock_bridge() {
    use std::collections::HashMap;
    use std::rc::Rc;

    #[derive(Default)]
    struct MockBridge {
      registered: HashMap<CallableId, Rc<dyn Callable>>,
      next_id: u64,
    }

    impl CallBridge for MockBridge {
      fn arora_call(&mut self, _call: Call) -> Result<CallResult, CallError> {
        Err(CallError::Generic {
          message: "the mock has no modules".to_string(),
        })
      }
      fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        let id = CallableId::from(self.next_id);
        self.next_id += 1;
        self.registered.insert(id.clone(), callable);
        id
      }
      fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
        self.registered.remove(callable_id);
      }
      fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        let callable = self
          .registered
          .get(callable_id)
          .cloned()
          .ok_or(CallError::Generic {
            message: "unknown callable".to_string(),
          })?;
        callable.call(self)
      }
    }

    struct Answer;
    impl Callable for Answer {
      fn call(&self, _caller: &mut dyn CallBridge) -> Result<Value, CallError> {
        Ok(Value::I32(42))
      }
    }

    let mut bridge = MockBridge::default();
    let id = bridge.arora_register_callable(Rc::new(Answer));
    let result = bridge.arora_call_indirect(&id).unwrap();
    assert!(matches!(result, Value::I32(42)));

    bridge.arora_unregister_callable(&id);
    assert!(bridge.arora_call_indirect(&id).is_err());
  }

  pub const CALL_TEST: &str = "\
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
          enums:
            id: 325a5767-e344-4532-860e-0749bcf2e428
            elements:
              - variant_id: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
                value: unit
      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563
        value:
          i32: 113
";

  pub const CALL_TEST_2: &str = "\
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
