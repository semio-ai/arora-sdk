//! The limits wasmtime's WASI 0.2 sets on what one call asks of a host,
//! which the browser's component executor holds a guest to as well: a call
//! past one traps on both hosts.

/// What an output stream's `check-write` permits at once: wasmtime's stdio
/// budget (`MAX_READ_SIZE_ALLOC`).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const WRITE_BUDGET: u64 = 64 << 10;

/// What a `blocking-…-and-flush` writes at once.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const BLOCKING_WRITE_LIMIT: u64 = 4096;

/// How many random bytes one call returns at most:
/// `wasmtime_wasi::random::DEFAULT_MAX_SIZE`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const RANDOM_BYTES_LIMIT: u64 = 64 << 20;

#[cfg(all(test, feature = "wasmtime-host"))]
mod tests {
    #[test]
    fn the_random_limit_is_wasmtime_s() {
        assert_eq!(
            super::RANDOM_BYTES_LIMIT,
            wasmtime_wasi::random::DEFAULT_MAX_SIZE
        );
    }
}
