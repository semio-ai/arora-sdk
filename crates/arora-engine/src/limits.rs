//! What a guest module may spend: the time of one call and the memory of its
//! instance.
//!
//! **Time.** A call to a guest gets, by default, the time left in the step
//! that makes it ([`Engine::set_step_budget`]), so the guest calls of one step
//! cannot together outlast it; a call made outside any step gets
//! [`DEFAULT_CALL_BUDGET`]. A module's [`GuestLimits::call_budget`] tightens
//! the step's, and outside a step replaces the default. A guest still
//! running at its deadline is interrupted: a thread advances an epoch counter
//! every [`EPOCH`], and at each tick a running guest's store compares its
//! deadline with the wall clock, so a call is interrupted at the first tick
//! past its deadline, never before it. Time a guest spends in a host
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
//! A guest's call to another guest ends by its caller's deadline, a guest
//! loading counting as a call to its callees. A module called back while its
//! call is under way ends by that call's deadline, or its caller's if
//! earlier: the call under way is never extended. A caller whose callee ran
//! past the caller's deadline is interrupted once the call returns.
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

/// How often a running guest's deadline is checked: a call overruns its
/// deadline by about this much, as the thread advancing the epoch is
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

/// Bound the calls into `store` by the deadline `deadline_of` reads from its
/// data, which [`Module::set_call_deadline`](crate::module::Module::set_call_deadline)
/// sets before each call. At each tick of the engine's epoch, the store's
/// callback compares the deadline with the wall clock: a call is interrupted
/// at the first tick past its deadline, never before it, whatever ticks the
/// counter has or has not made.
#[cfg(feature = "wasmtime-host")]
pub(crate) fn bound<T: 'static>(
    store: &mut wasmtime::Store<T>,
    deadline_of: fn(&T) -> std::time::Instant,
) {
    store.epoch_deadline_callback(move |state| {
        Ok(on_epoch(
            deadline_of(state.data()),
            std::time::Instant::now(),
        ))
    });
    store.set_epoch_deadline(1);
}

/// What a store does at a tick of its engine's epoch, at `now`, to end its
/// call by `deadline`: interrupt it once the deadline has passed, else check
/// again at the next tick.
#[cfg(feature = "wasmtime-host")]
fn on_epoch(deadline: std::time::Instant, now: std::time::Instant) -> wasmtime::UpdateDeadline {
    if now >= deadline {
        wasmtime::UpdateDeadline::Interrupt
    } else {
        wasmtime::UpdateDeadline::Continue(1)
    }
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

#[cfg(all(test, feature = "wasmtime-host"))]
mod tests {
    use super::*;
    use std::time::Instant;
    use wasmtime::UpdateDeadline;

    #[test]
    fn a_call_is_interrupted_once_its_deadline_has_passed_and_not_before() {
        let now = Instant::now();
        assert!(matches!(on_epoch(now, now), UpdateDeadline::Interrupt));
        assert!(matches!(
            on_epoch(now, now + Duration::from_millis(1)),
            UpdateDeadline::Interrupt
        ));
        assert!(matches!(
            on_epoch(now + Duration::from_micros(1), now),
            UpdateDeadline::Continue(1)
        ));
    }
}
