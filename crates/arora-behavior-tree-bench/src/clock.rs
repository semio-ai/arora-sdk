//! One monotonic clock contract for native and WASI measurements.
use std::time::Instant;

pub trait Clock {
    /// Monotonic microseconds from an arbitrary origin.
    fn now(&self) -> f64;
    fn step_us(&self) -> f64 {
        0.0
    }
    /// Work observer for deterministic virtual-clock checks; real clocks do nothing.
    #[cfg(any(test, feature = "bench-check"))]
    fn tick_finished(&self) {}
}

pub struct Monotonic(Instant);
impl Default for Monotonic {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Clock for Monotonic {
    fn now(&self) -> f64 {
        self.0.elapsed().as_secs_f64() * 1e6
    }
}

/// A tick needs at least 20 clock steps to resolve its latency. Compare the
/// step with the pilot's measured interval divided by its tick count in one place.
pub fn coarse(step_us: f64, replay_us: f64, ticks: usize) -> bool {
    step_us > 0.0 && step_us * ticks as f64 * 20.0 >= replay_us
}

pub struct WithStep(pub Monotonic, pub f64);
impl Clock for WithStep {
    fn now(&self) -> f64 {
        self.0.now()
    }
    fn step_us(&self) -> f64 {
        self.1
    }
}
