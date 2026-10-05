//! What a client of a device needs, whatever carries it to the device.
//!
//! A client reaches a device through [`BridgeOp`](crate::BridgeOp)s — over a
//! bridge's transport, or through the device's own in-process queue — and the
//! ops are the same either way. Some of what it asks for is not one op but a
//! convention over several, and this module holds those conventions so every
//! client speaks them alike:
//!
//! - **Calling a method by name.** [`DescribeMethods`](crate::BridgeOp::DescribeMethods)
//!   gives each method's [`MethodSignature`]; [`find_method`] picks the one a
//!   name (and, when the name is shared, a module) designates, and [`call_of`]
//!   binds arguments given by parameter name onto the ids that signature
//!   declares.
//! - **Runs.** A method whose return type is the behavior `Status` is
//!   [`task_shaped`]: it reports `Running`/`Success`/`Failure` across ticks, so
//!   the device hosts it as a task run rather than answering it at once. A
//!   client starts one by wrapping its call in [`spawn`], reads the run a spawn
//!   answers with through [`run_of`], and stops it with [`halt`]. These speak
//!   the interpreter module's ABI (`arora_behavior::interpreter_module`) over
//!   the value plane.
//! - **What a client reads back.** [`KeyInfo`] is a key with what its store says
//!   it is, and [`MethodInfo`] a method as a by-name client sees it — the shapes
//!   a client is answered with, so two clients of one device read the same thing.

use std::collections::HashMap;

use arora_behavior::interpreter_module;
use arora_behavior::{RunPolicy, TaskId, STATUS_ENUMERATION_ID};
use arora_types::call::Call;
use arora_types::data::{Key, KeyMeta};
use arora_types::keyvalue::{KeyValue, KeyValueField};
use arora_types::record::module::frozen::Function;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Type, Value};
use arora_types::Uuid;
use serde::{Deserialize, Serialize};

use crate::MethodSignature;

/// A key of the device: its path, and what the store says it is.
///
/// The meta is the store's answer, relayed — the shape the key holds, the range
/// it runs over and the unit it is counted in, where it rests, what it is for,
/// and whether a client may write it. A key nobody has described carries the
/// default: the shape of the value it holds, and closed to writes.
///
/// Serialized, the meta is `__meta`, the name every Arora bridge gives it, so a
/// key segment called `meta` never collides with it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyInfo {
    /// Hierarchical path identifier (e.g. `face/mouth/open`).
    pub path: String,

    /// What the store says this key is.
    #[serde(rename = "__meta")]
    pub meta: KeyMeta,
}

/// Descriptor for a method parameter, as a by-name client binds an argument to
/// it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodParam {
    /// Parameter name
    pub name: String,

    /// Parameter type
    pub param_type: Type,

    /// Whether this parameter is required
    #[serde(default)]
    pub required: bool,

    /// Default value if not provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Metadata describing a callable method, as a by-name client sees it.
///
/// Built from a described signature by [`method_info`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MethodInfo {
    /// Method path/name (e.g., "audio/play", "animation/trigger", "reset")
    pub path: String,

    /// Method parameters
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<MethodParam>,

    /// Return type (None means void/unit)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<Type>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether invoking this method starts a **run**: long-running, cancellable
    /// work that answers with a task handle at once and reports its outcome on
    /// the handle's status key. A client halts it by the run's id; a plain
    /// method answers with its return value instead.
    #[serde(default)]
    pub task: bool,
}

/// A described signature as a client reads it: the method's name, its parameters
/// in declaration order with the value shape and optionality of each, and
/// whether calling it starts a run.
pub fn method_info(signature: &MethodSignature) -> MethodInfo {
    let function = &signature.function;
    MethodInfo {
        path: signature.name.clone(),
        params: function
            .parameter_ordering
            .iter()
            .filter_map(|id| {
                let parameter = function.parameter(id)?;
                Some(MethodParam {
                    name: parameter.name.clone(),
                    param_type: value_type(&parameter.ty),
                    required: !parameter.ty.is_option(),
                    default_value: None,
                    description: None,
                })
            })
            .collect(),
        return_type: Some(value_type(&function.return_ty)),
        description: None,
        task: task_shaped(function),
    }
}

