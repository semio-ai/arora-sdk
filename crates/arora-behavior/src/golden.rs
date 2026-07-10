//! Golden keys and functions: the well-known names the runtime maintains for
//! every behavior.
//!
//! The runtime writes these at the start of each step — before it ticks any
//! behavior — so a behavior reads the current frame's timing straight from the
//! [`store`](super::BehaviorContext::store) instead of taking it as a tick
//! argument. Timing stays out of the
//! [`BehaviorInterpreter`](super::BehaviorInterpreter) API: it is
//! just data, like any other slot, and a behavior (an animation module, a graph
//! time node) derives whatever it needs — `dt`, elapsed time — by reading these
//! keys.
//!
//! The keys live under the reserved [`PREFIX`] namespace. The runtime keeps them
//! local: it never forwards them out to the bridge or hardware, since a
//! wall-clock advancing every frame is noise to a remote. Behaviors must not
//! publish their own keys under this prefix.

use arora_types::call::Call;
use arora_types::value::{StructureField, Value};
use uuid::{uuid, Uuid};

use crate::graph::GraphDiff;

/// Reserved namespace for runtime-maintained golden keys. The runtime owns it
/// and does not forward it outbound; behaviors must not write their own keys
/// under this prefix.
pub const PREFIX: &str = "arora/";

/// Monotonic **nanoseconds** since the runtime started, as a
/// [`U64`](arora_types::value::Value::U64) value. The runtime advances it by the
/// step's `dt` before each tick. Integer nanoseconds are exact (no float drift
/// over a long run) and give ~584 years of range before `u64` overflows.
pub const TIME: &str = "arora/time";

/// **Nanoseconds** elapsed since the previous step, as a
/// [`U64`](arora_types::value::Value::U64) value — the step's `dt`, surfaced for
/// behaviors that pace themselves (an animation module, a graph time node).
pub const DT: &str = "arora/dt";

/// Whether `key` is a reserved golden key: runtime-owned and never forwarded
/// outbound. The runtime uses this to keep the golden namespace out of the
/// changes it flushes to the bridge and hardware each step.
pub fn is_golden(key: &str) -> bool {
    key.starts_with(PREFIX)
}

// ---- golden functions ------------------------------------------------------

/// Module id under which the runtime registers its golden behavior functions
/// on the engine. Like the vizij type ids, it is self-identifying: the ASCII
/// bytes of "arora" lead the UUID, a small offset tails it.
pub const BEHAVIOR_MODULE: Uuid = uuid!("61726f72-6100-0000-0000-000000000001");

/// Function id of the behavior edit: apply a [`GraphDiff`] to the running
/// interpreter. The runtime registers it against the injected interpreter at
/// build, so a `Call{module_id: BEHAVIOR_MODULE, id: APPLY_DIFF}` — from a
/// remote through the bridge, or from a behavior through its
/// [`BehaviorContext`](super::BehaviorContext) — reaches
/// [`BehaviorInterpreter::apply`](super::BehaviorInterpreter::apply) through
/// the engine's normal dispatch.
pub const APPLY_DIFF: Uuid = uuid!("61726f72-6100-0000-0000-000000000002");

/// Argument id of [`APPLY_DIFF`]'s one argument: the [`GraphDiff`], serialized
/// as a JSON string.
pub const APPLY_DIFF_ARG: Uuid = uuid!("61726f72-6100-0000-0000-000000000003");

/// Build the [`Call`] that applies `diff` to the running behavior — the caller
/// side of the golden-edit convention ([`decode_apply`] is the callee side).
pub fn encode_apply(diff: &GraphDiff) -> Call {
    let json = serde_json::to_string(diff).expect("a GraphDiff serializes");
    Call {
        module_id: Some(BEHAVIOR_MODULE),
        id: APPLY_DIFF,
        args: vec![StructureField {
            id: APPLY_DIFF_ARG,
            value: Box::new(Value::String(json)),
        }],
    }
}

/// Read the [`GraphDiff`] out of an [`APPLY_DIFF`] call — the callee side of
/// the golden-edit convention.
pub fn decode_apply(call: &Call) -> Result<GraphDiff, String> {
    if call.id != APPLY_DIFF {
        return Err(format!("not the behavior-edit function: {}", call.id));
    }
    let arg = call
        .args
        .iter()
        .find(|arg| arg.id == APPLY_DIFF_ARG)
        .ok_or("the edit call is missing its diff argument")?;
    let Value::String(json) = arg.value.as_ref() else {
        return Err("the diff argument must be a JSON string".to_string());
    };
    serde_json::from_str(json).map_err(|e| format!("malformed GraphDiff: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_keys_share_the_reserved_prefix() {
        assert!(is_golden(TIME));
        assert!(is_golden(DT));
        assert!(TIME.starts_with(PREFIX));
        assert!(DT.starts_with(PREFIX));
    }

    #[test]
    fn ordinary_keys_are_not_golden() {
        assert!(!is_golden("sensor/x"));
        assert!(!is_golden("actuator/y"));
        // A key that merely mentions "arora" later in the path is not reserved.
        assert!(!is_golden("device/arora/time"));
    }

    #[test]
    fn apply_call_round_trips_the_diff() {
        let diff = GraphDiff {
            remove_nodes: vec![Uuid::from_u128(7)],
            set_root: Some(Uuid::from_u128(9)),
            ..Default::default()
        };
        let call = encode_apply(&diff);
        assert_eq!(call.module_id, Some(BEHAVIOR_MODULE));
        assert_eq!(call.id, APPLY_DIFF);
        assert_eq!(decode_apply(&call).unwrap(), diff);
    }

    #[test]
    fn decode_apply_rejects_malformed_calls() {
        // Wrong function id.
        let mut call = encode_apply(&GraphDiff::default());
        call.id = Uuid::from_u128(1);
        assert!(decode_apply(&call).is_err());

        // Missing the diff argument.
        let mut call = encode_apply(&GraphDiff::default());
        call.args.clear();
        assert!(decode_apply(&call).is_err());

        // The argument is not a JSON string.
        let mut call = encode_apply(&GraphDiff::default());
        call.args[0].value = Box::new(Value::Boolean(true));
        assert!(decode_apply(&call).is_err());

        // The string is not a GraphDiff.
        let mut call = encode_apply(&GraphDiff::default());
        call.args[0].value = Box::new(Value::String("not json".to_string()));
        assert!(decode_apply(&call).is_err());
    }
}
