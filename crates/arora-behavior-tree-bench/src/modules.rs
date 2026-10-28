//! Real functions linked into the benchmark, without the engine buffer ABI.
use arora_types::{module::declared::AroraFunction, Uuid};
use std::sync::{Mutex, MutexGuard};

pub struct Module {
    pub name: &'static str,
    pub id: Uuid,
    pub exports: fn() -> Vec<AroraFunction>,
    pub reset: fn(),
}

/// Adding a module takes a path dependency and one entry here.
pub static MODULES: &[Module] = &[Module {
    name: "silent-say",
    id: say_silent::silent::ids::MODULE,
    exports: say_silent::silent::exports,
    reset: say_silent::reset,
}];

static REPLAY: Mutex<()> = Mutex::new(());

pub(crate) fn fresh_device() -> MutexGuard<'static, ()> {
    // Real modules can keep process-global state. Hold this lock through reset,
    // lowering and the whole replay so parallel tests cannot share a device.
    let guard = REPLAY.lock().unwrap_or_else(|e| e.into_inner());
    for module in MODULES {
        (module.reset)();
    }
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Bridge;
    use arora_types::{
        call::{Call, CallBridge},
        value::{StructureField, Value},
    };

    /// Introspection and both bridge entry points preserve the declaration,
    /// including mutable outputs whose value did not change.
    #[test]
    fn bridge_uses_frozen_signatures_and_unfiltered_mutations() {
        let _device = fresh_device();
        let mut bridge = Bridge::new(Default::default(), 0.02);
        for module in MODULES {
            for function in (module.exports)() {
                let index = bridge.index();
                let indexed = &index[&function.id];
                assert_eq!(indexed.module_id, module.id);
                assert_eq!(indexed.function_name, function.name);
                assert_eq!(indexed.function, function.signature);
            }
        }
        use say_silent::silent::ids::{self, say};
        let args = vec![
            StructureField {
                id: say::TEXT,
                value: Box::new(Value::String(String::new())),
            },
            StructureField {
                id: say::VOICE,
                value: Box::new(Value::String(String::new())),
            },
            StructureField {
                id: say::VISEME,
                value: Box::new(Value::String("sil".into())),
            },
        ];
        let call = Call {
            module_id: Some(ids::MODULE),
            id: say::FUNCTION,
            args,
        };
        let owned = bridge.arora_call(call.clone()).unwrap();
        let args = call
            .args
            .iter()
            .map(|f| (f.id, f.value.as_ref()))
            .collect::<Vec<_>>();
        let borrowed = bridge
            .arora_call_borrowed(call.module_id, call.id, &args)
            .unwrap();
        assert_eq!(owned, borrowed);
        assert_eq!(
            owned.mutated,
            vec![StructureField {
                id: say::VISEME,
                value: Box::new(Value::String("sil".into()))
            }]
        );
        assert_eq!(bridge.module_calls, 2);
    }
}
