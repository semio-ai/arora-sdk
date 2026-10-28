//! Shared behavior-tree benchmark model, forms, measurement and front ends.
pub mod cli;
pub mod clock;
#[cfg(target_os = "wasi")]
mod exports;
pub mod forms;
pub mod host;
pub mod measure;
pub mod model;
pub mod modules;
pub mod scale;
pub mod synthetic;

/// Shared harness defaults, also exposed to JavaScript through the C ABI.
pub const DEFAULT_TICKS: usize = 500;
pub const DEFAULT_WARM_UP: usize = 200;
pub fn scale_name(scenario: &str, n: usize) -> String {
    format!("{scenario}/{n}")
}

#[cfg(any(test, feature = "bench-check"))]
mod checks;
