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
//! back-edges, so a call is interrupted after its deadline and less than two
//! epochs past it, as the counter's thread is scheduled. Time a guest spends in a host
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
//! A guest's call to another guest ends by its caller's deadline, and a guest
//! calling back into itself keeps its call's deadline: the call under way is
//! not extended. A caller whose callee ran past the caller's deadline is
//! interrupted once the call returns.
//!
//! **Memory.** Each linear memory of a guest's instance may hold up to
//! [`GuestLimits::memory`] bytes, [`DEFAULT_MEMORY`] by default: a
//! `memory.grow` past it fails inside the guest, which sees the grow return
//! -1, and the host grows nothing. A core module has one memory; a component
//! can hold several, each under the cap.
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
/// call overruns its deadline by less than two of these, as its thread is
/// scheduled.
pub const EPOCH: Duration = Duration::from_millis(1);

/// What one guest module may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestLimits {
    /// The bytes each linear memory of its instance may hold.
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

/// The counter a wasmtime engine's deadlines are checked against, as its
/// ticker advances it: one epoch per [`EPOCH`] since `start`, `ticked` of
/// them made so far.
#[cfg(feature = "wasmtime-host")]
pub(crate) struct Clock {
    start: std::time::Instant,
    ticked: std::sync::atomic::AtomicU64,
}

#[cfg(feature = "wasmtime-host")]
impl Clock {
    /// The deadline a guest's store runs to, as wasmtime counts it: the
    /// epochs still to tick until the one due after `deadline`, and at least
    /// one, so a call made with no time left runs for an epoch rather than
    /// fail unentered.
    ///
    /// It counts from the wall clock the ticker follows, not from the ticks
    /// made: a ticker running late makes up its missed ticks at once, and a
    /// deadline counted from its count would be reached early by as many.
    /// The epoch after the one `deadline` falls in is due once `deadline` has
    /// passed, so a call never ends before it.
    pub(crate) fn epochs_until(&self, deadline: std::time::Instant) -> u64 {
        let due = deadline
            .saturating_duration_since(self.start)
            .as_nanos()
            .div_ceil(EPOCH.as_nanos()) as u64
            + 1;
        due.saturating_sub(self.ticked.load(std::sync::atomic::Ordering::Acquire))
            .max(1)
    }
}

/// Advance `engine`'s epoch every [`EPOCH`], from a thread of its own, for the
/// life of the process; its deadlines are counted with the returned
/// [`Clock`]. Called once per wasmtime engine, when it is created.
#[cfg(feature = "wasmtime-host")]
pub(crate) fn tick(engine: &wasmtime::Engine) -> std::sync::Arc<Clock> {
    use std::sync::atomic::Ordering;
    let clock = std::sync::Arc::new(Clock {
        start: std::time::Instant::now(),
        ticked: std::sync::atomic::AtomicU64::new(0),
    });
    let engine = engine.weak();
    let ticker = clock.clone();
    std::thread::Builder::new()
        .name("arora-epoch".into())
        .spawn(move || {
            // The counter follows the wall clock, not the count of sleeps:
            // every sleep overruns, and a deadline counted in sleeps would
            // drift later by as much.
            while let Some(engine) = engine.upgrade() {
                let due = (ticker.start.elapsed().as_nanos() / EPOCH.as_nanos()) as u64;
                let mut ticked = ticker.ticked.load(Ordering::Relaxed);
                while ticked < due {
                    // The engine's epoch first: `ticked` never counts a tick
                    // the engine has not seen, so a deadline counted from it
                    // is late, if anything, never early.
                    engine.increment_epoch();
                    ticked += 1;
                    ticker.ticked.store(ticked, Ordering::Release);
                }
                drop(engine);
                let next =
                    ticker.start + Duration::from_nanos((ticked + 1) * EPOCH.as_nanos() as u64);
                std::thread::sleep(next.saturating_duration_since(std::time::Instant::now()));
            }
        })
        .expect("spawn the epoch thread");
    clock
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
    fn a_deadline_is_counted_from_the_wall_clock_to_the_epoch_after_it() {
        use std::sync::atomic::AtomicU64;
        use std::time::Instant;
        let now = Instant::now();
        let clock = Clock {
            start: now,
            ticked: AtomicU64::new(0),
        };
        assert_eq!(clock.epochs_until(now), 1);
        assert_eq!(clock.epochs_until(now - Duration::from_secs(1)), 1);
        assert_eq!(clock.epochs_until(now + Duration::from_millis(10)), 11);
        // A ticker 50 epochs late: the deadline counts the ticks it makes up.
        let late = Clock {
            start: now - Duration::from_millis(100),
            ticked: AtomicU64::new(50),
        };
        assert_eq!(late.epochs_until(now + Duration::from_millis(10)), 61);
    }
}
