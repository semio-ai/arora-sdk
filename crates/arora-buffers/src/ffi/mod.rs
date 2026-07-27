//! The arora-buffers C ABI: the `#[no_mangle] extern "C"` entry points that let
//! guest modules — native (C++, `arora-sdk/libs`) and WASM — drive a
//! [`BufferWriter`](crate::writer::BufferWriter) / [`BufferReader`](crate::reader::BufferReader)
//! across the module boundary. The safe Rust API is in `reader` / `writer`;
//! this is the raw-pointer surface over it.

pub mod reader;
pub mod writer;