/// The signature among `signatures` that `method` designates, narrowed to the
/// module `module` when one is given.
///
/// Method names are the bare names modules export, so two modules may export one
/// name (`play`, `stop`). A name more than one module exports designates
/// nothing on its own: this fails naming those modules, and the client names
/// the one it means. It never picks one for the client, because calling the
/// wrong module's `stop` is worse than being told to choose.
pub fn find_method<'a>(
    signatures: &'a [MethodSignature],
    method: &str,
    module: Option<Uuid>,
) -> Result<&'a MethodSignature, String> {
    let mut found = signatures
        .iter()
        .filter(|s| s.name == method && module.is_none_or(|module| s.module_id == module));
    let Some(first) = found.next() else {
        return Err(match module {
            Some(module) => format!("Method not found: {method} in module {module}"),
            None => format!("Method not found: {method}"),
        });
    };
    let rest: Vec<&MethodSignature> = found.collect();
    if rest.is_empty() {
        return Ok(first);
    }
    let mut modules: Vec<String> = std::iter::once(first)
        .chain(rest)
        .map(|s| s.module_id.to_string())
        .collect();
    modules.sort();
    modules.dedup();
    Err(format!(
        "{method} is exported by several modules ({}); name the module to call",
        modules.join(", ")
    ))
}

/// Bind `args` by parameter name onto the parameter ids `signature` declares,
/// giving the [`Call`] that invokes it.
///
/// An argument the signature does not name fails the call rather than being
/// dropped, so a client that misspells a parameter learns it. A parameter no
/// argument names is left out, and whether it may be is the function's business:
/// an optional parameter reads as absent, a required one fails the call naming
/// itself. The arguments follow the declaration order, so the call reads like
/// the signature.
pub fn call_of(signature: &MethodSignature, args: HashMap<String, Value>) -> Result<Call, String> {
    let function = &signature.function;
    let mut fields = Vec::with_capacity(args.len());
    for (name, value) in args {
        let Some(id) = function.parameter_id(&name) else {
            return Err(format!("{} has no parameter '{name}'", signature.name));
        };
        fields.push(StructureField {
            id: *id,
            value: Box::new(value),
        });
    }
    fields.sort_by_key(|field| {
        function
            .parameter_ordering
            .iter()
            .position(|id| *id == field.id)
            .unwrap_or(usize::MAX)
    });
    Ok(Call {
        module_id: Some(signature.module_id),
        id: signature.id,
        args: fields,
    })
}

/// Whether `function` is task-shaped — a run to [`spawn`] and [`halt`], rather
/// than a call that answers. It is when it returns the behavior `Status`
/// enumeration, which is what a run reports across ticks.
pub fn task_shaped(function: &Function) -> bool {
    matches!(
        &function.return_ty,
        FrozenTy::FrozenScalar(scalar) if scalar.reference.id == STATUS_ENUMERATION_ID
    )
}

/// The call that spawns `call` as a task run. It answers with the run's
/// handle (read it with [`run_of`]).
///
/// A client's run is concurrent: two spawns are two runs, and the device's
/// behavior is what arbitrates between them.
pub fn spawn(call: &Call) -> Call {
    interpreter_module::encode_spawn(call, RunPolicy::Concurrent)
}

/// The call that halts the run whose id is `run`. Halting is idempotent: a
/// finished or unknown run is a clean no-op.
pub fn halt(run: Uuid) -> Call {
    interpreter_module::encode_halt(TaskId(run))
}

/// A run as a client reads it: named fields, keys as paths — the form a by-name
/// client is given everything in.
///
/// Read it out of what a [`spawn`] answered with [`run_of`]. Serialized, it is
/// `{"run": "<uuid>", "status": "<path>", "feedback": [...], "result": [...],
/// "update": [...]}`; [`to_value`](Self::to_value) gives it as a value-plane
/// key-value with the same field names, for a client whose answers travel as
/// [`Value`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// The run's id, which [`halt`] takes.
    pub run: Uuid,
    /// The key that says how the run is going and how it ended: a behavior
    /// `Status`, `Running` until it ends.
    pub status: String,
    /// The keys carrying the run's progress while it runs.
    pub feedback: Vec<String>,
    /// The keys carrying its result, written when it ends.
    pub result: Vec<String>,
    /// The keys an observer may write to steer it — a moving target, say.
    pub update: Vec<String>,
}

