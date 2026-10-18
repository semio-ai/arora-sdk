//! Time: signed integer nanoseconds.
//!
//! A clip's keypoints sit at whole nanoseconds of clip-local time, which is
//! signed: a keypoint may come before the clip's 0. A device's clock
//! (`arora/time`) is whole nanoseconds too, so a player that reads it runs on
//! integers and does not drift however long it plays.
//!
//! Evaluation takes a local time as an `f64` of nanoseconds, so that a time
//! mapped through an instance's rate (a speed multiplier) keeps its fraction
//! of a nanosecond.

/// A time or a duration in nanoseconds.
pub type Nanos = i64;

/// Nanoseconds in a millisecond.
pub const NANOS_PER_MILLI: i64 = 1_000_000;

/// Nanoseconds in a second.
pub const NANOS_PER_SECOND: i64 = 1_000_000_000;

/// `ms` milliseconds as whole nanoseconds, rounded to the nearest; `None`
/// when `ms` is not finite or lies beyond what `i64` nanoseconds hold
/// (about 292 years either side of 0).
pub fn nanos_from_millis(ms: f64) -> Option<Nanos> {
    let ns = (ms * NANOS_PER_MILLI as f64).round();
    // `i64::MAX as f64` rounds up to 2^63, which no `i64` holds: the bound is
    // exclusive.
    (ns.is_finite() && ns >= i64::MIN as f64 && ns < i64::MAX as f64).then_some(ns as i64)
}

/// `ns` nanoseconds in milliseconds.
pub fn millis_from_nanos(ns: f64) -> f64 {
    ns / NANOS_PER_MILLI as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milliseconds_round_to_the_nearest_nanosecond() {
        assert_eq!(nanos_from_millis(333.3333), Some(333_333_300));
        assert_eq!(nanos_from_millis(-0.0000004), Some(0));
        assert_eq!(nanos_from_millis(-0.0000006), Some(-1));
        assert_eq!(nanos_from_millis(f64::NAN), None);
        assert_eq!(nanos_from_millis(1e300), None);
        assert_eq!(millis_from_nanos(1_500_000.0), 1.5);
    }
}
