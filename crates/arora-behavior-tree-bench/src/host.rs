//! Host functions and call accounting for every harness.
use crate::modules::MODULES;
use arora_behavior::{status::Status, FunctionIndex, ModuleFunction};
use arora_types::{
    call::{Call, CallBridge, CallError, CallResult, Callable, CallableId},
    module::declared::AroraFunction,
    record::{
        module::frozen::{Function, Parameter},
        ty::{FrozenTy, PrimitiveKind},
    },
    value::{StructureField, Value},
};
use std::{collections::HashMap, f64::consts::PI, rc::Rc};
use uuid::Uuid;
/// The damping time base, in seconds.
const DT: f64 = 1.0 / 30.0;
const MODULE: Uuid = Uuid::from_u128(0xb3ac_0000);
pub const TARGET: Uuid = Uuid::from_u128(0xb3ac_0001);
pub const HALF_LIFE: Uuid = Uuid::from_u128(0xb3ac_0002);
pub const STATE: Uuid = Uuid::from_u128(0xb3ac_0003);
pub const VALUE: Uuid = Uuid::from_u128(0xb3ac_0004);

/// Argument `i` of an n-ary function.
pub fn parameter(i: usize) -> Uuid {
    Uuid::from_u128(0xb3ac_1000 + i as u128)
}
/// Output `i` of a tree call: a mutable parameter bound to a key.
pub fn output(i: usize) -> Uuid {
    Uuid::from_u128(0xb3ac_2000 + i as u128)
}
/// State parameter `i` of a tree call.
pub fn state(i: usize) -> Uuid {
    Uuid::from_u128(0xb3ac_3000 + i as u128)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    ChainAdd,
    ChainMul,
    ChainClamp,
    ChainRemap,
    Sum,
    Sub,
    Product,
    Div,
    Clamp,
    Max,
    Remap,
    Overlay,
    First,
    Mix,
    Noise,
    Osc,
    Damp,
    /// Returns its argument: what writes a value to a key.
    Publish,
    Above,
    Hold,
    Follow,
    Pulse,
    Wander,
    Keyframes,
    Set,
}
impl Op {
    pub fn parse(name: &str) -> Op {
        match name {
            "chain-add" => Op::ChainAdd,
            "chain-mul" => Op::ChainMul,
            "chain-clamp" => Op::ChainClamp,
            "chain-remap" => Op::ChainRemap,
            "sum" => Op::Sum,
            "sub" => Op::Sub,
            "product" => Op::Product,
            "div" => Op::Div,
            "clamp" => Op::Clamp,
            "max" => Op::Max,
            "remap" => Op::Remap,
            "overlay" => Op::Overlay,
            "first" => Op::First,
            "mix" => Op::Mix,
            "noise" => Op::Noise,
            "osc" => Op::Osc,
            "damp" => Op::Damp,
            "publish" => Op::Publish,
            "above" => Op::Above,
            "hold" => Op::Hold,
            "follow" => Op::Follow,
            "pulse" => Op::Pulse,
            "wander" => Op::Wander,
            "keyframes" => Op::Keyframes,
            "set" => Op::Set,
            _ => panic!("unknown function {name}"),
        }
    }
    /// The parameter a graph call's argument `i` binds.
    pub fn role(self, i: usize) -> Uuid {
        match self {
            Op::Damp => [TARGET, HALF_LIFE][i],
            Op::Publish => VALUE,
            _ => parameter(i),
        }
    }
}

/// A function's operation and shape: its arguments, outputs and state.
#[derive(Clone, Copy)]
pub struct Signature {
    pub op: Op,
    pub arguments: usize,
    pub outputs: usize,
    pub state: usize,
}
impl Signature {
    /// The parameters, in order, and whether each is mutable.
    fn parameters(self) -> Vec<(Uuid, String, bool)> {
        match self.op {
            Op::ChainAdd | Op::ChainMul | Op::ChainClamp | Op::ChainRemap => vec![
                (parameter(0), "x".into(), false),
                (parameter(1), "y".into(), false),
                (STATE, "state".into(), true),
                (output(0), "out".into(), true),
            ],
            Op::Damp => vec![
                (TARGET, "target".into(), false),
                (HALF_LIFE, "half_life".into(), false),
                (STATE, "state".into(), true),
            ],
            Op::Publish => vec![(VALUE, "value".into(), false)],
            _ => (0..self.arguments)
                .map(|i| (parameter(i), format!("a{i}"), false))
                .chain((0..self.outputs).map(|i| (output(i), format!("o{i}"), true)))
                .chain((0..self.state).map(|i| (state(i), format!("s{i}"), true)))
                .collect(),
        }
    }
}

/// A deterministic 32-bit integer hash.
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}
/// A number in [0, 1) that the time and a salt decide.
fn random(t: f64, salt: f64) -> f64 {
    let seed = (t * 1e6).round() as u32;
    f64::from(hash(seed ^ hash(salt.round() as i64 as u32))) / 4_294_967_296.0
}

