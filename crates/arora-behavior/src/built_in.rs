//! Built-in keys: well-known store slots the runtime maintains for every behavior.
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
//! The keys live under the reserved [`PREFIX`] namespace, which the runtime
//! owns: behaviors must not publish their own keys under it. They travel
//! outbound with every other change, so a remote reads the device's clock and
//! derives what it needs from it — a step rate, jitter, a stalled loop. A
//! remote that does not want them filters them on its side.

/// Reserved namespace for runtime-maintained built-in keys. The runtime owns it;
/// behaviors must not write their own keys under this prefix.
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

/// Whether `key` is a reserved built-in key — runtime-owned, so a behavior must
/// not write it, and a consumer that wants to skip the clock can recognize it.
pub fn is_built_in(key: &str) -> bool {
    key.starts_with(PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_keys_share_the_reserved_prefix() {
        assert!(is_built_in(TIME));
        assert!(is_built_in(DT));
        assert!(TIME.starts_with(PREFIX));
        assert!(DT.starts_with(PREFIX));
    }

    #[test]
    fn ordinary_keys_are_not_built_in() {
        assert!(!is_built_in("sensor/x"));
        assert!(!is_built_in("actuator/y"));
        // A key that merely mentions "arora" later in the path is not reserved.
        assert!(!is_built_in("device/arora/time"));
    }
}
