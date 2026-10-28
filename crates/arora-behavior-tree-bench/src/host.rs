//! Host actions, linked modules and call accounting for every harness.
use crate::modules::MODULES;
use arora_behavior::{status::Status, FunctionIndex, ModuleFunction};
use arora_dataflow_nodes::functions::{self, describe, Module};
use arora_types::{
    call::{Call, CallBridge, CallError, CallResult, Callable, CallableId},
    module::declared::AroraFunction,
    record::ty::{FrozenTy, PrimitiveKind},
    value::{StructureField, Value},
};
use std::{cell::Cell, collections::HashMap, rc::Rc};
use uuid::Uuid;

const HOST: Module = Module {
    id: Uuid::from_u128(0xb3ac_0000),
    name: "bench",
};
fn id(name: &str) -> Uuid {
    Uuid::new_v5(&HOST.id, name.as_bytes())
}

#[derive(Default)]
pub struct Counts {
    pub module_calls: Cell<u64>,
    pub dispatches: Cell<u64>,
}
#[derive(Default)]
pub struct Bridge {
    real: HashMap<(Uuid, Uuid), AroraFunction>,
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
    gap: f64,
    pub counts: Rc<Counts>,
}
impl Bridge {
    pub fn new(dt: f64) -> Self {
        Self {
            real: MODULES
                .iter()
                .flat_map(|m| (m.exports)().into_iter().map(|f| ((m.id, f.id), f)))
                .collect(),
            gap: 1.5 * dt,
            ..Self::default()
        }
    }
    pub fn index() -> &'static FunctionIndex {
        static INDEX: std::sync::OnceLock<FunctionIndex> = std::sync::OnceLock::new();
        INDEX.get_or_init(Self::build_index)
    }
    fn build_index() -> FunctionIndex {
        let mut index = functions::function_index().clone();
        for (name, inputs) in [
            ("hold", vec!["t", "duration", "value"]),
            (
                "pulse",
                vec!["t", "gap_min", "gap_max", "low", "high", "width", "salt"],
            ),
            (
                "wander",
                vec!["t", "gap_min", "gap_max", "salt", "low", "high"],
            ),
        ] {
            let parameters = inputs
                .into_iter()
                .map(|p| (id(p), p.to_string(), false))
                .chain(
                    [("out", true), ("state", true)]
                        .into_iter()
                        .map(|(p, m)| (id(p), p.to_string(), m)),
                )
                .collect();
            let mut function = describe(HOST, id(name), name, parameters);
            for (parameter, p) in &mut function.function.parameters {
                if *parameter != id("state") {
                    p.ty = FrozenTy::from(PrimitiveKind::F64);
                }
            }
            index.insert(id(name), function);
        }
        for module in MODULES {
            for f in (module.exports)() {
                index.insert(
                    f.id,
                    ModuleFunction {
                        module_id: module.id,
                        function_id: f.id,
                        function_name: f.name.into(),
                        function: f.signature,
                    },
                );
            }
        }
        index
    }
    fn answer(&mut self, call: Call) -> Result<CallResult, CallError> {
        if let Some(function) = self
            .real
            .get_mut(&(call.module_id.unwrap_or_default(), call.id))
        {
            self.counts
                .module_calls
                .set(self.counts.module_calls.get() + 1);
            return (function.invoke)(call);
        }
        if call.module_id != Some(HOST.id) {
            return Err(CallError::FunctionNotFound { id: call.id });
        }
        let get = |name| {
            call.args
                .iter()
                .find(|f| f.id == id(name))
                .map(|f| &*f.value)
        };
        let number = |name| match get(name) {
            Some(Value::F64(x)) => *x,
            _ => 0.0,
        };
        let t = number("t");
        // A state starts as `()`; after a call it holds what the call wrote.
        let mut state = match get("state") {
            None | Some(Value::Unit) => vec![f64::NAN; 3],
            Some(Value::ArrayF64(v)) if v.len() == 3 => v.clone(),
            Some(Value::ArrayF64(v)) => {
                return Err(CallError::Generic {
                    message: format!("a saved state holds 3 numbers, not {}", v.len()),
                })
            }
            Some(other) => {
                return Err(CallError::Generic {
                    message: format!("a saved state starts as (), not {other}"),
                })
            }
        };
        let restart = state[0].is_nan() || t - state[0] > self.gap;
        let mut output = None;
        let status = if call.id == id("hold") {
            if restart {
                state[1] = t;
                output = Some(number("value"));
            }
            let duration = number("duration");
            if duration > 0.0 && t - state[1] >= duration - 1e-9 {
                Status::Success
            } else {
                Status::Running
            }
        } else if call.id == id("pulse") || call.id == id("wander") {
            let salt = number("salt");
            let gap = |salt| {
                number("gap_min") + (number("gap_max") - number("gap_min")) * random(t, salt)
            };
            if restart {
                state[1] = t + gap(salt);
                state[2] = f64::NAN;
            }
            if call.id == id("pulse") && !state[2].is_nan() && t >= state[2] {
                output = Some(0.0);
                state[2] = f64::NAN;
            }
            if t >= state[1] {
                output =
                    Some(number("low") + (number("high") - number("low")) * random(t, salt + 1.0));
                state[2] = t + number("width");
                state[1] = t + gap(salt + if call.id == id("pulse") { 2.0 } else { 100.0 });
            }
            Status::Running
        } else {
            return Err(CallError::FunctionNotFound { id: call.id });
        };
        state[0] = t;
        let mut mutated = vec![StructureField {
            id: id("state"),
            value: Box::new(Value::ArrayF64(state)),
        }];
        if let Some(value) = output.filter(|_| get("out").is_some()) {
            let value = Value::F64(value);
            if get("out") != Some(&value) {
                mutated.push(StructureField {
                    id: id("out"),
                    value: Box::new(value),
                });
            }
        }
        self.counts
            .module_calls
            .set(self.counts.module_calls.get() + 1);
        Ok(CallResult {
            ret: Value::from(status),
            mutated,
        })
    }
}
impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        self.answer(call)
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
        self.registered[&callable.id].clone().call(self)
    }
}

