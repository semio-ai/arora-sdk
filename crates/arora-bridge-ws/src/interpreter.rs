//! The interpreter-module ABI a task run travels on, and the value-plane shape
//! of a described signature.
//!
//! A method whose return type is the behavior-tree `Status` enumeration is
//! **task-shaped**: it reports `Running`/`Success`/`Failure` across repeated
//! ticks, which is what the engine's interpreter hosts as a task run. Starting
//! and stopping one goes through the interpreter module — the well-known ids
//! `arora-behavior`'s `interpreter_module` defines — so this crate speaks that
//! ABI over the value plane, needing the ids and the payload shapes but no code
//! dependency on the engine. `abi_matches_the_interpreter_module` (a
//! dev-dependency test) pins them against the defining crate; the ids are
//! self-identifying ("arora" in ASCII leads the UUID) and stable.

use arora_behavior_tree_types::STATUS_ENUMERATION_ID;
use arora_types::call::Call;
use arora_types::data::Key;
use arora_types::keyvalue::{KeyValue, KeyValueField};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Type, Value};
use arora_types::{value_serde, Uuid};

/// The interpreter module's id on the engine.
const INTERPRETER_MODULE: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000001);
/// Function id of **spawn**: start a task run, returning its handle.
const SPAWN: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000006);
/// Argument id of [`SPAWN`]'s first argument: the [`Call`] to run.
const SPAWN_CALL_ARG: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000007);
/// Argument id of [`SPAWN`]'s second argument: the run policy.
const SPAWN_POLICY_ARG: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000008);
/// Function id of **halt**: stop the run a task id names.
const HALT: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000009);
/// Argument id of [`HALT`]'s one argument: the run's task id.
const HALT_ARG: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_00000000000a);

/// The engine's `RunPolicy`, mirrored for the value plane (serde encodes by
/// variant name, so the mirror travels identically). A client's invoke spawns
/// concurrently: two calls are two runs, and the device's behavior is what
/// arbitrates between them.
#[derive(serde::Serialize)]
enum RunPolicy {
    Concurrent,
}

/// Whether a return type makes its method task-shaped — a run to spawn and
/// halt, rather than a call that answers.
pub(crate) fn task_shaped(return_ty: &FrozenTy) -> bool {
    matches!(
        return_ty,
        FrozenTy::FrozenScalar(scalar) if scalar.reference.id == STATUS_ENUMERATION_ID
    )
}

/// The call that spawns `call` as a task run, answering with its handle.
pub(crate) fn spawn(call: &Call) -> Call {
    Call {
        module_id: Some(INTERPRETER_MODULE),
        id: SPAWN,
        args: vec![
            StructureField {
                id: SPAWN_CALL_ARG,
                value: Box::new(value_serde::to_value(call).expect("a Call converts to a Value")),
            },
            StructureField {
                id: SPAWN_POLICY_ARG,
                value: Box::new(
                    value_serde::to_value(&RunPolicy::Concurrent)
                        .expect("a RunPolicy converts to a Value"),
                ),
            },
        ],
    }
}

/// The call that halts the run `run` names.
///
/// The run id travels as the engine's `TaskId` does — serde-encoded, which puts
/// a uuid on the value plane as its text, not as a `Value::Uuid`. The pinning
/// test is what keeps that in step with the defining crate.
pub(crate) fn halt(run: Uuid) -> Call {
    Call {
        module_id: Some(INTERPRETER_MODULE),
        id: HALT,
        args: vec![StructureField {
            id: HALT_ARG,
            value: Box::new(value_serde::to_value(&run).expect("a uuid converts to a Value")),
        }],
    }
}

/// The run a spawn answers with, read off the value plane: the interpreter module
/// returns the engine's `TaskHandle` serde-encoded, and this mirror
/// deserialises it by field name — the field names are `arora-behavior`'s, and
/// the pinning test keeps them so. `stop` is left out: a client halts by run id.
#[derive(serde::Deserialize)]
pub(crate) struct Run {
    /// The run's identity, which a `halt` names.
    id: Uuid,
    /// The key an observer watches to learn how the run ended.
    status: Key,
    /// Keys carrying the run's progress while it runs.
    feedback: Vec<Key>,
    /// Keys carrying its result, written when it terminates.
    result: Vec<Key>,
    /// Keys an observer may write to steer it — a moving target, say.
    update: Vec<Key>,
}

/// Read the [`Run`] out of what a spawn answered.
pub(crate) fn run_of(value: &Value) -> Result<Run, String> {
    value_serde::from_value(value.clone()).map_err(|e| format!("malformed run handle: {e}"))
}

/// The run as a client reads it: named fields, like everything else on this wire
/// — keys as paths, arguments as parameter names. `run` is what a `halt` names,
/// `status` the key that says how it ended.
pub(crate) fn run_value(run: &Run) -> Value {
    let paths = |keys: &[Key]| {
        Value::ArrayString(keys.iter().map(|key| key.path.clone()).collect::<Vec<_>>())
    };
    KeyValue::from(vec![
        KeyValueField::new("run", Value::String(run.id.to_string())),
        KeyValueField::new("status", Value::String(run.status.path.clone())),
        KeyValueField::new("feedback", paths(&run.feedback)),
        KeyValueField::new("result", paths(&run.result)),
        KeyValueField::new("update", paths(&run.update)),
    ])
    .as_value()
}

