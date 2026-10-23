//! What a guest module may spend: the time of one call and the memory of its
//! instance.
//!
//! **Time.** A call to a guest gets, by default, the time left in the step
//! that makes it ([`Engine::set_step_budget`]), so the guest calls of one step
//! cannot together outlast it; a call made outside any step gets
//! [`DEFAULT_CALL_BUDGET`]. A module's [`GuestLimits::call_budget`] tightens
//! the step's, and outside a step replaces the default. A guest still
//! running at its deadline is interrupted: wasmtime checks an epoch counter,
//! which a thread advances every [`EPOCH`], at function entries and loop
//! back-edges, so a call returns within its deadline plus one epoch. Time a guest spends in a host
//! function, a WASI wait or a call to another module, is not interrupted:
//! the guest is, once the host function returns. Its call fails with
//! [`DispatchError::Trap`], naming the module, the function and the budget,
//! and the module refuses its later calls until it is reloaded: an
//! interrupted call can leave its state half updated.
//!
//! At a device's 15 Hz, one step's guest calls share 66.7 ms; at 60 Hz,
//! 16.7 ms. Loading a module (its `_initialize`, a component's `functions`)
//! runs under [`DEFAULT_CALL_BUDGET`].
//!
//! **Memory.** A guest's instance may hold up to [`GuestLimits::memory`]
//! bytes of linear memory, [`DEFAULT_MEMORY`] by default: a `memory.grow`
//! past it fails inside the guest, which sees the grow return -1, and the
//! host grows nothing.
//!
//! **In the browser** a guest runs on the JS thread, which nothing
//! interrupts: the browser executors bound neither a call's time nor, beyond
//! the maximum a module's memory declares, its memory.
//!
//! [`Engine::set_step_budget`]: crate::engine::Engine::set_step_budget
//! [`DispatchError::Trap`]: crate::module::DispatchError::Trap

use std::time::Duration;

/// The time a call to a guest gets when no step bounds it: a call made
/// outside a device's step, or a module loading.
pub const DEFAULT_CALL_BUDGET: Duration = Duration::from_secs(1);

/// The linear memory a guest's instance may hold unless its module says
/// otherwise: 256 MiB.
pub const DEFAULT_MEMORY: usize = 256 << 20;

/// How often the counter a guest's deadline is checked against advances: a
/// call overruns its deadline by at most this much.
pub const EPOCH: Duration = Duration::from_millis(1);

/// What one guest module may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestLimits {
    /// The linear memory its instance may hold, in bytes.
    pub memory: usize,
    /// The most a call to it may take. `None`: the time left in the step, or
    /// [`DEFAULT_CALL_BUDGET`] outside one. A budget longer than the time
    /// left in the step does not extend it.
    pub call_budget: Option<Duration>,
}

impl Default for GuestLimits {
    fn default() -> Self {
        Self {
            memory: DEFAULT_MEMORY,
            call_budget: None,
        }
    }
}

/// The deadline a guest's store runs to, as wasmtime counts it: the epochs
/// from now until `deadline`, rounded up and at least one, so a call made
/// with no time left still runs for one epoch rather than failing unentered.
#[cfg(feature = "wasmtime-host")]
pub(crate) fn epochs_until(deadline: std::time::Instant) -> u64 {
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    let epochs = left.as_nanos().div_ceil(EPOCH.as_nanos());
    (epochs as u64).max(1)
}

/// Advance `engine`'s epoch every [`EPOCH`], from a thread of its own, for the
/// life of the process. Called once per wasmtime engine, when it is created.
#[cfg(feature = "wasmtime-host")]
pub(crate) fn tick(engine: &wasmtime::Engine) {
    let engine = engine.weak();
    std::thread::Builder::new()
        .name("arora-epoch".into())
        .spawn(move || {
            while let Some(engine) = engine.upgrade() {
                engine.increment_epoch();
                drop(engine);
                std::thread::sleep(EPOCH);
            }
        })
        .expect("spawn the epoch thread");
}

/// The message of a call interrupted at its deadline.
#[cfg(feature = "wasmtime-host")]
pub(crate) fn overran(module: &str, function: &uuid::Uuid, budget: Duration) -> String {
    // Tenths of a millisecond: a budget left over from a step is rarely whole.
    let ms = (budget.as_secs_f64() * 10_000.0).round() / 10.0;
    format!(
        "the call to {function} of '{module}' overran its budget of {ms} ms and was interrupted"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "wasmtime-host")]
    #[test]
    fn a_deadline_is_counted_in_whole_epochs_and_at_least_one() {
        let now = std::time::Instant::now();
        assert_eq!(epochs_until(now), 1);
        assert_eq!(epochs_until(now - Duration::from_secs(1)), 1);
        let ten = epochs_until(now + Duration::from_millis(10));
        assert!((9..=10).contains(&ten), "{ten}");
    }
}