/// A call's arguments as the host receives them, owned or lent.
trait Arguments {
    fn get(&self, id: Uuid) -> Option<&Value>;
}
impl Arguments for Vec<StructureField> {
    fn get(&self, id: Uuid) -> Option<&Value> {
        self.iter()
            .find(|field| field.id == id)
            .map(|field| &*field.value)
    }
}
impl Arguments for &[(Uuid, &Value)] {
    fn get(&self, id: Uuid) -> Option<&Value> {
        self.iter()
            .find(|(parameter, _)| *parameter == id)
            .map(|(_, value)| *value)
    }
}

#[derive(Default)]
pub struct Bridge {
    real: HashMap<(Uuid, Uuid), AroraFunction>,
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
    // Restart after a missed tick; Arora has no halt callback.
    gap: f64,
    pub functions: HashMap<Uuid, Signature>,
    pub module_calls: u64,
    pub dispatches: u64,
}
impl Bridge {
    pub fn new(functions: HashMap<Uuid, Signature>, dt: f64) -> Self {
        Self {
            functions,
            real: MODULES
                .iter()
                .flat_map(|m| (m.exports)().into_iter().map(|f| ((m.id, f.id), f)))
                .collect(),
            gap: 1.5 * dt,
            ..Self::default()
        }
    }
    pub fn index(&self) -> FunctionIndex {
        self.functions
            .iter()
            .map(|(id, signature)| {
                let parameters = signature.parameters();
                let function = ModuleFunction {
                    module_id: MODULE,
                    function_id: *id,
                    function_name: format!("{:?}", signature.op),
                    function: Function {
                        parameter_ordering: parameters.iter().map(|(id, _, _)| *id).collect(),
                        parameters: parameters
                            .into_iter()
                            .map(|(id, name, mutable)| {
                                let ty = FrozenTy::from(PrimitiveKind::F64);
                                (id, Parameter { name, ty, mutable })
                            })
                            .collect(),
                        return_ty: FrozenTy::from(PrimitiveKind::F64),
                    },
                };
                (*id, function)
            })
            .chain(self.real.iter().map(|((module_id, id), f)| {
                (
                    *id,
                    ModuleFunction {
                        module_id: *module_id,
                        function_id: *id,
                        function_name: f.name.into(),
                        function: f.signature.clone(),
                    },
                )
            }))
            .collect()
    }
    fn answer(
        &mut self,
        module_id: Option<Uuid>,
        id: Uuid,
        args: impl Arguments,
    ) -> Result<CallResult, CallError> {
        let signature = self
            .functions
            .get(&id)
            .copied()
            .ok_or(CallError::FunctionNotFound { id })?;
        assert_eq!(module_id, Some(MODULE));
        let number = |id| match args.get(id) {
            Some(Value::F64(x)) => Some(*x),
            _ => None,
        };
        let n = signature.arguments;
        let a = |i: usize| number(parameter(i)).unwrap_or(0.0);
        let mut mutated = Vec::new();
        // A tree call's output or state changes only when its last write in
        // the call differs from the value it was called with: a later write
        // replaces an earlier one, so two keyframes due in one tick leave the
        // second frame's value.
        let mut write = |id: Uuid, value: f64| {
            mutated.retain(|field: &StructureField| field.id != id);
            let same = number(id).is_some_and(|x| x == value || (x.is_nan() && value.is_nan()));
            if !same {
                mutated.push(StructureField {
                    id,
                    value: Box::new(Value::F64(value)),
                });
            }
        };
        let s = |i: usize| number(state(i)).unwrap_or(f64::NAN);
        // A tree call restarts when it was not called on the previous tick;
        // `last` is its state parameter `at`.
        let restart = |at: usize, t: f64| s(at).is_nan() || t - s(at) > self.gap;
        let ret = match signature.op {
            Op::ChainAdd => Value::F64(a(0) + a(1)),
            Op::ChainMul => Value::F64(a(0) * a(1)),
            Op::ChainClamp => Value::F64(a(0).max(0.0).min(a(1).abs() + 1.0)),
            Op::ChainRemap => Value::F64((a(0) - a(1)) * 0.5),
            Op::Sum => Value::F64((0..n).map(a).fold(0.0, |sum, x| sum + x)),
            Op::Sub => Value::F64(a(0) - a(1)),
            Op::Product => Value::F64((0..n).map(a).fold(1.0, |product, x| product * x)),
            Op::Div if a(1) == 0.0 => Value::F64(0.0),
            Op::Div => Value::F64(a(0) / a(1)),
            Op::Clamp => Value::F64(a(0).max(a(1)).min(a(2))),
            Op::Max => Value::F64((0..n).map(a).fold(f64::NEG_INFINITY, f64::max)),
            Op::Remap if a(2) == a(1) => Value::F64(a(3)),
            Op::Remap => Value::F64(a(3) + (a(0) - a(1)) * (a(4) - a(3)) / (a(2) - a(1))),
            Op::Overlay => Value::F64(a(3) + a(0)),
            Op::First => Value::F64(a(0)),
            Op::Mix => Value::F64(
                (0..n)
                    .map(|i| a(i) * 0.5f64.powi(i as i32 + 1))
                    .fold(0.0, |sum, x| sum + x),
            ),
            Op::Noise => Value::F64((1.3 * a(0) + 0.7 * a(1)).sin()),
            Op::Osc => Value::F64((2.0 * PI * a(0) * a(2) + a(1)).sin()),
            Op::Publish => Value::F64(number(VALUE).unwrap_or(0.0)),
            Op::Damp => {
                // The first call starts from its target; the state then eases
                // toward it and snaps once it is within 1e-4, so it settles.
                let target = number(TARGET).unwrap_or(0.0);
                let state = number(STATE).filter(|x| !x.is_nan()).unwrap_or(target);
                let half_life = number(HALF_LIFE).unwrap_or(0.12);
                let mut next = state + (target - state) * (1.0 - (-DT / half_life).exp2());
                if (target - next).abs() < 1e-4 {
                    next = target;
                }
                mutated.push(StructureField {
                    id: STATE,
                    value: Box::new(Value::F64(next)),
                });
                Value::F64(next)
            }
            Op::Above => Value::from(if a(0) > a(1) {
                Status::Success
            } else {
                Status::Failure
            }),
            Op::Hold => {
                let (t, duration) = (a(0), a(1));
                let mut since = s(0);
                if restart(1, t) {
                    since = t;
                    for i in 0..signature.outputs {
                        write(output(i), a(2 + i));
                    }
                }
                write(state(0), since);
                write(state(1), t);
                Value::from(if duration > 0.0 && t - since >= duration - 1e-9 {
                    Status::Success
                } else {
                    Status::Running
                })
            }
            Op::Follow => {
                for i in 0..signature.outputs {
                    write(output(i), a(i));
                }
                Value::from(Status::Running)
            }
            Op::Pulse => {
                let (t, low, high, width, salt) = (a(0), a(3), a(4), a(5), a(6));
                let gap = |salt| a(1) + (a(2) - a(1)) * random(t, salt);
                let (mut next, mut release) = (s(1), s(2));
                if restart(0, t) {
                    next = t + gap(salt);
                    release = f64::NAN;
                }
                if !release.is_nan() && t >= release {
                    write(output(0), 0.0);
                    release = f64::NAN;
                }
                if t >= next {
                    write(output(0), low + (high - low) * random(t, salt + 1.0));
                    release = t + width;
                    next = t + gap(salt + 2.0);
                }
                write(state(0), t);
                write(state(1), next);
                write(state(2), release);
                Value::from(Status::Running)
            }
            Op::Wander => {
                let (t, salt) = (a(0), a(3));
                let gap = |salt| a(1) + (a(2) - a(1)) * random(t, salt);
                let mut next = s(1);
                if restart(0, t) {
                    next = t + gap(salt);
                }
                if t >= next {
                    for i in 0..signature.outputs {
                        let (low, high) = (a(4 + 2 * i), a(5 + 2 * i));
                        write(
                            output(i),
                            low + (high - low) * random(t, salt + 1.0 + i as f64),
                        );
                    }
                    next = t + gap(salt + 100.0);
                }
                write(state(0), t);
                write(state(1), next);
                Value::from(Status::Running)
            }
            Op::Keyframes => {
                let (t, width) = (a(0), 1 + signature.outputs);
                let frames = (n - 2) / width;
                let (mut since, mut frame) = (s(0), s(2));
                if restart(1, t) {
                    since = t;
                    frame = 0.0;
                }
                while (frame as usize) < frames && t - since >= a(2 + frame as usize * width) - 1e-9
                {
                    for i in 0..signature.outputs {
                        write(output(i), a(3 + frame as usize * width + i));
                    }
                    frame += 1.0;
                }
                write(state(0), since);
                write(state(1), t);
                write(state(2), frame);
                Value::from(if frame as usize >= frames {
                    Status::Success
                } else {
                    Status::Running
                })
            }
            Op::Set => {
                for i in 0..signature.outputs {
                    write(output(i), a(i));
                }
                Value::from(Status::Success)
            }
        };
        self.module_calls += 1;
        Ok(CallResult { ret, mutated })
    }
}
impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        if call
            .module_id
            .is_some_and(|id| MODULES.iter().any(|m| m.id == id))
        {
            let function = self
                .real
                .get_mut(&(call.module_id.unwrap(), call.id))
                .ok_or(CallError::FunctionNotFound { id: call.id })?;
            self.module_calls += 1;
            return (function.invoke)(call);
        }
        self.answer(call.module_id, call.id, call.args)
    }
    fn arora_call_borrowed(
        &mut self,
        module_id: Option<Uuid>,
        id: Uuid,
        args: &[(Uuid, &Value)],
    ) -> Result<CallResult, CallError> {
        if module_id.is_some_and(|id| MODULES.iter().any(|m| m.id == id)) {
            return self.arora_call(Call {
                module_id,
                id,
                args: args
                    .iter()
                    .map(|(id, value)| StructureField {
                        id: *id,
                        value: Box::new((*value).clone()),
                    })
                    .collect(),
            });
        }
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