/// The value-plane shape of a frozen type: which [`Value`] a client sends for a
/// parameter of that type, and which one the method answers with.
///
/// A non-primitive is advertised as the shape it travels in — a structure, or an
/// array of values — not as the record it is: pinning that down needs a type
/// registry, which a bridge does not hold. A client that knows the record sends
/// its encoding either way.
pub(crate) fn value_type(ty: &FrozenTy) -> Type {
    match ty {
        FrozenTy::Primitive(primitive) => match primitive.kind {
            PrimitiveKind::Unit => Type::Unit,
            PrimitiveKind::Boolean => Type::Boolean,
            PrimitiveKind::U8 => Type::U8,
            PrimitiveKind::U16 => Type::U16,
            PrimitiveKind::U32 => Type::U32,
            PrimitiveKind::U64 => Type::U64,
            PrimitiveKind::I8 => Type::I8,
            PrimitiveKind::I16 => Type::I16,
            PrimitiveKind::I32 => Type::I32,
            PrimitiveKind::I64 => Type::I64,
            PrimitiveKind::F32 => Type::F32,
            PrimitiveKind::F64 => Type::F64,
            PrimitiveKind::String => Type::String,
            PrimitiveKind::ArrayBoolean => Type::ArrayBoolean,
            PrimitiveKind::ArrayU8 => Type::ArrayU8,
            PrimitiveKind::ArrayU16 => Type::ArrayU16,
            PrimitiveKind::ArrayU32 => Type::ArrayU32,
            PrimitiveKind::ArrayU64 => Type::ArrayU64,
            PrimitiveKind::ArrayI8 => Type::ArrayI8,
            PrimitiveKind::ArrayI16 => Type::ArrayI16,
            PrimitiveKind::ArrayI32 => Type::ArrayI32,
            PrimitiveKind::ArrayI64 => Type::ArrayI64,
            PrimitiveKind::ArrayF32 => Type::ArrayF32,
            PrimitiveKind::ArrayF64 => Type::ArrayF64,
            PrimitiveKind::ArrayString => Type::ArrayString,
        },
        FrozenTy::FrozenScalar(_) => Type::Structure,
        FrozenTy::FrozenArray(_) => Type::ArrayValue,
        FrozenTy::FrozenOption(_) => Type::Option,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::record::ty::{FrozenScalar, Primitive};
    use arora_types::record::{FrozenReference, Version};

    fn scalar(id: Uuid) -> FrozenTy {
        FrozenTy::FrozenScalar(FrozenScalar {
            reference: FrozenReference {
                id,
                version: Version::parse("1.0.0").expect("a valid version"),
            },
        })
    }

    /// The value-plane ABI this crate speaks is the interpreter module's, id for
    /// id and byte for byte — the pin behind speaking it without a dependency on
    /// the engine.
    #[test]
    fn abi_matches_the_interpreter_module() {
        use arora_behavior::interpreter_module;

        let inner = Call {
            module_id: Some(Uuid::from_u128(1)),
            id: Uuid::from_u128(2),
            args: vec![],
        };
        assert_eq!(
            spawn(&inner),
            interpreter_module::encode_spawn(&inner, arora_behavior::RunPolicy::Concurrent),
            "the spawn call must be the interpreter module's"
        );

        let run = Uuid::from_u128(5);
        assert_eq!(
            halt(run),
            interpreter_module::encode_halt(arora_behavior::TaskId(run)),
            "the halt call must be the interpreter module's"
        );

        // The run mirror reads what a spawn answers with.
        let task = arora_behavior::TaskId(run);
        let handle = arora_behavior::TaskHandle {
            id: task,
            stop: interpreter_module::encode_halt(task),
            status: Key::from("arora/tasks/m/f/5/status"),
            feedback: vec![Key::from("arora/tasks/m/f/5/feedback")],
            result: vec![Key::from("arora/tasks/m/f/5/result")],
            update: vec![Key::from("arora/tasks/m/f/5/target")],
        };
        let mirrored =
            run_of(&interpreter_module::encode_spawn_result(&handle)).expect("the run decodes");
        assert_eq!(mirrored.id, run);
        assert_eq!(mirrored.status, handle.status);
        assert_eq!(mirrored.feedback, handle.feedback);
        assert_eq!(mirrored.result, handle.result);
        assert_eq!(mirrored.update, handle.update);
    }

    /// A run answers with a behavior status; anything else answers with a value.
    #[test]
    fn a_status_return_is_task_shaped() {
        assert!(task_shaped(&scalar(STATUS_ENUMERATION_ID)));
        assert!(!task_shaped(&scalar(Uuid::from_u128(7))));
        assert!(!task_shaped(&FrozenTy::Primitive(Primitive::from(
            PrimitiveKind::F32
        ))));
    }
}
