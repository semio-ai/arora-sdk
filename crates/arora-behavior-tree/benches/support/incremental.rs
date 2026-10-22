//! Small deterministic module host shared by the incremental tests and bench.
use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::{FunctionIndex, ModuleFunction, Status};
use arora_behavior_tree_types::TickId;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::Slot;
use arora_types::module::low::TypeRef;
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use arora_types::AroraType;
use uuid::Uuid;

pub const X: Uuid = Uuid::from_u128(0x10001);
pub const Y: Uuid = Uuid::from_u128(0x10002);
pub const STATE: Uuid = Uuid::from_u128(0x10003);
pub const OUT: Uuid = Uuid::from_u128(0x10004);
pub const MODULE: Uuid = Uuid::from_u128(0x10000);
pub const CHILDREN: Uuid = uuid::uuid!("5b6e9515-dbcc-411d-bee9-3d8cba5fedda");

#[derive(Clone, Copy)]
pub enum Op {
    Add,
    #[allow(dead_code)] // Exercised by the tests sharing this host.
    Approach,
    Mul,
    Clamp,
    Remap,
    #[allow(dead_code)] // Exercised by the tests sharing this host.
    Effect,
    #[allow(dead_code)]
    Running(u64),
    #[allow(dead_code)] // Exercised by the tests sharing this host.
    WriteThenTick,
}

pub fn signature(id: Uuid) -> (Uuid, ModuleFunction) {
    let parameters: Vec<_> = [
        (X, "x", false),
        (Y, "y", false),
        (STATE, "state", true),
        (OUT, "out", true),
    ]
    .into_iter()
    .map(|(id, name, mutable)| {
        (
            id,
            Parameter {
                name: name.into(),
                ty: FrozenTy::from(PrimitiveKind::F64),
                mutable,
            },
        )
    })
    .collect();
    (
        id,
        ModuleFunction {
            module_id: MODULE,
            function_id: id,
            function_name: format!("f{id}"),
            function: Function {
                parameter_ordering: parameters.iter().map(|(id, _)| *id).collect(),
                parameters: parameters.into_iter().collect(),
                return_ty: FrozenTy::from(PrimitiveKind::F64),
            },
        },
    )
}

