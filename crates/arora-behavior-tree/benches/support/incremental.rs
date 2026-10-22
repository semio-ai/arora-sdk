//! Small deterministic module host shared by the incremental tests and bench.
use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::{FunctionIndex, ModuleFunction, Status};
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use uuid::Uuid;

pub const X: Uuid = Uuid::from_u128(0x10001);
pub const Y: Uuid = Uuid::from_u128(0x10002);
pub const STATE: Uuid = Uuid::from_u128(0x10003);
pub const OUT: Uuid = Uuid::from_u128(0x10004);
pub const MODULE: Uuid = Uuid::from_u128(0x10000);

#[derive(Clone, Copy)]
pub enum Op {
    Add,
    Mul,
    Clamp,
    Remap,
    #[allow(dead_code)] // Exercised by the tests sharing this host.
    Effect,
    #[allow(dead_code)]
    Running(u64),
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
}
impl Bridge {
    pub fn index(&self) -> FunctionIndex {
        self.ops.keys().map(|id| signature(*id)).collect()
    }
    pub fn reset_counts(&mut self) {
        self.calls.clear();
        self.module_calls = 0;
        self.dispatches = 0;
    }
}
impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        let op = self
            .ops
            .get(&call.id)
            .copied()
            .ok_or(CallError::FunctionNotFound { id: call.id })?;
        assert_eq!(call.module_id, Some(MODULE));
        let value = |id| {
            call.args
                .iter()
                .find(|field| field.id == id)
                .map(|field| &*field.value)
        };
        let number = |id| match value(id) {
            Some(Value::F64(x)) => *x,
            _ => 0.0,
        };
        let (x, y, state) = (number(X), number(Y), number(STATE));
        let mut mutated = Vec::new();
        let ret = match op {
            Op::Add => Value::F64(x + y),
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
        if value(OUT).is_some() {
            mutated.push(StructureField {
                id: OUT,
                value: Box::new(ret.clone()),
            });
        }
        self.module_calls += 1;
        if !self.count_only {
            self.calls.push((call.id, call.args));
        }
        Ok(CallResult { ret, mutated })
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
