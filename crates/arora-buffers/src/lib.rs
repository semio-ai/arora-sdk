// arora-buffers exposes the `#[no_mangle] extern "C"` buffer ABI — the common
// interop surface for guest modules, native (C++, `arora-sdk/libs`) and WASM
// alike. These entry points take raw pointers by contract; marking each one
// `unsafe` (or documenting a `# Safety` section per function) would churn the
// ABI surface for every caller without making the FFI boundary any safer.
// Suppress the two pointer-hygiene lints crate-wide.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]

pub mod alloc;
pub mod ffi;
pub mod format;
pub mod froto_borrowed_value;
pub mod froto_checked_value;
pub mod froto_serde;
pub mod froto_value;
pub mod reader;
pub mod writer;

pub use format::*;
pub use reader::*;
pub use writer::*;

/// Migration alias: `serde_uuid` was the old name of the generic `Value` codec,
/// now [`froto_value`]. External consumers — arora-engine, generated modules,
/// the module-authoring codegen — still reach it under the old path until they
/// move to `froto_value`.
pub use froto_value as serde_uuid;
