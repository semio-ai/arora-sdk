//! One monotonic clock contract for native and WASI measurements.
use std::time::Instant;

pub trait Clock {
    /// Monotonic microseconds from an arbitrary origin.
    fn now(&self) -> f64;
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
