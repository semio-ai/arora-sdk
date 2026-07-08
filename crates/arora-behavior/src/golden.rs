//! Golden keys: well-known store slots the runtime maintains for every behavior.
//!
//! The runtime writes these at the start of each step — before it ticks any
//! behavior — so a behavior reads the current frame's timing straight from the
//! [`store`](super::BehaviorContext::store) instead of taking it as a tick
//! argument. Timing stays out of the [`Behavior`](super::Behavior) API: it is
//! just data, like any other slot, and a behavior (an animation module, a graph
//! time node) derives whatever it needs — `dt`, elapsed time — by reading these
//! keys.
//!
//! The keys live under the reserved [`PREFIX`] namespace. The runtime keeps them
//! local: it never forwards them out to the bridge or hardware, since a
//! wall-clock advancing every frame is noise to a remote. Behaviors must not
//! publish their own keys under this prefix.

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
}
