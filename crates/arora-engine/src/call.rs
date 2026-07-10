use std::{collections::HashMap, rc::Rc};

use rand::{rng, Rng};
use uuid::Uuid;

use arora_buffers::serde_uuid::{deserialize, serialize};
pub use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::value::{Structure, StructureField, Value};

use crate::module::DispatchError;

pub fn serialize_to_arg(call: Call) -> Box<[u8]> {
    serialize(&Value::Structure(Structure {
        id: call.id,
        fields: call.args,
    }))
}

/// Decode a dispatch argument buffer back into the [`Call`] it serializes —
/// the inverse of [`serialize_to_arg`]. `module_id` is not on the wire (the
/// dispatch already routed), so it comes back `None`.
pub fn decode_arg(function_id: Uuid, arg: &[u8]) -> Result<Call, String> {
    match deserialize(arg) {
        Value::Structure(structure) => {
            if structure.id != function_id {
                return Err(format!(
                    "argument structure id {} differs from function id {}",
                    structure.id, function_id
                ));
            }
            Ok(Call {
                module_id: None,
                id: structure.id,
                args: structure.fields,
            })
        }
        _ => Err("argument buffer is not a structure".to_string()),
    }
}

/// Encode a [`CallResult`] the way guest modules do — a [`Structure`] whose id
/// is the function id, first field the return value, remaining fields the
/// mutated arguments. The exact form `arora_call`'s result parsing expects.
pub fn encode_call_result(function_id: Uuid, result: CallResult) -> Box<[u8]> {
    let mut fields = Vec::with_capacity(1 + result.mutated.len());
    fields.push(StructureField {
        id: function_id,
        value: Box::new(result.ret),
    });
    fields.extend(result.mutated);
    serialize(&Value::Structure(Structure {
        id: function_id,
        fields,
    }))
}

// `Callable`, `CallBridge`, `CallError`, and `CallableId` now live in
// `arora-types` (re-exported above) so module-shaped libraries can use the
// call boundary without depending on the engine crate. The engine-internal
// machinery below stays here.

/// Maps the engine's internal [`DispatchError`] onto the public
/// [`CallError`].
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

lazy_static::lazy_static! {
  pub static ref CALLABLE_ID_TYPE_ID: Uuid = Uuid::parse_str("6dd7f535-8245-4bf2-b081-81fd4636fa90").unwrap();
  pub static ref CALLABLE_ID_ID_FIELD_ID: Uuid = Uuid::parse_str("d799be14-e12a-4539-ac56-f7dc4634161b").unwrap();
}

pub struct CallableRegistry {
    callables_by_id: HashMap<CallableId, Rc<dyn Callable>>,
}

impl Default for CallableRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CallableRegistry {
    pub fn new() -> Self {
        Self {
            callables_by_id: HashMap::new(),
        }
    }

    pub fn register_callable(
        &mut self,
        callable: Rc<dyn Callable>,
    ) -> Result<CallableId, CallError> {
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
        let mut tick_id = CallableId { id: rng.next_u64() };
        while self.callables_by_id.contains_key(&tick_id) {
            tick_id.id = rng.next_u64();
        }
        tick_id
    }
}