/// Reinstall the observer when a registered callable reenters the dataflow
/// bridge. Forwarded host/module calls are counted only where answered.
pub struct Counting<'a> {
    pub inner: &'a mut dyn CallBridge,
    pub counts: Rc<Counts>,
}
struct Observed {
    callable: Rc<dyn Callable>,
    counts: Rc<Counts>,
}
impl Callable for Observed {
    fn call(&self, bridge: &mut dyn CallBridge) -> Result<Value, CallError> {
        self.callable.call(&mut Counting {
            inner: bridge,
            counts: self.counts.clone(),
        })
    }
}
impl CallBridge for Counting<'_> {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        let own = call
            .module_id
            .is_some_and(|id| functions::MODULES.iter().any(|m| m.id == id));
        let result = self.inner.arora_call(call);
        if own && result.is_ok() {
            self.counts
                .module_calls
                .set(self.counts.module_calls.get() + 1);
        }
        result
    }
    fn arora_call_borrowed(
        &mut self,
        module: Option<Uuid>,
        function: Uuid,
        args: &[(Uuid, &Value)],
    ) -> Result<CallResult, CallError> {
        let own = module.is_some_and(|id| functions::MODULES.iter().any(|m| m.id == id));
        let result = self.inner.arora_call_borrowed(module, function, args);
        if own && result.is_ok() {
            self.counts
                .module_calls
                .set(self.counts.module_calls.get() + 1);
        }
        result
    }
    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        self.inner.arora_register_callable(Rc::new(Observed {
            callable,
            counts: self.counts.clone(),
        }))
    }
    fn arora_unregister_callable(&mut self, callable: &CallableId) {
        self.inner.arora_unregister_callable(callable);
    }
    fn arora_call_indirect(&mut self, callable: &CallableId) -> Result<Value, CallError> {
        self.counts.dispatches.set(self.counts.dispatches.get() + 1);
        self.inner.arora_call_indirect(callable)
    }
}
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}
/// A number in [0, 1) that the time, in whole microseconds, and a salt decide.
/// Microseconds past 2^32 fold their high word in, so draws keep varying on
/// replays longer than 71 minutes; earlier draws are those of the low word.
fn random(t: f64, salt: f64) -> f64 {
    let micros = (t * 1e6).round() as u64;
    let (low, high) = (micros as u32, (micros >> 32) as u32);
    let seed = if high == 0 { low } else { low ^ hash(high) };
    f64::from(hash(seed ^ hash(salt.round() as i64 as u32))) / 4_294_967_296.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_vary_past_two_to_the_32_microseconds() {
        let past = 5_000.0;
        assert!((0..8).any(|i| random(past, 7.0) != random(past + 0.25 * f64::from(i + 1), 7.0)));
        // Before 2^32 us, a draw hashes the microseconds alone.
        assert_eq!(
            random(1.5, 7.0),
            f64::from(hash(1_500_000 ^ hash(7))) / 4_294_967_296.0
        );
    }
}