pub type RecordedCall = (Uuid, Vec<StructureField>);
#[derive(Default)]
pub struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
    pub ops: HashMap<Uuid, Op>,
    pub calls: Vec<RecordedCall>,
    pub dispatches: u64,
    pub module_calls: u64,
    pub count_only: bool,
    pub raw_write: Option<Box<dyn Slot>>,
    raw_writes: u64,
}
impl Bridge {
    pub fn index(&self) -> FunctionIndex {
        self.ops
            .iter()
            .map(|(id, op)| {
                let (id, mut function) = signature(*id);
                if matches!(op, Op::WriteThenTick) {
                    function.function = Function {
                        parameter_ordering: vec![CHILDREN],
                        parameters: HashMap::from([(
                            CHILDREN,
                            Parameter {
                                name: "children".into(),
                                ty: FrozenTy::from_type_ref(
                                    &TypeRef::Array {
                                        id: TickId::arora_type_id(),
                                    },
                                    &TickId::arora_type_version(),
                                )
                                .unwrap(),
                                mutable: false,
                            },
                        )]),
                        return_ty: FrozenTy::from_type_ref(
                            &TypeRef::Scalar {
                                id: Status::arora_type_id(),
                            },
                            &Status::arora_type_version(),
                        )
                        .unwrap(),
                    };
                }
                (id, function)
            })
            .collect()
    }
    pub fn reset_counts(&mut self) {
        self.calls.clear();
        self.module_calls = 0;
        self.dispatches = 0;
    }
}
/// A call's arguments as the host receives them: owned, through
/// [`CallBridge::arora_call`], or lent, through
/// [`CallBridge::arora_call_borrowed`]. The host reads either in place; it
/// copies lent arguments only to record them, in the order it was given them.
trait Arguments {
    fn get(&self, id: Uuid) -> Option<&Value>;
    fn into_recorded(self) -> Vec<StructureField>;
}
impl Arguments for Vec<StructureField> {
    fn get(&self, id: Uuid) -> Option<&Value> {
        self.iter()
            .find(|field| field.id == id)
            .map(|field| &*field.value)
    }
    fn into_recorded(self) -> Vec<StructureField> {
        self
    }
}
impl Arguments for &[(Uuid, &Value)] {
    fn get(&self, id: Uuid) -> Option<&Value> {
        self.iter()
            .find(|(parameter, _)| *parameter == id)
            .map(|(_, value)| *value)
    }
    fn into_recorded(self) -> Vec<StructureField> {
        self.iter()
            .map(|(id, value)| StructureField {
                id: *id,
                value: Box::new((*value).clone()),
            })
            .collect()
    }
}
impl Bridge {
    /// Answer a call of the function `id`: both entry points answer, count
    /// and record a call alike.
    fn answer(
        &mut self,
        module_id: Option<Uuid>,
        id: Uuid,
        args: impl Arguments,
    ) -> Result<CallResult, CallError> {
        let op = self
            .ops
            .get(&id)
            .copied()
            .ok_or(CallError::FunctionNotFound { id })?;
        assert_eq!(module_id, Some(MODULE));
        if matches!(op, Op::WriteThenTick) {
            let slot = self.raw_write.as_ref().expect("a raw store slot");
            slot.set(Some(Value::F64(self.raw_writes as f64))).unwrap();
            self.raw_writes += 1;
            let children = args.get(CHILDREN).unwrap().clone().into_elements().unwrap();
            self.module_calls += 1;
            if !self.count_only {
                self.calls.push((id, args.into_recorded()));
            }
            let mut status = Status::Success;
            for child in children {
                let child = TickId::try_from(child).unwrap();
                status = CallableId {
                    id: child.callable_id,
                }
                .call(self)?
                .try_into()
                .unwrap();
                if status != Status::Success {
                    break;
                }
            }
            return Ok(CallResult {
                ret: status.into(),
                mutated: Vec::new(),
            });
        }
        let number = |id| match args.get(id) {
            Some(Value::F64(x)) => *x,
            _ => 0.0,
        };
        let (x, y, state) = (number(X), number(Y), number(STATE));
        let mut mutated = Vec::new();
        let ret = match op {
            Op::WriteThenTick => unreachable!("composites tick above"),
            Op::Add => Value::F64(x + y),
            Op::Approach => {
                let next = if (x - state).abs() < 1e-3 {
                    x
                } else {
                    state + (x - state) / 2.0
                };
                mutated.push(StructureField {
                    id: STATE,
                    value: Box::new(Value::F64(next)),
                });
                Value::F64(next)
            }
            Op::Mul => Value::F64(x * y),
            Op::Clamp => Value::F64(x.max(0.0).min(y.abs() + 1.0)),
            Op::Remap => Value::F64((x - y) * 0.5),
            Op::Effect => {
                mutated.push(StructureField {
                    id: STATE,
                    value: Box::new(Value::F64(state + 1.0)),
                });
                Status::Success.into()
            }
            Op::Running(k) => {
                let next = (state as u64 + 1) % (k + 1);
                mutated.push(StructureField {
                    id: STATE,
                    value: Box::new(Value::F64(next as f64)),
                });
                if next == 0 {
                    Status::Success.into()
                } else {
                    Status::Running.into()
                }
            }
        };
        if args.get(OUT).is_some() {
            mutated.push(StructureField {
                id: OUT,
                value: Box::new(ret.clone()),
            });
        }
        self.module_calls += 1;
        if !self.count_only {
            self.calls.push((id, args.into_recorded()));
        }
        Ok(CallResult { ret, mutated })
    }
}
impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        self.answer(call.module_id, call.id, call.args)
    }
    fn arora_call_borrowed(
        &mut self,
        module_id: Option<Uuid>,
        id: Uuid,
        args: &[(Uuid, &Value)],
    ) -> Result<CallResult, CallError> {
        self.answer(module_id, id, args)
    }
    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        self.next += 1;
        self.registered.insert(self.next, callable);
        CallableId { id: self.next }
    }
    fn arora_unregister_callable(&mut self, callable: &CallableId) {
        self.registered.remove(&callable.id);
    }
    fn arora_call_indirect(&mut self, callable: &CallableId) -> Result<Value, CallError> {
        self.dispatches += 1;
        let callable = self.registered[&callable.id].clone();
        callable.call(self)
    }
}