impl Run {
    /// The run as a value-plane key-value, its fields named as this struct's:
    /// `run` (the id as a string), `status`, and the `feedback`, `result` and
    /// `update` paths as string arrays.
    pub fn to_value(&self) -> Value {
        KeyValue::from(vec![
            KeyValueField::new("run", Value::String(self.run.to_string())),
            KeyValueField::new("status", Value::String(self.status.clone())),
            KeyValueField::new("feedback", Value::ArrayString(self.feedback.clone())),
            KeyValueField::new("result", Value::ArrayString(self.result.clone())),
            KeyValueField::new("update", Value::ArrayString(self.update.clone())),
        ])
        .as_value()
    }
}

/// The run a [`spawn`] answered with. Fails when `spawned` is not a run handle.
pub fn run_of(spawned: &Value) -> Result<Run, String> {
    let handle = interpreter_module::decode_spawn_result(spawned)?;
    let paths = |keys: Vec<Key>| keys.into_iter().map(|key| key.path).collect();
    Ok(Run {
        run: handle.id.0,
        status: handle.status.path,
        feedback: paths(handle.feedback),
        result: paths(handle.result),
        update: paths(handle.update),
    })
}

/// The value-plane shape of a frozen type: which [`Value`] a client sends for a
/// parameter of that type, and which one the method answers with.
///
/// A non-primitive is advertised as the shape it travels in — a structure, or an
/// array of values — not as the record it is: pinning that down needs a type
/// registry, which a client of the device does not hold. A client that knows
/// the record sends its encoding either way.
fn value_type(ty: &FrozenTy) -> Type {
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
    use arora_behavior::TaskHandle;
    use arora_types::record::module::frozen::Parameter;
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

    fn returning(return_ty: FrozenTy) -> Function {
        Function {
            parameters: HashMap::new(),
            parameter_ordering: Vec::new(),
            return_ty,
        }
    }

    /// `name(a: f32, b: f32) -> f32` exported by `module`.
    fn signature(module: u128, id: u128, name: &str) -> MethodSignature {
        let a = Uuid::from_u128(id << 8 | 1);
        let b = Uuid::from_u128(id << 8 | 2);
        let parameter = |name: &str| Parameter {
            name: name.to_string(),
            ty: FrozenTy::from(PrimitiveKind::F32),
            mutable: false,
        };
        MethodSignature {
            module_id: Uuid::from_u128(module),
            id: Uuid::from_u128(id),
            name: name.to_string(),
            function: Function {
                parameters: HashMap::from([(a, parameter("a")), (b, parameter("b"))]),
                parameter_ordering: vec![a, b],
                return_ty: FrozenTy::from(PrimitiveKind::F32),
            },
        }
    }

    /// A run answers with a behavior status; anything else answers with a value.
    #[test]
    fn a_status_return_is_task_shaped() {
        assert!(task_shaped(&returning(scalar(STATUS_ENUMERATION_ID))));
        assert!(!task_shaped(&returning(scalar(Uuid::from_u128(7)))));
        assert!(!task_shaped(&returning(FrozenTy::Primitive(
            Primitive::from(PrimitiveKind::F32)
        ))));
    }

    /// Arguments bind by name, in declaration order; a name the signature does
    /// not declare fails the call, naming it.
    #[test]
    fn call_of_binds_arguments_by_parameter_name() {
        let add = signature(1, 2, "add");
        let call = call_of(
            &add,
            HashMap::from([
                ("b".to_string(), Value::F32(2.0)),
                ("a".to_string(), Value::F32(1.0)),
            ]),
        )
        .expect("both parameters exist");
        assert_eq!(call.module_id, Some(add.module_id));
        assert_eq!(call.id, add.id);
        let bound: Vec<(Uuid, Value)> = call
            .args
            .into_iter()
            .map(|field| (field.id, *field.value))
            .collect();
        assert_eq!(
            bound,
            vec![
                (add.function.parameter_ordering[0], Value::F32(1.0)),
                (add.function.parameter_ordering[1], Value::F32(2.0)),
            ]
        );

        let error = call_of(&add, HashMap::from([("c".to_string(), Value::F32(0.0))]))
            .expect_err("add has no parameter c");
        assert_eq!(error, "add has no parameter 'c'");
    }

    /// A name one module exports designates it; a name several export
    /// designates nothing until the module is named, and the error names them.
    #[test]
    fn a_shared_name_needs_its_module() {
        let signatures = vec![
            signature(0xA, 1, "play"),
            signature(0xB, 2, "play"),
            signature(0xA, 3, "seek"),
        ];

        assert_eq!(
            find_method(&signatures, "seek", None).unwrap().id,
            Uuid::from_u128(3)
        );

        let error = find_method(&signatures, "play", None).expect_err("play is ambiguous");
        assert!(error.contains(&Uuid::from_u128(0xA).to_string()), "{error}");
        assert!(error.contains(&Uuid::from_u128(0xB).to_string()), "{error}");

        let play = find_method(&signatures, "play", Some(Uuid::from_u128(0xB)))
            .expect("the module disambiguates");
        assert_eq!(play.id, Uuid::from_u128(2));

        assert_eq!(
            find_method(&signatures, "pause", None).expect_err("nothing exports pause"),
            "Method not found: pause"
        );
        assert!(find_method(&signatures, "seek", Some(Uuid::from_u128(0xB))).is_err());
    }

    /// A spawn answers with the interpreter's handle, and a client reads it as
    /// named fields, the run id first among them; the run id is what [`halt`]
    /// takes.
    #[test]
    fn a_spawned_run_reads_as_named_fields() {
        let run = Uuid::from_u128(5);
        let task = TaskId(run);
        let handle = TaskHandle {
            id: task,
            stop: interpreter_module::encode_halt(task),
            status: Key::from("arora/tasks/m/f/5/status"),
            feedback: vec![Key::from("arora/tasks/m/f/5/feedback")],
            result: vec![Key::from("arora/tasks/m/f/5/result")],
            update: vec![Key::from("arora/tasks/m/f/5/target")],
        };
        let read =
            run_of(&interpreter_module::encode_spawn_result(&handle)).expect("the run decodes");
        assert_eq!(
            read,
            Run {
                run,
                status: "arora/tasks/m/f/5/status".to_string(),
                feedback: vec!["arora/tasks/m/f/5/feedback".to_string()],
                result: vec!["arora/tasks/m/f/5/result".to_string()],
                update: vec!["arora/tasks/m/f/5/target".to_string()],
            }
        );
        assert_eq!(halt(read.run), handle.stop);

        let Value::KeyValue(fields) = read.to_value() else {
            panic!("a run reads as a key-value");
        };
        let field = |name: &str| {
            fields
                .fields
                .values()
                .find(|field| field.name == name)
                .and_then(|field| field.value.as_deref().cloned())
                .unwrap_or_else(|| panic!("the run carries {name}"))
        };
        assert_eq!(field("run"), Value::String(run.to_string()));
        assert_eq!(field("status"), Value::String(read.status.clone()));
        assert_eq!(field("update"), Value::ArrayString(read.update.clone()));

        assert!(run_of(&Value::Unit).is_err());
    }

    /// Spawning wraps the call in the interpreter's spawn, concurrently.
    #[test]
    fn spawn_wraps_the_call_concurrently() {
        let inner = Call {
            module_id: Some(Uuid::from_u128(1)),
            id: Uuid::from_u128(2),
            args: vec![],
        };
        assert_eq!(
            interpreter_module::decode_spawn(&spawn(&inner)).expect("a spawn call"),
            (inner, RunPolicy::Concurrent)
        );
    }
}
