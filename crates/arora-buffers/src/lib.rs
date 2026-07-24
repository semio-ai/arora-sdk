// arora-buffers exposes the `#[no_mangle] extern "C"` buffer ABI consumed by
// WASM guests and host bindings. These entry points take raw pointers by
// contract; marking each one `unsafe` (or documenting a `# Safety` section per
// function) would churn the ABI surface for every caller without making the
// FFI boundary any safer. Suppress the two pointer-hygiene lints crate-wide.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]

pub mod alloc;
pub mod read;
pub mod serde_raw_id;
pub mod serde_uuid;
pub mod typed;
pub mod value_io;
pub mod write;

pub use read::*;
pub use write::*;

pub const TYPE_UNIT: u8 = 0;
pub const TYPE_BOOLEAN: u8 = 1;
pub const TYPE_U8: u8 = 2;
pub const TYPE_U16: u8 = 3;
pub const TYPE_U32: u8 = 4;
pub const TYPE_U64: u8 = 5;
pub const TYPE_I8: u8 = 6;
pub const TYPE_I16: u8 = 7;
pub const TYPE_I32: u8 = 8;
pub const TYPE_I64: u8 = 9;
pub const TYPE_F32: u8 = 10;
pub const TYPE_F64: u8 = 11;
pub const TYPE_STRING: u8 = 12;
pub const TYPE_STRUCTURE: u8 = 13;
pub const TYPE_ENUMERATION: u8 = 14;
pub const TYPE_ARRAY: u8 = 15;
pub const TYPE_MAP: u8 = 16;
pub const TYPE_OPTION: u8 = 17;
pub const TYPE_UUID: u8 = 18;
pub const TYPE_VALUE: u8 = 19;
pub const TYPE_ERROR: u8 = 20;

const ALIGNMENT: usize = 8;

pub const BUFFER_SIZE_SIZE: usize = std::mem::size_of::<u32>();
